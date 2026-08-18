use agent::{run_agent, ToolRegistry};
use anyhow::Result;
use ide_core::{AgentEvent, AgentRole, WorkspaceRoot};
use sandbox::Sandbox;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub async fn run_task(
    prompt: String,
    model: String,
    ws: WorkspaceRoot,
    sandbox: Arc<dyn Sandbox>,
    events: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
) -> Result<String> {
    if is_greeting(&prompt) {
        let _ = events.send(AgentEvent::Status {
            message: "agent".into(),
        });
        return spawn_role(
            AgentRole::Single,
            prompt,
            model,
            ws,
            sandbox,
            events,
            cancel,
            ToolRegistry::none(),
            true,
        )
        .await;
    }

    let _ = events.send(AgentEvent::Status {
        message: "planner".into(),
    });
    let plan = spawn_role(
        AgentRole::Planner,
        format!("User task:\n{prompt}\n\nIf this is not a coding task, reply in one sentence. Otherwise produce a numbered implementation plan."),
        model.clone(),
        ws.clone(),
        sandbox.clone(),
        events.clone(),
        cancel.clone(),
        ToolRegistry::read_only(),
        false,
    )
    .await?;

    let mut last = String::new();
    for round in 0..3 {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let _ = events.send(AgentEvent::Status {
            message: format!("coder {}", round + 1),
        });
        last = spawn_role(
            AgentRole::Coder,
            format!("User task:\n{prompt}\n\nPlan:\n{plan}\n\nImplement now."),
            model.clone(),
            ws.clone(),
            sandbox.clone(),
            events.clone(),
            cancel.clone(),
            ToolRegistry::full(),
            false,
        )
        .await?;

        let _ = events.send(AgentEvent::Status {
            message: "reviewer".into(),
        });
        let review = spawn_role(
            AgentRole::Reviewer,
            format!(
                "User task:\n{prompt}\n\nCoder summary:\n{last}\n\nReview. Start with APPROVED or REVISE."
            ),
            model.clone(),
            ws.clone(),
            sandbox.clone(),
            events.clone(),
            cancel.clone(),
            ToolRegistry::read_only(),
            false,
        )
        .await?;
        last = review.clone();
        if review.to_ascii_uppercase().contains("APPROVED") {
            let _ = events.send(AgentEvent::Done {
                summary: review,
            });
            return Ok(last);
        }
    }
    let _ = events.send(AgentEvent::Done {
        summary: last.clone(),
    });
    Ok(last)
}

fn is_greeting(prompt: &str) -> bool {
    let t = prompt
        .trim()
        .trim_end_matches(['!', '.', '?', ','])
        .to_ascii_lowercase();
    matches!(
        t.as_str(),
        "hi" | "hii" | "hello" | "hey" | "yo" | "sup" | "thanks" | "thank you" | "gm" | "good morning"
            | "good evening" | "good night" | "howdy"
    ) || t.split_whitespace().count() <= 3
        && ["hi", "hello", "hey"].iter().any(|w| t.split_whitespace().any(|p| p == *w))
}

async fn spawn_role(
    role: AgentRole,
    prompt: String,
    model: String,
    ws: WorkspaceRoot,
    sandbox: Arc<dyn Sandbox>,
    events: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    tools: ToolRegistry,
    announce_done: bool,
) -> Result<String> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<String>>(1);
    let handle = tokio::spawn(async move {
        let r = run_agent(role, prompt, model, ws, sandbox, events, cancel, tools, announce_done).await;
        let _ = tx.send(r).await;
    });
    let out = rx
        .recv()
        .await
        .ok_or_else(|| anyhow::anyhow!("agent task died"))?;
    let _ = handle.await;
    out
}
