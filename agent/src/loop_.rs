use crate::compact;
use crate::provider::{self, Message, StreamKind};
use crate::settings::Settings;
use crate::tools::{PermGate, ToolCtx, ToolRegistry};
use anyhow::Result;
use ide_core::{AgentEvent, AgentRole, ShellEvent, ShellRegistry, WorkspaceRoot};
use sandbox::Sandbox;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// Everything a run needs from the outside world, bundled once in the HTTP
/// layer and passed down through every role of the pipeline.
#[derive(Clone)]
pub struct RunEnv {
    pub ws: WorkspaceRoot,
    pub sandbox: Arc<dyn Sandbox>,
    pub events: broadcast::Sender<AgentEvent>,
    /// Terminal output channel; agent commands stream under id `"agent"`.
    pub shell_tx: broadcast::Sender<ShellEvent>,
    pub shells: ShellRegistry,
    pub cancel: CancellationToken,
    pub settings: Settings,
    /// Paths this task actually wrote. Lets the pipeline tell "done" apart from
    /// "talked about it and changed nothing".
    pub writes: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Bytes of whole-file content served this task; bounds runaway reading.
    pub read_budget: Arc<std::sync::atomic::AtomicUsize>,
    /// Content each written path had the *first* time this task touched it —
    /// `None` means the path did not exist yet. Lets "undo this message" put
    /// every file it touched back exactly where it started, without a commit.
    pub before: Arc<std::sync::Mutex<std::collections::HashMap<String, Option<String>>>>,
}

/// Tools that only read, so one turn's calls to them can run concurrently —
/// Codex's parallel tool calls. Delegation is here too: several `task` calls
/// in one turn are how the model fans out to subagents.
const PARALLEL_SAFE: &[&str] = &[
    "read_file", "list_files", "search_files", "web_search", "web_fetch", "skill", "task",
    "spawn_agent", "wait_agent",
];

/// Cancels a conversation's unfinished background subagents on every exit
/// path, including errors and Stop.
struct CloseAgents(crate::tools::AgentPool);
impl Drop for CloseAgents {
    fn drop(&mut self) {
        self.0.close_all();
    }
}

