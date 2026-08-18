use crate::anthropic::{self, Message, StreamKind};
use crate::compact;
use crate::tools::{ToolCtx, ToolRegistry};
use anyhow::Result;
use ide_core::{AgentEvent, AgentRole, WorkspaceRoot};
use sandbox::Sandbox;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub async fn run_agent(
    role: AgentRole,
    prompt: String,
    ws: WorkspaceRoot,
    sandbox: Arc<dyn Sandbox>,
    events: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    tools: ToolRegistry,
    announce_done: bool,
) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()?;
    let schemas = tools.schemas();
    let ctx = ToolCtx {
        ws,
        sandbox,
        events: events.clone(),
    };
    let mut messages = vec![Message {
        role: "user".into(),
        content: json!([{ "type": "text", "text": prompt }]),
    }];
    let system = system_prompt(role);
    let mut last_text = String::new();

    for _ in 0..24 {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        compact::compact(&mut messages);
        let ev = events.clone();
        let turn = anthropic::stream(&client, &system, &schemas, &messages, |k| match k {
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

        let mut assistant_content = Vec::new();
        if !turn.text.is_empty() {
            assistant_content.push(json!({"type":"text","text": turn.text}));
        }
        for t in &turn.tools {
            assistant_content.push(json!({
                "type": "tool_use",
                "id": t.id,
                "name": t.name,
                "input": t.input,
            }));
        }
        messages.push(Message {
            role: "assistant".into(),
            content: json!(assistant_content),
        });

        let mut results = Vec::new();
        for t in turn.tools {
            if cancel.is_cancelled() {
                anyhow::bail!("cancelled");
            }
            let output = match tools.get(&t.name) {
                Some(tool) => tool.call(&ctx, t.input, &cancel).await.unwrap_or_else(|e| e.to_string()),
                None => format!("unknown tool {}", t.name),
            };
            let _ = events.send(AgentEvent::ToolResult {
                name: t.name.clone(),
                output: output.clone(),
            });
            results.push(json!({
                "type": "tool_result",
                "tool_use_id": t.id,
                "content": output,
            }));
        }
        messages.push(Message {
            role: "user".into(),
            content: json!(results),
        });
    }
    let _ = last_text;
    anyhow::bail!("tool loop limit reached")
}

fn system_prompt(role: AgentRole) -> String {
    let common = "You are a coding agent. The user opened one local folder; that is the only workspace. \
Use tools. Paths are relative to the workspace root. Never ask to run unsandboxed commands. \
Edits must be minimal. When changing code, run check_code and run_tests before claiming done.";
    match role {
        AgentRole::Planner => format!(
            "{common}\nYou are the planner. Read the repo as needed, then output a short numbered plan. \
Do not edit files. Do not run tests unless needed to understand the repo."
        ),
        AgentRole::Coder => format!(
            "{common}\nYou are the coder. Implement the given plan with edit_file. \
Then check_code and run_tests. Fix failures. Reply with a short summary of files changed."
        ),
        AgentRole::Reviewer => format!(
            "{common}\nYou are the reviewer. Inspect the diff via read_file and git-unaware file reads. \
Run check_code and run_tests. If tests/compiler fail, say REVISE and list exact fixes. \
If they pass, say APPROVED and a one-line summary."
        ),
        AgentRole::Single => common.into(),
    }
}
