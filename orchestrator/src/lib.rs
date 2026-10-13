use agent::{run_agent, Message, PermGate, RunEnv, ToolRegistry};
use anyhow::Result;
use ide_core::{AgentEvent, AgentRole, Permission, PermissionSet};

/// Tool-call turns before the agent gives up and hands back whatever it has.
const TURNS_TASK: u32 = 60;
const TURNS_PLAN: u32 = 25;
const TURNS_TRIVIAL: u32 = 6;
/// Extra passes the agent gets to fix a build or test failure it caused.
const FIX_ROUNDS: u32 = 2;

/// How much the agent may do. Picked by the user in the composer. These are
/// Codex's approval/sandbox policies: one agent loop, different permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Full tools, no prompts (Codex `--full-auto`).
    Auto,
    /// Read-only: investigate and propose a plan, change nothing.
    Plan,
    /// Full tools, but every shell command asks first (Codex `untrusted`).
    Manual,
    /// Edits allowed, shell withheld; the agent lists commands for the user.
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

/// One task, Codex-style: a single agent works the whole thing in one
/// conversation — plans with todo_write, fans out with subagents, edits,
/// verifies. This replaced a planner -> coder -> reviewer pipeline whose roles
/// each started from an empty context and re-read the same files, and whose
/// keyword routing sent "start the app" to a planner that could not run it.
///
/// Sends exactly one `Done`, at the end. The UI stops listening on the first
/// one, so an early Done from an inner role used to swallow everything after.
pub async fn run_task(
    prompt: String,
    model: String,
    mode: Mode,
    effort: String,
    env: RunEnv,
    // Earlier turns of this chat session.
    history: Vec<Message>,
    // Present in manual mode so shell commands ask before running.
    perm: Option<PermGate>,
) -> Result<String> {
    let answer = run_inner(prompt, model, mode, effort, &env, history, perm).await?;
    let _ = env.events.send(AgentEvent::Done { summary: answer.clone() });
    Ok(answer)
}

async fn run_inner(
    prompt: String,
    model: String,
    mode: Mode,
    effort: String,
    env: &RunEnv,
    history: Vec<Message>,
    perm: Option<PermGate>,
) -> Result<String> {
    if is_greeting(&prompt) {
        let _ = env.events.send(AgentEvent::Status { message: "agent".into() });
        let (answer, _) = run_agent(
            AgentRole::Single,
            prompt,
            model,
            effort,
            ToolRegistry::none(),
            true,
            TURNS_TRIVIAL,
            env,
            history,
            None,
            read_only_perms(),
            None,
        )
        .await?;
        return Ok(answer);
    }

    let (role, perms, turns) = match mode {
        Mode::Plan => (AgentRole::Planner, read_only_perms(), TURNS_PLAN),
        Mode::Manual => (AgentRole::Single, manual_perms(), TURNS_TASK),
        Mode::Auto => (AgentRole::Coder, PermissionSet::allow_all(), TURNS_TASK),
        Mode::Approve => (AgentRole::Coder, no_shell_perms(), TURNS_TASK),
    };
    let task = if mode == Mode::Approve {
        format!(
            "{prompt}\n\n(Approve mode: the shell is withheld on purpose — a setting, not a missing \
             capability. List any commands that need running at the end under `Commands to run:`.)"
        )
    } else {
        prompt.clone()
    };
    let runner = (mode != Mode::Plan)
        .then(|| subagent_runner(env, model.clone(), effort.clone(), perm.clone(), perms.clone()));

    let _ = env.events.send(AgentEvent::Status { message: "working".into() });
    let (mut answer, mut transcript) = run_agent(
        role,
        task,
        model.clone(),
        effort.clone(),
        ToolRegistry::for_permissions(&perms),
        true,
        turns,
        env,
        history,
        perm.clone(),
        perms.clone(),
        runner.clone(),
    )
    .await?;

    // The deterministic gate: the project's own build and tests, not an LLM
    // reading exit codes. A failure goes back into the same conversation, so
    // the agent fixes it with everything it already knows.
    if matches!(mode, Mode::Auto | Mode::Approve) && !wrote_nothing(env) {
        let mut round = 0;
        loop {
            if env.cancel.is_cancelled() {
                anyhow::bail!("cancelled");
            }
            let _ = env.events.send(AgentEvent::Status { message: "checking".into() });
            // Shown as a feed row, which also separates this round's reply
            // from the next one's.
            let _ = env.events.send(AgentEvent::ToolCall {
                name: "check_code".into(),
                input: Default::default(),
            });
            let Some(report) = gate_check(env).await else { break };
            if round == FIX_ROUNDS {
                answer.push_str(&say(
                    env,
                    format!(
                        "\n\n**The project's checks are still failing**, so this is unfinished:\n\n{}",
                        fence(&report)
                    ),
                ));
                break;
            }
            round += 1;
            let _ = env
                .events
                .send(AgentEvent::Status { message: format!("fixing ({round})") });
            (answer, transcript) = run_agent(
                role,
                format!(
                    "The project does not build, or its tests fail, after your change. This is the \
                     toolchain's own output. Fix it, then reply with the final summary of the \
                     whole task.\n\n{}",
                    fence(&report)
                ),
                model.clone(),
                effort.clone(),
                ToolRegistry::for_permissions(&perms),
                true,
                turns,
                env,
                transcript,
                perm.clone(),
                perms.clone(),
                runner.clone(),
            )
            .await?;
        }
    }

    // A run asked to change something that wrote nothing is unfinished, not
    // done — running the dev server and opening a browser is not the work.
    if mode != Mode::Plan && wrote_nothing(env) && asks_for_a_change(&prompt) {
        answer.push_str(&say(
            env,
            "\n\n**No files were changed.** This asked for a change but nothing was written to \
             disk — treat it as unfinished."
                .into(),
        ));
    }
    if answer.trim().is_empty() {
        answer = say(env, non_empty("", &changed_files(env)));
    }
    Ok(answer)
}

