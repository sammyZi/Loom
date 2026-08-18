use agent::{run_agent, ToolRegistry};
use anyhow::Result;
use core::{AgentEvent, AgentRole, WorkspaceRoot};
use sandbox::Sandbox;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub async fn run_task(
    prompt: String,
    ws: WorkspaceRoot,
    sandbox: Arc<dyn Sandbox>,
    events: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
) -> Result<String> {
    let _ = events.send(AgentEvent::Status {
        message: "planner".into(),
    });
    let plan = spawn_role(
        AgentRole::Planner,
        format!("User task:\n{prompt}\n\nProduce a numbered implementation plan."),
        ws.clone(),
        sandbox.clone(),
        events.clone(),
        cancel.clone(),
        ToolRegistry::read_only(),
    )
    .await?;

    let mut last = String::new();
    for round in 0..3 {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let _ = events.send(AgentEvent::Status {
            message: format!("coder round {}", round + 1),
        });
        last = spawn_role(
            AgentRole::Coder,
            format!("User task:\n{prompt}\n\nPlan:\n{plan}\n\nImplement now."),
            ws.clone(),
            sandbox.clone(),
            events.clone(),
            cancel.clone(),
            ToolRegistry::full(),
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
            ws.clone(),
            sandbox.clone(),
            events.clone(),
            cancel.clone(),
            ToolRegistry::read_only(),
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

async fn spawn_role(
    role: AgentRole,
    prompt: String,
    ws: WorkspaceRoot,
    sandbox: Arc<dyn Sandbox>,
    events: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    tools: ToolRegistry,
) -> Result<String> {
    // Each role is its own task + mailbox-equivalent: isolated stack, no shared mut.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<String>>(1);
    let handle = tokio::spawn(async move {
        let r = run_agent(role, prompt, ws, sandbox, events, cancel, tools, false).await;
        let _ = tx.send(r).await;
    });
    let out = rx
        .recv()
        .await
        .ok_or_else(|| anyhow::anyhow!("agent task died"))?;
    let _ = handle.await;
    out
}
