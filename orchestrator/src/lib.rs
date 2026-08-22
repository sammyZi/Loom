use agent::{run_agent, Message, PermGate, RunEnv, ToolRegistry};
use anyhow::Result;
use ide_core::{AgentEvent, AgentRole};

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
    env: RunEnv,
    // Earlier turns of this chat session. Only the first agent of the task
    // sees it — later roles work within the task's own context.
    history: Vec<Message>,
    // Present in manual mode so shell commands ask before running.
    perm: Option<PermGate>,
) -> Result<String> {
    if mode == Mode::Plan {
        let _ = env.events.send(AgentEvent::Status { message: "planner".into() });
        return spawn_role(
            AgentRole::Planner,
            format!(
                "User task:\n{prompt}\n\nProduce a numbered implementation plan only. Do not edit \
                 any files. If this is not a coding task, answer it directly in a sentence or two."
            ),
            model,
            effort,
            ToolRegistry::read_only(),
            true,
            &env,
            history,
            None,
        )
        .await;
    }

    if mode == Mode::Manual {
        let _ = env.events.send(AgentEvent::Status { message: "agent".into() });
        return spawn_role(
            AgentRole::Single,
            prompt,
            model,
            effort,
            ToolRegistry::full(),
            true,
            &env,
            history,
            perm,
        )
        .await;
    }

    if is_greeting(&prompt) {
        let _ = env.events.send(AgentEvent::Status { message: "agent".into() });
        return spawn_role(
            AgentRole::Single,
            prompt,
            model,
            effort,
            ToolRegistry::none(),
            true,
            &env,
            history,
            None,
        )
        .await;
    }

    let _ = env.events.send(AgentEvent::Status { message: "planner".into() });
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
        ToolRegistry::read_only(),
        false,
        &env,
        history,
        perm.clone(),
    )
    .await?;

    // The planner decides this, not a keyword list: a greeting or a question stops here
    // instead of dragging a coder through the repo.
    if let Some(answer) = plan.trim_start().strip_prefix("NO_CODE:") {
        let answer = answer.trim().to_string();
        let _ = env.events.send(AgentEvent::Done { summary: answer.clone() });
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
        "\n\nApprove mode is on, so the shell is withheld from you on purpose — this is a mode \
         setting, not a missing capability. List any commands that need running at the end under \
         `Commands to run:`, and say in one line that switching the composer's mode to Auto or \
         Manual lets you run them yourself."
    } else {
        ""
    };

    let mut last = String::new();
    for round in 0..3 {
        if env.cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let _ = env
            .events
            .send(AgentEvent::Status { message: format!("coder {}", round + 1) });
        last = spawn_role(
            AgentRole::Coder,
            format!("User task:\n{prompt}\n\nPlan:\n{plan}\n\nImplement now.{coder_note}"),
            model.clone(),
            effort.clone(),
            coder_tools(),
            false,
            &env,
            Vec::new(),
            perm.clone(),
        )
        .await?;

        let _ = env.events.send(AgentEvent::Status { message: "reviewer".into() });
        let review = spawn_role(
            AgentRole::Reviewer,
            format!(
                "User task:\n{prompt}\n\nCoder summary:\n{last}\n\nReview. Start with APPROVED or REVISE."
            ),
            model.clone(),
            effort.clone(),
            ToolRegistry::read_only(),
            false,
            &env,
            Vec::new(),
            None,
        )
        .await?;
        last = review.clone();
        if review.to_ascii_uppercase().contains("APPROVED") {
            let _ = env.events.send(AgentEvent::Done { summary: review });
            return Ok(last);
        }
    }
    let _ = env.events.send(AgentEvent::Done { summary: last.clone() });
    Ok(last)
}

/// Strict small-talk detection. The old "≤3 words starting with hi/hello/hey"
/// rule swallowed real tasks like "hello world program?" and sent them to a
/// no-tool chat agent. Now only exact greetings match.
fn is_greeting(prompt: &str) -> bool {
    let t = prompt
        .trim()
        .trim_end_matches(['!', '.', ',', ':'])
        .to_ascii_lowercase();
    if t.is_empty() || t.len() > 32 || t.contains('?') || t.contains('=') || t.contains('(') {
        return false;
    }
    matches!(
        t.as_str(),
        "hi" | "hii" | "hiii"
            | "hey" | "heyy"
            | "hello" | "helloo"
            | "yo" | "sup" | "howdy"
            | "thanks" | "thank you" | "thx" | "ty"
            | "gm" | "gn" | "good morning" | "good evening" | "good night" | "good afternoon"
            | "hi there" | "hey there" | "hello there"
            | "hi loom" | "hey loom" | "hello loom"
            | "hi agent" | "hey agent"
    )
}

#[cfg(test)]
mod greeting_tests {
    use super::is_greeting;

    #[test]
    fn plain_greetings_are_small_talk() {
        for g in ["hi", "Hello!", "hey there", "good morning", "thanks"] {
            assert!(is_greeting(g), "{g}");
        }
    }

    #[test]
    fn real_tasks_are_not_greetings_even_if_they_start_with_hello() {
        for t in [
            "hello world program?",
            "write a hello world app",
            "hi, add dark mode to the settings page",
            "explain how routing works",
            "fix the bug",
        ] {
            assert!(!is_greeting(t), "{t}");
        }
    }

    #[test]
    fn long_or_structured_input_is_never_a_greeting() {
        assert!(!is_greeting("fn main() { println!(\"hi\"); }"));
        assert!(!is_greeting(""));
    }
}

#[allow(clippy::too_many_arguments)]
async fn spawn_role(
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
    // A panicking role must degrade into an error, not kill the whole
    // pipeline task silently.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<String>>(1);
    let env = env.clone();
    let handle = tokio::spawn(async move {
        let r = run_agent(role, prompt, model, effort, tools, announce_done, &env, seed, perm)
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
