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
}

#[allow(clippy::too_many_arguments)]
pub async fn run_agent(
    role: AgentRole,
    prompt: String,
    model: String,
    effort: String,
    tools: ToolRegistry,
    announce_done: bool,
    env: &RunEnv,
    seed: Vec<Message>,
    perm: Option<PermGate>,
) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let schemas = tools.schemas();
    let ctx = ToolCtx {
        ws: env.ws.clone(),
        sandbox: env.sandbox.clone(),
        events: env.events.clone(),
        shell_tx: env.shell_tx.clone(),
        shells: env.shells.clone(),
        perm,
    };
    let mut messages = seed;
    messages.push(Message::user_text(prompt));
    let system = system_prompt(role, &ctx.ws);
    let mut last_text = String::new();
    // The most accurate conversation size seen so far: the provider's own
    // input-token report beats any local character estimate. Shared because
    // the streaming callback owns its captures.
    let measured_input = Arc::new(std::sync::Mutex::new(None::<u64>));

    for _ in 0..24 {
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
                StreamKind::ThinkDelta(_) => {}
                StreamKind::TextDelta(t) => {
                    let _ = ev_tx.send(AgentEvent::Token { text: t });
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
            let output = match tools.get(&t.name) {
                Some(tool) => tool
                    .call(&ctx, t.input, &env.cancel)
                    .await
                    .unwrap_or_else(|e| e.to_string()),
                None => format!("unknown tool {}", t.name),
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
    let _ = env.events.send(AgentEvent::Done { summary: last_text });
    anyhow::bail!("tool loop limit reached")
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
            "{common}\nYou are the coder. Implement the given plan with edit_file. \
Then check_code and run_tests. Fix failures. Reply with a short summary of files changed."
        ),
        AgentRole::Reviewer => format!(
            "{common}\nYou are the reviewer. Inspect changed files with read_file. \
Run check_code and run_tests. If tests/compiler fail, say REVISE and list exact fixes. \
If they pass, say APPROVED and a one-line summary."
        ),
        AgentRole::Single => format!(
            "{common}\nYou are chatting in the IDE and can use tools directly. Shell commands \
             need the user's approval first; if they decline one, do not retry — adapt. \
             Reply like a person. If they say hi or small-talk, greet them and ask what to work on."
        ),
    }
}
