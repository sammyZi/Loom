use agent::{run_agent, Message, PermGate, RunEnv, ToolRegistry};
use anyhow::Result;
use ide_core::{AgentEvent, AgentRole, Permission, PermissionSet};

/// Tool-call turns before a role's loop gives up and hands back whatever it
/// has. Was one hardcoded 24 for every role; sized here to the job instead —
/// the coder is the one doing the real work and ran out first on large asks.
const TURNS_INVESTIGATE: u32 = 20;
const TURNS_CODE: u32 = 40;
const TURNS_REVIEW: u32 = 15;
const TURNS_TRIVIAL: u32 = 6;

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
            TURNS_INVESTIGATE,
            &env,
            history,
            None,
            read_only_perms(),
            None,
        )
        .await;
    }

    if mode == Mode::Manual {
        let _ = env.events.send(AgentEvent::Status { message: "agent".into() });
        let answer = spawn_role(
            AgentRole::Single,
            prompt.clone(),
            model.clone(),
            effort.clone(),
            ToolRegistry::full(),
            true,
            TURNS_CODE,
            &env,
            history,
            perm,
            manual_perms(),
            Some(subagent_runner(&env, model, effort)),
        )
        .await?;
        // This check used to live only in the Auto pipeline, so a single-agent
        // run could read the project, start the dev server, open the browser
        // and report success having edited nothing at all.
        return Ok(warn_if_nothing_changed(&env, &prompt, answer));
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
            TURNS_TRIVIAL,
            &env,
            history,
            None,
            read_only_perms(),
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
        TURNS_INVESTIGATE,
        &env,
        history.clone(),
        perm.clone(),
        read_only_perms(),
        Some(subagent_runner(&env, model.clone(), effort.clone())),
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
    // Snapshot of what had been written before each round, so a round that
    // adds nothing can end the loop instead of burning two more.
    let mut before_round = changed_files(&env);
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
            TURNS_CODE,
            &env,
            // The coder sees the conversation too. It used to get an empty
            // seed, so a follow-up like "now make it blue" arrived with no
            // idea what "it" referred to — only the plan, which the planner
            // had written without knowing it needed to spell that out.
            history.clone(),
            perm.clone(),
            coder_perms(mode),
            Some(subagent_runner(&env, model.clone(), effort.clone())),
        )
        .await?;

        // Nothing was written, so there is nothing to review. Running the
        // reviewer here just paid for it to re-read the whole project and
        // conclude the same thing.
        if wrote_nothing(&env) {
            let note = format!(
                "{last}\n\n**No files were changed.** The task asked for a change but nothing \
                 was written — treat this as unfinished rather than done."
            );
            let _ = env.events.send(AgentEvent::Done { summary: note.clone() });
            return Ok(note);
        }

        let _ = env.events.send(AgentEvent::Status { message: "reviewer".into() });
        // Hand over the diff, not the project. The reviewer used to be given a
        // summary and left to re-read every file to find the change — the
        // single biggest source of duplicated reads in a run.
        let changed = changed_files(&env);
        let review = spawn_role(
            AgentRole::Reviewer,
            format!(
                "User task:\n{prompt}\n\nCoder summary:\n{last}\n\nFiles this run changed:\n\
                 {changed}\n\nReview only those files and only what changed in them. Do not \
                 read anything else. Start with APPROVED or REVISE."
            ),
            model.clone(),
            effort.clone(),
            ToolRegistry::read_only(),
            false,
            TURNS_REVIEW,
            &env,
            Vec::new(),
            None,
            read_only_perms(),
            None,
        )
        .await?;
        last = review.clone();
        if review.to_ascii_uppercase().contains("APPROVED") {
            let _ = env.events.send(AgentEvent::Done { summary: review });
            return Ok(last);
        }
        // A REVISE round that changed nothing will not change anything next
        // time either — the loop was re-running the whole planner/coder pass
        // to reach the same place. Stop and say so rather than pay for it
        // three times.
        let touched = changed_files(&env);
        if touched == before_round {
            let note = format!(
                "{review}\n\n**Stopped after round {}:** the last pass changed no files, so \
                 another round would repeat it. Say what is blocking and try a narrower ask.",
                round + 1
            );
            let _ = env.events.send(AgentEvent::Done { summary: note.clone() });
            return Ok(note);
        }
        before_round = touched;
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
mod loop_tests {
    use super::*;
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    fn writes(paths: &[&str]) -> Arc<Mutex<HashSet<String>>> {
        Arc::new(Mutex::new(paths.iter().map(|s| s.to_string()).collect()))
    }

