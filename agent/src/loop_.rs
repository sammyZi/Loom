use crate::compact;
use crate::deepseek::{self, Message, StreamKind};
use crate::tools::{ToolCtx, ToolRegistry};
use anyhow::Result;
use ide_core::{AgentEvent, AgentRole, WorkspaceRoot};
use sandbox::Sandbox;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub async fn run_agent(
    role: AgentRole,
    prompt: String,
    model: String,
    ws: WorkspaceRoot,
    sandbox: Arc<dyn Sandbox>,
    events: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    tools: ToolRegistry,
    announce_done: bool,
    effort: String,
) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;
    let schemas = tools.schemas();
    let ctx = ToolCtx {
        ws,
        sandbox,
        events: events.clone(),
    };
    let mut messages = vec![Message::user_text(prompt)];
    let system = system_prompt(role, &ctx.ws);
    let mut last_text = String::new();

    for _ in 0..24 {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        compact::compact(&mut messages);
        let ev = events.clone();
        let turn = deepseek::stream(&client, &model, &system, &schemas, &messages, &effort, |k| match k {
            StreamKind::Usage(tokens) => {
                let _ = ev.send(AgentEvent::Usage { tokens });
            }
            StreamKind::ThinkDelta(_) => {}
            StreamKind::TextDelta(t) => {
                let _ = ev.send(AgentEvent::Token { text: t });
            }
            StreamKind::ToolUse { name, input } => {
                let _ = ev.send(AgentEvent::ToolCall { name, input });
            }
            StreamKind::Error(message) => {
                let _ = ev.send(AgentEvent::Error { message });
            }
        })
        .await?;

        last_text = turn.text.clone();
        if turn.tools.is_empty() {
            if announce_done {
                let _ = events.send(AgentEvent::Done {
                    summary: last_text.clone(),
                });
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
            if cancel.is_cancelled() {
                anyhow::bail!("cancelled");
            }
            let output = match tools.get(&t.name) {
                Some(tool) => tool
                    .call(&ctx, t.input, &cancel)
                    .await
                    .unwrap_or_else(|e| e.to_string()),
                None => format!("unknown tool {}", t.name),
            };
            let _ = events.send(AgentEvent::ToolResult {
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
    let _ = last_text;
    anyhow::bail!("tool loop limit reached")
}

fn system_prompt(role: AgentRole, ws: &WorkspaceRoot) -> String {
    // The orientation block matters: without it the agent guessed the stack from the
    // tool names and described files that were never in the open folder.
    let common = format!(
        "You are a coding agent. The user opened one local folder; that is the only workspace. \
Paths are relative to the workspace root. You CAN run commands with run_command: it is \
sandboxed and waits for the command to finish, so it suits installs, builds, tests and one-shot \
checks. A long-running process such as a dev server is killed when the call returns, so never \
claim to have started one; give the user the exact command to run in the app terminal panel. \
Edits must be minimal. Never dump chain-of-thought, tool traces, or README paste into the user reply.\n\n{}",
        crate::project::summary(ws)
    );
    match role {
        AgentRole::Planner => format!(
            "{common}\nYou are the planner. If the user is greeting or chatting, reply in one short sentence and stop — no tools, no plan. \
Otherwise read the repo as needed, then output a short numbered plan. Do not edit files."
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
            "{common}\nYou are chatting in the IDE. Reply like a person. \
If they say hi or small-talk, greet them and ask what to work on. Do not use tools. Do not write a plan."
        ),
    }
}
