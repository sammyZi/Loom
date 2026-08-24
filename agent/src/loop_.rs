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
    /// Files already handed to the model this task. Shared by every role so the
    /// coder does not re-read what the planner just read.
    pub reads: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
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

#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    role: AgentRole,
    prompt: String,
    model: String,
    effort: String,
    tools: ToolRegistry,
    announce_done: bool,
    // Tool-call turns before this role gives up and hands back whatever it
    // has. Was a hardcoded 24 for every role; callers now size it to the job
    // (a coder doing real work needs far more than a read-only reviewer).
    max_turns: u32,
    env: &RunEnv,
    seed: Vec<Message>,
    perm: Option<PermGate>,
    perms: ide_core::PermissionSet,
    spawn_subagent: Option<crate::tools::SubagentRunner>,
) -> Result<String> {
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
        reads: env.reads.clone(),
        writes: env.writes.clone(),
        read_budget: env.read_budget.clone(),
        before: env.before.clone(),
    };
    let mut messages = seed;
    messages.push(Message::user_text(prompt));
    let system = format!("{}{skill_text}", system_prompt(role, &ctx.ws));
    let mut last_text = String::new();
    // The most accurate conversation size seen so far: the provider's own
    // input-token report beats any local character estimate. Shared because
    // the streaming callback owns its captures.
    let measured_input = Arc::new(std::sync::Mutex::new(None::<u64>));

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
                    let _ = ev_tx.send(if announce_done {
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
            if announce_done {
                let _ = env.events.send(AgentEvent::Done { summary: last_text.clone() });
            }
            return Ok(last_text);
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

        for t in turn.tools {
            if env.cancel.is_cancelled() {
                anyhow::bail!("cancelled");
            }
            // One gate for every tool, checked against the call's own subject,
            // so `git status` can be free while `git push` is refused. Gating
            // inside individual tools only ever covered run_command.
            let subject = crate::tools::call_subject(&t.name, &t.input);
            let output = match ctx.gate(&t.name, &subject, &env.cancel).await? {
                Some(refusal) => refusal,
                None => match tools.get(&t.name) {
                    Some(tool) => tool
                        .call(&ctx, t.input, &env.cancel)
                        .await
                        .unwrap_or_else(|e| e.to_string()),
                    None => format!("unknown tool {}", t.name),
                },
            };
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
    if announce_done {
        let _ = env.events.send(AgentEvent::Done { summary: note.clone() });
    }
    Ok(note)
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

fn system_prompt(role: AgentRole, ws: &WorkspaceRoot) -> String {
    // The orientation block matters: without it the agent guessed the stack from the
    // tool names and described files that were never in the open folder.
    let common = format!(
        "You are a coding agent. The user opened one local folder; that is the only workspace. \
Paths are relative to the workspace root. You CAN run commands with run_command: foreground \
calls wait up to 120s and suit installs, builds, tests and one-shot checks; pass background:true \
for dev servers or watchers so they keep running after you reply, with their output streaming to \
the terminal panel. Do not start the same background job twice. A dev server prints the URL and \
port it actually bound to — read it from that output and use it. Never assume the default port: \
it moves when the port is busy (Next falls back to 3001), so checking the wrong one reports a \
server that is not yours. \
Reading costs more than anything else you do, so read like someone paying for it. \
Find the place first with search_files, then read_file with offset and limit around it. \
Read a whole file only when you are about to rewrite it whole. Never open every file in a \
folder to 'get oriented' — decide from the file list and one search which two or three \
actually matter. Read each file once: its contents stay in this conversation, and asking \
again returns a one-line note, not the file. \
Never restate a file's contents, a diff, or a code block in your reasoning. The code is \
already in the conversation; repeating it there is pure waste and it is not shown to the user. \
Think in short notes to yourself, not in code. \
If a skill covers the work you are about to do, load it first and follow it — when a \
`ponytail` skill is listed, load it before writing any code and apply it. \
Answer questions about this machine by running the command, never by refusing: the date and \
time, tool versions, disk contents and git state are all one run_command away. \
Do the task the user actually asked for and stop. 'Start the app' means start it — not install, \
not build, not lint, not test. Run extra commands only when the task needs them or something \
failed. \
You have internet access through web_search (find pages) and web_fetch (read one page); prefer \
official docs over blog guesses and never fabricate a URL. Use search_files to locate code by \
content instead of reading files one by one. Edits must be minimal; edit_file replaces exact \
text and refuses to overwrite an existing file without an exact match. Never dump \
chain-of-thought, tool traces, or README paste into the user reply.\n\n{}",
        crate::project::summary(ws)
    );
    match role {
        AgentRole::Planner => format!(
            "{common}\nYou are the planner. If the user is greeting or chatting, reply in one short sentence and stop — no tools, no plan. \
Otherwise read the repo as needed, then output a short numbered plan. Do not edit files. \
Never tell the user what you cannot do: a coder with a terminal runs right after you and \
carries out the plan, so 'I can't run commands' is wrong and confusing. Plan the command; \
do not hand it back for the user to type."
        ),
        AgentRole::Coder => format!(
            "{common}\nYou are the coder. Write the change — a turn that reads and plans but \
edits nothing has failed, however good the explanation. Carry out the plan. Only *code changes* get \
check_code and run_tests afterwards — a task that runs or inspects something is finished when \
it has run, and following it with a build or test suite wastes the user's time. \
Reply with the result: for a change, the files touched; for a command, what it did and the \
URL or output that matters."
        ),
        AgentRole::Reviewer => format!(
            "{common}\nYou are the reviewer. Inspect changed files with read_file. \
If no files were changed — the task only ran or inspected something — reply APPROVED at once \
and run nothing. Otherwise run check_code and run_tests: if they fail, say REVISE and list \
exact fixes; if they pass, say APPROVED and a one-line summary."
        ),
        AgentRole::Single => format!(
            "{common}\nYou are chatting in the IDE and can use tools directly. Shell commands \
             need the user's approval first; if they decline one, do not retry — adapt. \
             Reply like a person. If they say hi or small-talk, greet them and ask what to work on."
        ),
    }
}