    /// The reviewer is bounded to what changed. Feeding it a summary and
    /// letting it re-read the project was the biggest source of duplicated
    /// reads in a run, so this list has to be exact and stable.
    #[test]
    fn changed_files_lists_writes_in_a_stable_order() {
        let w = writes(&["src/b.rs", "src/a.rs"]);
        let listed = super::changed_files_from(&w);
        assert_eq!(listed, "- src/a.rs\n- src/b.rs", "sorted, one per line");
    }

    /// The reported failure: "fix the buttons and the navbar" read the whole
    /// project, ran the dev server, opened a browser and reported success
    /// having written nothing. Any prompt asking for a change must be caught.
    #[test]
    fn change_requests_are_recognised() {
        for p in [
            "fix the issue in the app some things are white and not visible",
            "add a dark mode toggle",
            "make it non ai looking",
            "keep the navbar fixed when scrolling",
            "Refactor the Hero component",
            "creating a new page",
            "updates the styles",
        ] {
            assert!(super::asks_for_a_change(p), "should be a change request: {p}");
        }
    }

    /// Conservative on purpose: warning on a genuine question would nag.
    #[test]
    fn questions_and_commands_are_not_change_requests() {
        for p in [
            "what does this project do?",
            "explain the routing",
            "run the tests",
            "start the app",
            "hi",
        ] {
            assert!(!super::asks_for_a_change(p), "should not be a change request: {p}");
        }
    }

    /// Word-boundary matching: "prefix" contains "fix", and "buildings"
    /// contains "build", but neither asks for a change.
    #[test]
    fn substrings_of_longer_words_do_not_count() {
        assert!(!super::asks_for_a_change("what is the prefix used here?"));
        assert!(!super::asks_for_a_change("describe the buildings dataset"));
    }

    #[test]
    fn changed_files_says_none_rather_than_going_blank() {
        assert_eq!(super::changed_files_from(&writes(&[])), "(none)");
    }

