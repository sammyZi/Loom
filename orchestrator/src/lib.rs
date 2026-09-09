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

/// How many changed files make a run worth an LLM reviewer once the project's
/// own checks already pass. Below this the checks are the whole verification
/// story and the run ends without one.
const REVIEW_MIN_FILES: usize = 3;

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
    // What the last round learned, appended to the next coder prompt: a failing
    // build's own output, a reviewer's REVISE, or the wrote-nothing nudge.
    //
    // This used to be the nudge alone. The reviewer's REVISE was assigned to
    // `last` and then never interpolated anywhere, so a second round re-issued
    // a byte-identical prompt and hoped for a different answer. The loop only
    // works if the feedback actually reaches the coder.
    let mut feedback = String::new();
    let mut nudged = false;
    // The last round's failing check, when it failed. Rounds are finite, so the
    // loop can run out with the build still broken — and reporting the coder's
    // cheerful summary in that case would be the worst outcome of the lot:
    // "done" over a project that does not compile.
    let mut unresolved: Option<String> = None;
    for round in 0..3 {
        if env.cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let _ = env
            .events
            .send(AgentEvent::Status { message: format!("coder {}", round + 1) });
        last = spawn_role(
            AgentRole::Coder,
            coder_prompt(&prompt, &plan, coder_note, &feedback),
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

        // Nothing was written, so there is nothing to verify.
        if wrote_nothing(&env) {
            // Ending the run here was too quick a surrender: the commonest way
            // to write nothing is to spend the round reading and drafting the
            // change in prose, which one pointed retry usually fixes. Only a
            // second empty round is treated as stuck.
            if !nudged {
                nudged = true;
                feedback = "\n\nYour last attempt changed no files: you explored the code and \
                         described the work instead of doing it. Do not plan again, and do not \
                         put file contents in your reply — they belong in a write_file or \
                         edit_file call. Start with the single most important file and edit it \
                         now; you already have what you need to begin."
                    .into();
                let _ = env.events.send(AgentEvent::Status {
                    message: "nothing written — retrying".into(),
                });
                continue;
            }
            let note = format!(
                "{last}\n\n**No files were changed.** Two attempts explored the code without \
                 editing anything, so this is unfinished rather than done. A narrower ask — one \
                 file, one change — usually gets past it."
            );
            let _ = env.events.send(AgentEvent::Done { summary: note.clone() });
            return Ok(note);
        }

        // The gate: verify deterministically rather than paying an LLM to read
        // exit codes and relay them. This replaced an unconditional reviewer
        // role whose entire verdict was discarded on the happy path anyway.
        let _ = env.events.send(AgentEvent::Status { message: "checking".into() });
        if let Some(report) = gate_check(&env).await {
            // A compiler's own output is better feedback than an LLM's
            // paraphrase of it, and it costs nothing to produce.
            feedback = format!(
                "\n\nThe project does not build, or its tests fail, after your last change. Fix \
                 that before anything else — this is the output of the project's own toolchain, \
                 not an opinion:\n\n{report}"
            );
            let touched = changed_files(&env);
            if touched == before_round {
                let note = format!(
                    "{last}\n\n**Stopped after round {}:** the checks are still failing and the \
                     last pass changed no files, so another round would repeat it.\n\n{report}",
                    round + 1
                );
                let _ = env.events.send(AgentEvent::Done { summary: note.clone() });
                return Ok(note);
            }
            before_round = touched;
            unresolved = Some(report);
            continue;
        }
        unresolved = None;

        let changed = changed_files(&env);
        // Checks pass. On a small change that is the whole verification story,
        // so the run ends here having spent nothing on a reviewer. A larger
        // change still gets one, because what is left to catch is semantic —
        // work that compiles and is nonetheless half-done — and no type-checker
        // sees that.
        if changed_count(&env) < REVIEW_MIN_FILES {
            let summary = non_empty(&last, &changed);
            let _ = env.events.send(AgentEvent::Done { summary: summary.clone() });
            return Ok(summary);
        }

        let _ = env.events.send(AgentEvent::Status { message: "reviewer".into() });
        // Hand over the diff, not the project. The reviewer used to be given a
        // summary and left to re-read every file to find the change — the
        // single biggest source of duplicated reads in a run.
        let review = spawn_role(
            AgentRole::Reviewer,
            format!(
                "User task:\n{prompt}\n\nCoder summary:\n{last}\n\nFiles this run changed:\n\
                 {changed}\n\nThe project already builds and its tests already pass — that is \
                 checked before you are asked, so do not re-run checks and do not report on \
                 them. Look only for work that is wrong or unfinished: something the task asked \
                 for that these changes do not do. Review only those files and only what \
                 changed in them. Do not read anything else. Start with APPROVED or REVISE."
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
        if review.to_ascii_uppercase().contains("APPROVED") {
            // The reviewer is internal QA. Its verdict and its file-by-file
            // notes answer nothing the user asked for, so a passing review ends
            // the run silently and the coder's summary — the one that describes
            // the change — is what ships.
            // A coder that ran out of turns mid-tool-call leaves no closing
            // text at all, and an empty bubble tells the user nothing about
            // work that did happen; name the files in that case.
            let summary = non_empty(&last, &changed);
            let _ = env.events.send(AgentEvent::Done { summary: summary.clone() });
            return Ok(summary);
        }
        feedback = format!("\n\nA reviewer read your changes and asked for revisions:\n\n{review}");
        last = review.clone();
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
    // Out of rounds. If the checks were still failing on the way out, that is
    // the headline — not the coder's summary of what it meant to do.
    let note = match unresolved {
        Some(report) => format!(
            "{last}\n\n**The project's checks are still failing after three rounds**, so this is \
             unfinished rather than done:\n\n{report}"
        ),
        None => last.clone(),
    };
    let _ = env.events.send(AgentEvent::Done { summary: note.clone() });
    Ok(note)
}

/// The coder's instructions for one round. Split out so a test can prove the
/// previous round's feedback actually reaches it — the bug this replaced was
/// invisible precisely because the prompt was built inline.
fn coder_prompt(task: &str, plan: &str, note: &str, feedback: &str) -> String {
    format!("User task:\n{task}\n\nPlan:\n{plan}\n\nImplement now.{note}{feedback}")
}

/// Run the project's own checks. `Some(report)` when something failed and the
/// coder needs to see it; `None` when everything that could be verified passed.
///
/// Tests run only once the build is clean: a project that does not compile
/// fails its tests for the same single reason, and reporting both just doubles
/// the output the next round has to read.
async fn gate_check(env: &RunEnv) -> Option<String> {
    match agent::checks::check_code(&env.ws, &env.sandbox, &env.cancel).await {
        Ok(c) if !c.passed => return Some(c.report),
        Ok(_) => {}
        // A check that could not start at all — missing toolchain, cancelled —
        // is not evidence the code is broken. Sending the coder to fix a
        // phantom build failure would be worse than not checking.
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

/// The coder's own words when it left any, otherwise the shortest honest
/// account of the run: what it changed.
fn non_empty(summary: &str, changed: &str) -> String {
    if !summary.trim().is_empty() {
        return summary.to_string();
    }
    if changed.trim().is_empty() {
        return "Done — no files changed.".into();
    }
    format!("Done. Files changed:
{changed}")
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

    /// The gate picks the reviewer's fate from this count, so it has to agree
    /// with the list `changed_files` renders — one counting the set and the
    /// other formatting it must never disagree about how many there are.
    #[test]
    fn changed_count_agrees_with_the_rendered_list() {
        for paths in [vec![], vec!["a.rs"], vec!["a.rs", "b.rs", "c.rs"]] {
            let w = writes(&paths);
            let listed = super::changed_files_from(&w);
            let counted = w.lock().unwrap().len();
            assert_eq!(counted, paths.len());
            let rendered = if paths.is_empty() {
                0
            } else {
                listed.lines().count()
            };
            assert_eq!(rendered, counted, "list and count disagree for {paths:?}");
        }
    }

    /// The regression test for the loop that never closed: a reviewer's REVISE
    /// and a failing build's output were computed and then dropped, so round
    /// two re-sent round one's prompt verbatim. If feedback stops reaching the
    /// coder again, this fails.
    #[test]
    fn the_next_round_carries_the_last_rounds_feedback() {
        let first = super::coder_prompt("add a button", "1. edit App.tsx", "", "");
        assert!(first.contains("add a button"));
        assert!(first.contains("1. edit App.tsx"));
        assert!(first.ends_with("Implement now."), "no feedback on round one: {first}");

        let second = super::coder_prompt(
            "add a button",
            "1. edit App.tsx",
            "",
            "\n\nerror[E0425]: cannot find value `handleClick`",
        );
        assert!(
            second.contains("E0425"),
            "the failure has to reach the coder, or the round repeats: {second}"
        );
        assert_ne!(first, second, "a round with feedback must not repeat the last prompt");
    }

    /// Approve mode's note and the feedback are separate inputs and both have
    /// to survive; an earlier version interpolated only one of them.
    #[test]
    fn the_mode_note_and_the_feedback_both_survive() {
        let p = super::coder_prompt("t", "p", " SHELL-WITHHELD", " FEEDBACK");
        assert!(p.contains("SHELL-WITHHELD") && p.contains("FEEDBACK"), "{p}");
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

/// How many distinct files this task has written — the same set
/// `changed_files` formats, counted rather than rendered.
fn changed_count(env: &RunEnv) -> usize {
    env.writes.lock().map(|w| w.len()).unwrap_or(0)
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