/// Stream an addendum as answer text, so it lands in the same reply bubble as
/// the agent's own words. Returns it for the stored history.
fn say(env: &RunEnv, text: String) -> String {
    let _ = env.events.send(AgentEvent::Token { text: text.clone() });
    text
}

fn fence(report: &str) -> String {
    format!("```\n{}\n```", report.trim())
}

/// Run the project's own checks. `Some(report)` when something failed and the
/// agent needs to see it; `None` when everything that could be verified passed.
///
/// Tests run only once the build is clean: a project that does not compile
/// fails its tests for the same single reason, and reporting both just doubles
/// the output the next round has to read.
async fn gate_check(env: &RunEnv) -> Option<String> {
    match agent::checks::check_code(&env.ws, &env.sandbox, &env.cancel).await {
        Ok(c) if !c.passed => return Some(c.report),
        Ok(_) => {}
        // A check that could not start at all — missing toolchain, cancelled —
        // is not evidence the code is broken.
        Err(e) => {
            tracing::warn!("gate: check_code did not run ({e:#}); treating as unverified");
            return None;
        }
    }
    match agent::checks::run_tests(&env.ws, &env.sandbox, &env.cancel).await {
        Ok(t) if !t.passed => Some(t.report),
        Ok(_) => None,
        Err(e) => {
            tracing::warn!("gate: run_tests did not run ({e:#}); treating as unverified");
            None
        }
    }
}