    /// The loop-exit rule: a round that writes nothing has made no progress,
    /// and two more rounds would reach the same place at triple the cost.
    #[test]
    fn a_round_that_writes_nothing_is_detected_as_no_progress() {
        let w = writes(&["a.rs"]);
        let before = super::changed_files_from(&w);
        // second round adds nothing
        let after = super::changed_files_from(&w);
        assert_eq!(before, after, "no new writes must compare equal");

        w.lock().unwrap().insert("b.rs".into());
        assert_ne!(before, super::changed_files_from(&w), "a new write must differ");
    }
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

/// Append a warning when a run that was clearly asked to change something
/// finished without writing a file. Running commands and opening a browser is
/// not the same as doing the work, and reporting it as done hides the failure.
/// Emits the amended text as the Done summary so the UI shows the warning too.
fn warn_if_nothing_changed(env: &RunEnv, prompt: &str, answer: String) -> String {
    if !wrote_nothing(env) || !asks_for_a_change(prompt) {
        return answer;
    }
    let note = format!(
        "{answer}\n\n**No files were changed.** This task asked for a change but nothing was \
         written to disk — treat it as unfinished, not done."
    );
    let _ = env.events.send(AgentEvent::Done { summary: note.clone() });
    note
}

/// Rough intent check. Deliberately conservative: a false negative just skips
/// the warning, while a false positive would nag on a genuine question.
fn asks_for_a_change(prompt: &str) -> bool {
    const VERBS: &[&str] = &[
        "fix", "add", "change", "update", "make", "create", "remove", "delete", "rename",
        "refactor", "implement", "build", "write", "replace", "improve", "convert", "migrate",
        "style", "redesign",
    ];
    let p = prompt.to_ascii_lowercase();
    let words: Vec<&str> = p.split(|c: char| !c.is_ascii_alphabetic()).collect();
    VERBS.iter().any(|v| {
        // Inflections, including the two that bit: past tense ("keep the navbar
        // fixed") and the dropped-e gerund ("creating", not "createing").
        let stem = v.strip_suffix('e').unwrap_or(v);
        let forms = [
            v.to_string(),
            format!("{v}s"),
            format!("{v}ed"),
            format!("{v}d"),
            format!("{stem}ing"),
        ];
        words.iter().any(|w| forms.iter().any(|f| f == w))
    })
}

/// True when the run edited nothing. `reads` is populated by read_file and
/// `writes` by edit_file/write_file, so an empty write set after a coding round
/// means the pipeline talked about the work without doing it.
fn wrote_nothing(env: &RunEnv) -> bool {
    env.writes.lock().map(|w| w.is_empty()).unwrap_or(false)
}

/// The paths this run touched, as a list for the reviewer to bound itself to.
fn changed_files(env: &RunEnv) -> String {
    changed_files_from(&env.writes)
}

/// Split from `changed_files` so the ordering and the no-progress comparison
/// can be tested without standing up a whole RunEnv.
fn changed_files_from(
    writes: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
) -> String {
    let Ok(w) = writes.lock() else {
        return "(unknown)".into();
    };
    if w.is_empty() {
        return "(none)".into();
    }
    // Sorted: a HashSet iterates in arbitrary order, which would make the
    // round-to-round comparison below fire at random.
    let mut names: Vec<&str> = w.iter().map(String::as_str).collect();
    names.sort_unstable();
    names
        .iter()
        .map(|p| format!("- {p}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lets an agent delegate. The child runs the same loop with its own agent
/// definition, its own permissions and a fresh context; only its final message
/// comes back. It gets no runner of its own, so delegation cannot recurse.
fn subagent_runner(env: &RunEnv, model: String, effort: String) -> agent::SubagentRunner {
    let env = env.clone();
    std::sync::Arc::new(move |def: &'static agent::agents::AgentDef, prompt, cancel| {
        let env = env.clone();
        let model = model.clone();
        let effort = effort.clone();
        Box::pin(async move {
            let perms = def.permissions();
            let tools = ToolRegistry::for_permissions(&perms);
            let mut child = env.clone();
            child.cancel = cancel;
            run_agent(
                AgentRole::Single,
                format!("{}\n\n{prompt}", def.prompt),
                model,
                effort,
                tools,
                false,
                // Finally wired up: this field has named a per-agent budget
                // since agents.rs was written, but nothing ever read it and
                // every subagent shared one hardcoded cap regardless of role.
                def.steps,
                &child,
                Vec::new(),
                None,
                perms,
                None,
            )
            .await
        })
    })
}

/// Modes are now presets over the permission model rather than a separate
/// mechanism. Keeping them means the existing composer keeps working while the
/// agent definitions in `agent::agents` take over underneath.
fn read_only_perms() -> PermissionSet {
    PermissionSet::from_pairs(&[
        ("edit_file", Permission::Deny),
        ("write_file", Permission::Deny),
        ("run_command", Permission::Deny),
        // A read-only role must not reach the shell by delegating to `general`.
        ("task", Permission::Deny),
    ])
}

/// Manual asks before every shell command; everything else runs freely.
fn manual_perms() -> PermissionSet {
    PermissionSet::from_pairs(&[("run_command", Permission::Ask)])
}

fn coder_perms(mode: Mode) -> PermissionSet {
    if mode == Mode::Approve {
        PermissionSet::from_pairs(&[("run_command", Permission::Deny)])
    } else {
        PermissionSet::allow_all()
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
    max_turns: u32,
    env: &RunEnv,
    seed: Vec<Message>,
    perm: Option<PermGate>,
    perms: ide_core::PermissionSet,
    spawn: Option<agent::SubagentRunner>,
) -> Result<String> {
    // A panicking role must degrade into an error, not kill the whole
    // pipeline task silently.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Result<String>>(1);
    let env = env.clone();
    let handle = tokio::spawn(async move {
        let r = run_agent(
            role, prompt, model, effort, tools, announce_done, max_turns, &env, seed, perm, perms, spawn,
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
