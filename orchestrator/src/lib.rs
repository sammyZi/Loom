use agent::{run_agent, ToolRegistry};
use anyhow::Result;
use ide_core::{AgentEvent, AgentRole, WorkspaceRoot};
use sandbox::Sandbox;
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// How much of the pipeline a run uses. Picked by the user in the composer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// planner -> coder -> reviewer, with shell access.
    Auto,
    /// planner only: produce a plan, change nothing.
    Plan,
    /// one agent, one pass, full tools. No planner, no reviewer.
    Manual,
    /// like Auto, but the agent has no shell; it lists commands for the user to run.
    Approve,
}

impl Mode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "plan" => Self::Plan,
            "manual" => Self::Manual,
            "approve" | "cmd" | "cmd accept" => Self::Approve,
            _ => Self::Auto,
        }
    }
}

pub async fn run_task(
    prompt: String,
    model: String,
    mode: Mode,
    effort: String,
    ws: WorkspaceRoot,
    sandbox: Arc<dyn Sandbox>,
    events: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
) -> Result<String> {
    if mode == Mode::Plan {
        let _ = events.send(AgentEvent::Status {
            message: "planner".into(),
        });
        return spawn_role(
            AgentRole::Planner,
            format!(
                "User task:\n{prompt}\n\nProduce a numbered implementation plan only. Do not edit \
                 any files. If this is not a coding task, answer it directly in a sentence or two."
            ),
            model,
            effort,
            ws,
            sandbox,
            events,
            cancel,
            ToolRegistry::read_only(),
            true,
        )
        .await;
    }

    if mode == Mode::Manual {
        let _ = events.send(AgentEvent::Status {
            message: "agent".into(),
        });
        return spawn_role(
            AgentRole::Single,
            prompt,
            model,
            effort,
            ws,
            sandbox,
            events,
            cancel,
            ToolRegistry::full(),
            true,
        )
        .await;
    }

    if is_greeting(&prompt) {
        let _ = events.send(AgentEvent::Status {
            message: "agent".into(),
        });
        return spawn_role(
            AgentRole::Single,
            prompt,
            model,
            effort,
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
        format!(
            "User task:\n{prompt}\n\nDecide which of three things this is.\n\
             1. Small talk or a general question with nothing to do with this repo: reply \
             `NO_CODE:` and your answer. No tools.\n\
             2. A question ABOUT this repo (explain the project, how does X work, where is Y): \
             investigate first. Call list_files, then read_file on the files that actually answer \
             it, for example the entry point, the main screens or routes, package.json scripts, \
             and any config that changes behaviour. Then reply `NO_CODE:` and explain what the \
             code DOES, citing the specific files you read. A directory listing is not an \
             explanation; do not describe a file you have not opened.\n\
             3. A request to change code: reply with a numbered implementation plan naming the \
             files to change."
        ),
        model.clone(),
        effort.clone(),
        ws.clone(),
        sandbox.clone(),
        events.clone(),
        cancel.clone(),
        ToolRegistry::read_only(),
        false,
    )
    .await?;

    // The planner decides this, not a keyword list: a greeting or a question stops here
    // instead of dragging a coder through the repo.
    if let Some(answer) = plan.trim_start().strip_prefix("NO_CODE:") {
        let answer = answer.trim().to_string();
        let _ = events.send(AgentEvent::Done {
            summary: answer.clone(),
        });
        return Ok(answer);
    }

    // Approve mode withholds the shell, so the coder hands commands back to the user.
    let coder_tools = || {
        if mode == Mode::Approve {
            ToolRegistry::no_shell()
        } else {
            ToolRegistry::full()
        }
    };
    let coder_note = if mode == Mode::Approve {
        "\n\nYou cannot run shell commands. If any command needs running, list it at the end \
         under `Commands to run:` for the user to approve."
    } else {
        ""
    };

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
            format!("User task:\n{prompt}\n\nPlan:\n{plan}\n\nImplement now.{coder_note}"),
            model.clone(),
            effort.clone(),
            ws.clone(),
            sandbox.clone(),
            events.clone(),
            cancel.clone(),
            coder_tools(),
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
            effort.clone(),
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
    effort: String,
    ws: WorkspaceRoot,
    sandbox: Arc<dyn Sandbox>,
    events: broadcast::Sender<AgentEvent>,
    cancel: CancellationToken,
    tools: ToolRegistry,
    announce_done: bool,
) -> Result<String> {
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<String>>(1);
    let handle = tokio::spawn(async move {
        let r = run_agent(
            role, prompt, model, ws, sandbox, events, cancel, tools, announce_done, effort,
        )
        .await;
        let _ = tx.send(r).await;
    });
    let out = rx
        .recv()
        .await
        .ok_or_else(|| anyhow::anyhow!("agent task died"))?;
    let _ = handle.await;
    out
}