/// The agent's own words when it left any, otherwise the shortest honest
/// account of the run: what it changed.
fn non_empty(summary: &str, changed: &str) -> String {
    if !summary.trim().is_empty() {
        return summary.to_string();
    }
    if changed.trim().is_empty() || changed == "(none)" {
        return "Done — no files changed.".into();
    }
    format!("Done. Files changed:\n{changed}")
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
    use std::collections::HashSet;
    use std::sync::{Arc, Mutex};

    fn writes(paths: &[&str]) -> Arc<Mutex<HashSet<String>>> {
        Arc::new(Mutex::new(paths.iter().map(|s| s.to_string()).collect()))
    }

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
        assert_eq!(super::non_empty("", "(none)"), "Done — no files changed.");
        assert_eq!(super::non_empty("", "- a.rs"), "Done. Files changed:\n- a.rs");
    }

    /// Every mode is the same loop under different permissions; these are
    /// the lines that must hold.
    #[test]
    fn mode_permissions() {
        use ide_core::Permission::*;
        let ro = super::read_only_perms();
        assert_eq!(ro.decide("edit_file", "a.rs"), Deny);
        assert_eq!(ro.decide("run_command", "ls"), Deny);
        assert_eq!(ro.decide("task", "general"), Deny, "no shell by delegation");
        assert_eq!(super::no_shell_perms().decide("run_command", "ls"), Deny);
        assert_eq!(super::no_shell_perms().decide("edit_file", "a.rs"), Allow);
        assert_eq!(super::manual_perms().decide("run_command", "ls"), Ask);
    }

    #[test]
    fn modes_parse() {
        use super::Mode;
        assert_eq!(Mode::parse("Plan"), Mode::Plan);
        assert_eq!(Mode::parse("cmd accept"), Mode::Approve);
        assert_eq!(Mode::parse(""), Mode::Auto);
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

/// True when the run edited nothing.
fn wrote_nothing(env: &RunEnv) -> bool {
    env.writes.lock().map(|w| w.is_empty()).unwrap_or(false)
}

/// The paths this run touched.
fn changed_files(env: &RunEnv) -> String {
    changed_files_from(&env.writes)
}

fn changed_files_from(
    writes: &std::sync::Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
) -> String {
    let Ok(w) = writes.lock() else {
        return "(unknown)".into();
    };
    if w.is_empty() {
        return "(none)".into();
    }
    // Sorted: a HashSet iterates in arbitrary order.
    let mut names: Vec<&str> = w.iter().map(String::as_str).collect();
    names.sort_unstable();
    names
        .iter()
        .map(|p| format!("- {p}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Lets an agent delegate, blocking (`task`) or in the background
/// (`spawn_agent`). The child runs the same loop with its own agent
/// definition and a fresh context; only its final message comes back. It gets
/// no delegation tools, so it cannot recurse.
///
/// Its events go through a filter: tool calls and diffs show in the feed, but
/// its streamed text, Done and Error stay private — several children
/// streaming into one reasoning block was unreadable, and a child's Error
/// ended the parent's run in the UI while the parent kept working.
fn subagent_runner(
    env: &RunEnv,
    model: String,
    effort: String,
    perm: Option<PermGate>,
    parent_perms: PermissionSet,
) -> agent::SubagentRunner {
    let env = env.clone();
    std::sync::Arc::new(move |def: &'static agent::agents::AgentDef, prompt, cancel| {
        let env = env.clone();
        let model = model.clone();
        let effort = effort.clone();
        let perm = perm.clone();
        let parent_perms = parent_perms.clone();
        Box::pin(async move {
            // `general` has full tools of its own, so it inherits the parent's
            // limits instead: delegating must not be a way around Manual's
            // approvals or Approve's missing shell.
            let perms = if def.id == "general" { parent_perms } else { def.permissions() };
            let tools = ToolRegistry::for_permissions(&perms)
                .without(&["task", "spawn_agent", "wait_agent", "close_agent", "todo_write"]);
            let (tx, mut rx) = tokio::sync::broadcast::channel::<AgentEvent>(1024);
            let parent = env.events.clone();
            tokio::spawn(async move {
                use tokio::sync::broadcast::error::RecvError;
                loop {
                    match rx.recv().await {
                        Ok(ev) => {
                            if matches!(
                                ev,
                                AgentEvent::ToolCall { .. }
                                    | AgentEvent::Diff { .. }
                                    | AgentEvent::Ask { .. }
                                    | AgentEvent::Usage { .. }
                                    | AgentEvent::Browse { .. }
                            ) {
                                let _ = parent.send(ev);
                            }
                        }
                        Err(RecvError::Lagged(_)) => continue,
                        Err(RecvError::Closed) => break,
                    }
                }
            });
            let mut child = env.clone();
            child.cancel = cancel;
            child.events = tx;
            let (answer, _) = run_agent(
                AgentRole::Single,
                format!("{}\n\n{prompt}", def.prompt),
                model,
                effort,
                tools,
                false,
                def.steps,
                &child,
                Vec::new(),
                perm,
                perms,
                None,
            )
            .await?;
            Ok(answer)
        })
    })
}

/// Plan mode: reads anything, changes nothing.
fn read_only_perms() -> PermissionSet {
    PermissionSet::from_pairs(&[
        ("edit_file", Permission::Deny),
        ("write_file", Permission::Deny),
        ("run_command", Permission::Deny),
        // A read-only role must not reach the shell by delegating to `general`.
        ("task", Permission::Deny),
        ("spawn_agent", Permission::Deny),
    ])
}

/// Manual asks before every shell command; everything else runs freely.
fn manual_perms() -> PermissionSet {
    PermissionSet::from_pairs(&[("run_command", Permission::Ask)])
}

/// Approve mode: edits yes, shell no.
fn no_shell_perms() -> PermissionSet {
    PermissionSet::from_pairs(&[("run_command", Permission::Deny)])
}