/// One conversation: stream → run tool calls → repeat until the model answers
/// without calling a tool. Returns that answer and the full transcript, so a
/// caller can continue the same conversation (the orchestrator feeds failing
/// checks back this way instead of starting a fresh agent from nothing).
///
/// Never sends `Done` — the run has exactly one, from whoever owns the run.
/// `speaks` decides whether text streams to the user as the answer or is
/// shown as reasoning.
#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    role: AgentRole,
    prompt: String,
    model: String,
    effort: String,
    tools: ToolRegistry,
    speaks: bool,
    // Tool-call turns before this role gives up and hands back whatever it
    // has. Callers size it to the job.
    max_turns: u32,
    env: &RunEnv,
    seed: Vec<Message>,
    perm: Option<PermGate>,
    perms: ide_core::PermissionSet,
    spawn_subagent: Option<crate::tools::SubagentRunner>,
) -> Result<(String, Vec<Message>)> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    // Skills ride in the system prompt rather than waiting to be fetched: the
    // `skill` tool alone meant they only applied when the model remembered to
    // ask. The system prompt is also the cached prefix, so this is billed in
    // full once and at roughly a tenth on every turn after.
    let (skill_text, preloaded) = crate::skills::preload(&env.ws);
    let all_skills = crate::skills::discover(&env.ws);
    let schemas = tools.schemas_with_loaded(&all_skills, &preloaded);
    let ctx = ToolCtx {
        ws: env.ws.clone(),
        sandbox: env.sandbox.clone(),
        events: env.events.clone(),
        shell_tx: env.shell_tx.clone(),
        shells: env.shells.clone(),
        perm,
        perms,
        spawn_subagent,
        // One cache per conversation, not per task. It used to be shared by
        // every role "so the coder does not re-read what the planner just
        // read" — but each role runs its own conversation, so the planner's
        // file contents were never in front of the coder. It was told the
        // contents were already above, they were not, and it either re-read in
        // a loop or gave up and wrote nothing.
        reads: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        writes: env.writes.clone(),
        read_budget: env.read_budget.clone(),
        before: env.before.clone(),
        agents: Default::default(),
    };
    let _close = CloseAgents(ctx.agents.clone());
    let mut messages = seed;
    messages.push(Message::user_text(prompt));
    let system = format!("{}{skill_text}", system_prompt(role, &ctx.ws));
    let mut last_text = String::new();
    // The most accurate conversation size seen so far: the provider's own
    // input-token report beats any local character estimate. Shared because
    // the streaming callback owns its captures.
    let measured_input = Arc::new(std::sync::Mutex::new(None::<u64>));
    // OpenHands' stuck detector, the core case: the same call returning the
    // same result again and again. Warn at the third, stop at the fifth.
    let mut loop_guard = LoopGuard::default();

    for _ in 0..max_turns {
        if env.cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let limit = provider::context_limit(&model, &env.settings);
        report_context(&env.events, &messages, limit, *measured_input.lock().unwrap());
        // Prune old tool outputs / summarize old turns once the window fills.
        let deps = compact::CompactDeps {
            client: &client,
            model: model.clone(),
            settings: &env.settings,
            cancel: &env.cancel,
        };
        let _ = compact::compact(&mut messages, limit, &deps, |msg| {
            let _ = env.events.send(AgentEvent::Status { message: msg.into() });
        })
        .await;

        // The stream itself is cancellation-aware: Stop aborts the HTTP read
        // mid-flight instead of waiting out a long generation.
        let ev_tx = env.events.clone();
        let measured_cb = measured_input.clone();
        let turn = provider::stream(
            &client,
            &model,
            &system,
            &schemas,
            &messages,
            &effort,
            &env.settings,
            &env.cancel,
            move |k| match k {
                StreamKind::Usage { input, output } => {
                    if let Some(i) = input {
                        let mut m = measured_cb.lock().unwrap();
                        *m = Some(m.map_or(i, |p| p.max(i)));
                    }
                    if let Some(o) = output {
                        let _ = ev_tx.send(AgentEvent::Usage { tokens: o });
                    }
                }
                // Reasoning was being dropped on the floor, so the "thinking"
                // block in the feed never had anything to show.
                StreamKind::ThinkDelta(t) => {
                    let _ = ev_tx.send(AgentEvent::Think { text: t });
                }
                StreamKind::TextDelta(t) => {
                    // Only the role that closes the run speaks to the user. A
                    // planner or reviewer streaming as answer text is why an
                    // internal plan ("Implementation plan: 1. Scaffold…") was
                    // being narrated at the user as if it were the reply.
                    let _ = ev_tx.send(if speaks {
                        AgentEvent::Token { text: t }
                    } else {
                        AgentEvent::Think { text: t }
                    });
                }
                StreamKind::ToolUse { name, input } => {
                    let _ = ev_tx.send(AgentEvent::ToolCall { name, input });
                }
                StreamKind::Error(message) => {
                    let _ = ev_tx.send(AgentEvent::Error { message });
                }
            },
        )
        .await?;

        last_text = turn.text.clone();
        if turn.tools.is_empty() {
            if !last_text.is_empty() {
                messages.push(Message {
                    role: "assistant".into(),
                    content: Some(json!(last_text)),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
            return Ok((last_text, messages));
        }

        let tool_calls: Vec<_> = turn
            .tools
            .iter()
            .map(|t| {
                json!({
                    "id": t.id,
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "arguments": t.input.to_string(),
                    }
                })
            })
            .collect();
        messages.push(Message {
            role: "assistant".into(),
            content: if last_text.is_empty() {
                None
            } else {
                Some(json!(last_text))
            },
            tool_calls: Some(json!(tool_calls)),
            tool_call_id: None,
        });

        if env.cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        // One gate for every tool, checked against the call's own subject, so
        // `git status` can be free while `git push` is refused.
        let exec = |t: &provider::ToolUse| {
            let ctx = &ctx;
            let tools = &tools;
            let cancel = &env.cancel;
            let (name, input) = (t.name.clone(), t.input.clone());
            async move {
                let subject = crate::tools::call_subject(&name, &input);
                Ok::<String, anyhow::Error>(match ctx.gate(&name, &subject, cancel).await? {
                    Some(refusal) => refusal,
                    None => match tools.get(&name) {
                        Some(tool) => tool
                            .call(ctx, input, cancel)
                            .await
                            .unwrap_or_else(|e| e.to_string()),
                        None => format!("unknown tool {name}"),
                    },
                })
            }
        };
        // Run the turn's calls concurrently only when every one is read-only
        // and needs no approval: two approval prompts at once would leave one
        // unanswerable, and a write must never race a read of the same file.
        let parallel = turn.tools.len() > 1
            && turn.tools.iter().all(|t| {
                PARALLEL_SAFE.contains(&t.name.as_str())
                    && ctx.perms.decide(&t.name, &crate::tools::call_subject(&t.name, &t.input))
                        == ide_core::Permission::Allow
            });
        let outputs: Vec<String> = if parallel {
            futures_util::future::join_all(turn.tools.iter().map(exec))
                .await
                .into_iter()
                .collect::<Result<_>>()?
        } else {
            let mut out = Vec::with_capacity(turn.tools.len());
            for t in &turn.tools {
                if env.cancel.is_cancelled() {
                    anyhow::bail!("cancelled");
                }
                out.push(exec(t).await?);
            }
            out
        };
        let mut stuck = false;
        for (t, mut output) in turn.tools.into_iter().zip(outputs) {
            match loop_guard.see(&t.name, &t.input, &output) {
                LoopVerdict::Fine => {}
                LoopVerdict::Warn(n) => output.push_str(&format!(
                    "\n\n[loop guard] This is the {n}th identical `{}` call in a row with the \
                     same result. Repeating it will not change anything — try a different \
                     approach, or reply with what is blocking you.",
                    t.name
                )),
                LoopVerdict::Stop => stuck = true,
            }
            let _ = env.events.send(AgentEvent::ToolResult {
                name: t.name.clone(),
                output: output.clone(),
            });
            messages.push(Message {
                role: "tool".into(),
                content: Some(Value::String(output)),
                tool_calls: None,
                tool_call_id: Some(t.id),
            });
        }
        if stuck {
            let note = format!(
                "{}Stopped: I kept repeating the same step with the same result, so I ended the \
                 run rather than burn more turns on it. Tell me what to try differently.",
                if last_text.trim().is_empty() { String::new() } else { format!("{last_text}\n\n") }
            );
            return Ok((note, messages));
        }
    }
    // The budget ran out mid-work, not mid-answer — bailing here used to
    // surface to the user as a bare "tool loop limit reached", which explains
    // nothing. Hand back a real sentence instead, same as the pipeline's other
    // early-stop paths (wrote_nothing, a stalled review round).
    let note = if last_text.trim().is_empty() {
        format!(
            "Stopped after {max_turns} tool-call turns without a final summary — the task was \
             larger than this turn's budget. The work so far may be partial; ask me to continue."
        )
    } else {
        last_text
    };
    Ok((note, messages))
}

#[derive(Default)]
struct LoopGuard {
    last: Option<u64>,
    run: u32,
}

#[derive(Debug, PartialEq)]
enum LoopVerdict {
    Fine,
    Warn(u32),
    Stop,
}

impl LoopGuard {
    const WARN_AT: u32 = 3;
    const STOP_AT: u32 = 5;

    fn see(&mut self, name: &str, input: &Value, output: &str) -> LoopVerdict {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (name, input.to_string(), output).hash(&mut h);
        let sig = h.finish();
        if self.last == Some(sig) {
            self.run += 1;
        } else {
            self.last = Some(sig);
            self.run = 1;
        }
        match self.run {
            n if n >= Self::STOP_AT => LoopVerdict::Stop,
            n if n >= Self::WARN_AT => LoopVerdict::Warn(n),
            _ => LoopVerdict::Fine,
        }
    }
}

#[cfg(test)]
mod loop_guard_tests {
    use super::*;

    #[test]
    fn repeats_warn_then_stop_and_any_change_resets() {
        let mut g = LoopGuard::default();
        let i = json!({"program": "npm", "args": ["test"]});
        assert_eq!(g.see("run_command", &i, "fail"), LoopVerdict::Fine);
        assert_eq!(g.see("run_command", &i, "fail"), LoopVerdict::Fine);
        assert_eq!(g.see("run_command", &i, "fail"), LoopVerdict::Warn(3));
        // a different result is progress, not a loop
        assert_eq!(g.see("run_command", &i, "pass"), LoopVerdict::Fine);
        for _ in 0..4 {
            g.see("read_file", &json!({"path": "a"}), "x");
        }
        assert_eq!(g.see("read_file", &json!({"path": "a"}), "x"), LoopVerdict::Stop);
    }
}

/// Context metering: prefer real provider-reported input tokens when we have
/// them, fall back to the chars/4 estimate, and never exceed the window.
fn report_context(
    events: &broadcast::Sender<AgentEvent>,
    messages: &[Message],
    limit: u64,
    measured: Option<u64>,
) {
    let estimated = compact::estimate_tokens(messages);
    let used = measured.unwrap_or(0).max(estimated).min(limit);
    let _ = events.send(AgentEvent::Context { used, limit });
}

/// Codex's project doc: AGENTS.md at the workspace root is the repo's own
/// standing instructions, read into every conversation. CLAUDE.md is the
/// fallback so a repo set up for another agent still gets its rules.
/// Capped like Codex's `project_doc_max_bytes`.
pub fn project_doc(ws: &WorkspaceRoot) -> String {
    const MAX: usize = 32 * 1024;
    for name in ["AGENTS.md", "CLAUDE.md"] {
        let Ok(mut text) = std::fs::read_to_string(ws.root().join(name)) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        if text.len() > MAX {
            let mut cut = MAX;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            text.push_str("\n[… truncated]");
        }
        return format!(
            "\n\n# Project instructions ({name})\nThe repository's own rules. Follow them; they \
             override the general guidance above where they disagree.\n\n{text}"
        );
    }
    String::new()
}

fn system_prompt(role: AgentRole, ws: &WorkspaceRoot) -> String {
    // The orientation block matters: without it the agent guessed the stack from the
    // tool names and described files that were never in the open folder.
    let common = format!(
        "You are a coding agent working in the user's local folder; that folder is the only \
workspace and paths are relative to it. Keep going until the task is fully done before you \
reply — do not stop at a plan or hand back work you could do yourself.\n\
\n\
Workflow:\n\
- Questions about the code: investigate (search_files, then read_file with offset/limit around \
the hit) and answer, citing files. Do not edit anything for a question.\n\
- Changes: for anything with more than two steps, write the plan with todo_write first and keep \
it updated as you go. Edit with edit_file (exact old_text, unique in the file) or write_file for \
new files. Put file contents straight into the edit call — never draft them in prose first.\n\
- Bugs: localize before editing — narrow from file to function to line with search_files and \
ranged reads. When it is cheap, reproduce the bug first (a command or a small test), fix it, then \
run the reproduction again to show it is gone.\n\
- Verify code changes with check_code, and run_tests when the project has tests. A task that only \
runs or inspects something is done when it has run.\n\
- Parallel work: independent read-only lookups in one turn run concurrently. For big jobs, \
spawn_agent several subagents (explore to map code, scout for docs, general for a separate \
chunk of work), keep working, then wait_agent. Never give two agents the same file.\n\
\n\
Commands: run_command foreground calls suit installs, builds, tests and one-shot checks; pass \
background:true for dev servers and watchers so they keep running, with output in the terminal \
panel. Do not start the same background job twice. A dev server prints the port it actually bound \
— read it from the output; never assume the default (Next falls back to 3001 when 3000 is busy). \
Answer questions about this machine (date, versions, git state) by running the command.\n\
\n\
Reading is the expensive part: find the place first, read ranges, read each file once — its \
contents stay in this conversation. Use web_search/web_fetch for docs; prefer official sources and \
never invent a URL. If a skill covers the work, load it first and follow it; when a `ponytail` \
skill is listed, load it before writing code.\n\
\n\
Final reply: short and in the user's terms — what changed and where, or the answer. No tool \
narration, no restating the task, no pasted file contents. If a check failed or part of the ask \
is undone, say so plainly.\n\n{}{}",
        crate::project::summary(ws),
        project_doc(ws),
    );
    match role {
        AgentRole::Planner => format!(
            "{common}\n\nYou are in plan mode: you cannot edit files or run commands. Investigate, \
then give a short numbered plan naming the exact files and changes. For a plain question, just \
answer it. Say what you would do, not what you are unable to do — the user switches to Auto to \
have it carried out."
        ),
        AgentRole::Coder => format!(
            "{common}\n\nYou are the build agent with full tools. Do the work end to end."
        ),
        AgentRole::Reviewer => format!(
            "{common}\n\nYou are reviewing changes another agent made. Read only the changed \
files. Start with APPROVED or REVISE; for REVISE list exact fixes."
        ),
        AgentRole::Single => format!(
            "{common}\n\nShell commands need the user's approval first; if they decline one, do \
not retry it — adapt. If the user is just chatting, reply like a person and ask what to work on."
        ),
    }
}
