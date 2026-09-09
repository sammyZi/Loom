//! Type-check and test the open project, using whatever toolchain it actually
//! uses.
//!
//! These live outside `tools.rs` because two callers need the same body: the
//! `check_code` / `run_tests` tools, which let the model verify its own work
//! mid-task, and the orchestrator's gate, which verifies deterministically
//! after the coder finishes. The gate used to be an LLM reviewer whose entire
//! contribution was reading these exit codes and relaying them.

use crate::project::Stack;
use anyhow::Result;
use ide_core::WorkspaceRoot;
use sandbox::Sandbox;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// The result of one check.
///
/// `report` is what a model reads — exit code and both streams, already
/// clipped. `passed` is what the harness branches on, so the orchestrator
/// never has to string-match its way to a verdict.
pub struct CheckOutcome {
    pub passed: bool,
    pub report: String,
}

impl CheckOutcome {
    /// A check that ran and succeeded, or had nothing to do.
    ///
    /// "This project has no type-check configured" is an answer, not a
    /// failure. Counting it as one would put the gate into a loop asking the
    /// coder to fix a build that does not exist.
    fn ok(report: impl Into<String>) -> Self {
        Self { passed: true, report: report.into() }
    }

    fn from_command(out: ide_core::CommandOutput) -> Result<Self> {
        let passed = out.exit_code == 0;
        Ok(Self { passed, report: crate::tools::format_cmd(out)? })
    }
}

/// Type-check or lint the project. Cargo projects also get clippy once the
/// build itself is clean.
pub async fn check_code(
    ws: &WorkspaceRoot,
    sandbox: &Arc<dyn Sandbox>,
    cancel: &CancellationToken,
) -> Result<CheckOutcome> {
    if cancel.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    match Stack::detect(ws) {
        Stack::Cargo => {}
        Stack::Node => {
            if !ws.root().join("tsconfig.json").exists() {
                return Ok(CheckOutcome::ok("no type-check configured (no tsconfig.json)"));
            }
            return CheckOutcome::from_command(
                sandbox
                    .run(
                        ws,
                        "npx",
                        &["tsc".into(), "--noEmit".into()],
                        Duration::from_secs(240),
                        cancel,
                    )
                    .await?,
            );
        }
        Stack::Go => {
            return CheckOutcome::from_command(
                sandbox
                    .run(
                        ws,
                        "go",
                        &["build".into(), "./...".into()],
                        Duration::from_secs(240),
                        cancel,
                    )
                    .await?,
            );
        }
        Stack::Python | Stack::Unknown => {
            return Ok(CheckOutcome::ok("no check configured for this project type"));
        }
    }
    let check = sandbox
        .run(
            ws,
            "cargo",
            &["check".into(), "--message-format=short".into()],
            Duration::from_secs(180),
            cancel,
        )
        .await?;
    if check.exit_code != 0 {
        return CheckOutcome::from_command(check);
    }
    let clippy = sandbox
        .run(
            ws,
            "cargo",
            &[
                "clippy".into(),
                "--message-format=short".into(),
                "--".into(),
                "-W".into(),
                "clippy::all".into(),
            ],
            Duration::from_secs(180),
            cancel,
        )
        .await;
    match clippy {
        Ok(c) => CheckOutcome::from_command(c),
        // Clippy missing is not a failing build; fall back to the check that
        // already passed rather than reporting a toolchain gap as a defect.
        Err(_) => CheckOutcome::from_command(check),
    }
}

/// Run the project's test suite, or say why there is nothing to run.
pub async fn run_tests(
    ws: &WorkspaceRoot,
    sandbox: &Arc<dyn Sandbox>,
    cancel: &CancellationToken,
) -> Result<CheckOutcome> {
    if cancel.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    let stack = Stack::detect(ws);
    // Ask the project whether it has tests before paying to start a runner.
    // `npm test` in a package with no test script burns a process and comes
    // back "exit 1, no output" on every single run, which reads like a
    // failure and taught nobody anything.
    if let Some(why) = no_test_suite(ws, stack) {
        return Ok(CheckOutcome::ok(why));
    }
    let (program, args): (&str, Vec<String>) = match stack {
        Stack::Cargo => ("cargo", vec!["test".into()]),
        Stack::Node => ("npm", vec!["test".into(), "--silent".into()]),
        Stack::Go => ("go", vec!["test".into(), "./...".into()]),
        Stack::Python => ("python", vec!["-m".into(), "pytest".into(), "-q".into()]),
        Stack::Unknown => {
            return Ok(CheckOutcome::ok("no test runner configured for this project type"))
        }
    };
    CheckOutcome::from_command(
        sandbox.run(ws, program, &args, Duration::from_secs(300), cancel).await?,
    )
}

/// `Some(reason)` when the project plainly has no suite to run, so the caller
/// can say so instead of starting a runner that will fail for want of tests.
/// Only claims "no tests" on evidence; anything ambiguous runs as before.
pub fn no_test_suite(ws: &WorkspaceRoot, stack: Stack) -> Option<String> {
    let root = ws.root();
    match stack {
        Stack::Node => {
            let pkg = std::fs::read_to_string(root.join("package.json")).ok()?;
            let json: Value = serde_json::from_str(&pkg).ok()?;
            // A missing `scripts` object means no test script — the `?` here
            // used to bail out of the whole check and run the runner anyway.
            let script = json
                .get("scripts")
                .and_then(|s| s.get("test"))
                .and_then(|v| v.as_str());
            match script {
                // `npm init` writes this placeholder; it is not a test suite.
                None => Some("this project has no test script in package.json — nothing to run".into()),
                Some(s) if s.contains("no test specified") => {
                    Some("package.json still has npm's placeholder test script — no suite to run".into())
                }
                Some(_) => None,
            }
        }
        Stack::Python => {
            let has = root.join("tests").is_dir()
                || root.join("pytest.ini").exists()
                || root.join("conftest.py").exists()
                || std::fs::read_dir(root).ok()?.flatten().any(|e| {
                    e.file_name().to_string_lossy().starts_with("test_")
                });
            (!has).then(|| "no tests directory or test_*.py in this project — nothing to run".into())
        }
        // cargo and go both handle an empty suite sensibly and cheaply.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_ws(name: &str) -> WorkspaceRoot {
        let d = std::env::temp_dir().join(format!("ide-ai-checks-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        WorkspaceRoot::open(&d).unwrap()
    }

    #[test]
    fn node_without_a_test_script_is_not_run() {
        let ws = tmp_ws("node-none");
        std::fs::write(ws.root().join("package.json"), r#"{"name":"x"}"#).unwrap();
        let why = no_test_suite(&ws, Stack::Node);
        assert!(why.is_some(), "a package with no test script must be skipped");
    }

    #[test]
    fn npm_placeholder_script_is_not_a_suite() {
        let ws = tmp_ws("node-placeholder");
        let pkg = r#"{"scripts":{"test":"echo \"Error: no test specified\" && exit 1"}}"#;
        std::fs::write(ws.root().join("package.json"), pkg).unwrap();
        assert!(no_test_suite(&ws, Stack::Node).is_some());
    }

    #[test]
    fn a_real_test_script_still_runs() {
        let ws = tmp_ws("node-real");
        std::fs::write(ws.root().join("package.json"), r#"{"scripts":{"test":"vitest run"}}"#).unwrap();
        assert!(no_test_suite(&ws, Stack::Node).is_none());
    }

    #[test]
    fn python_needs_visible_tests() {
        let ws = tmp_ws("py");
        assert!(no_test_suite(&ws, Stack::Python).is_some());
        std::fs::create_dir_all(ws.root().join("tests")).unwrap();
        assert!(no_test_suite(&ws, Stack::Python).is_none());
    }

    /// The gate loops on a failing check, so "there is nothing to check" must
    /// come back as a pass. Reporting it as a failure would send the coder
    /// round after round trying to fix a build the project does not have.
    #[test]
    fn nothing_to_run_counts_as_passing() {
        for reason in [
            "no check configured for this project type",
            "no type-check configured (no tsconfig.json)",
            "this project has no test script in package.json — nothing to run",
        ] {
            let outcome = CheckOutcome::ok(reason);
            assert!(outcome.passed, "{reason:?} must not read as a failure");
            assert_eq!(outcome.report, reason, "the reason is what the model sees");
        }
    }

    /// The other half of the same contract: a real non-zero exit is a failure,
    /// and its report is what gets handed back to the coder.
    #[test]
    fn a_failing_command_is_a_failing_check() {
        let bad = ide_core::CommandOutput {
            exit_code: 101,
            stdout: String::new(),
            stderr: "error[E0425]: cannot find value `x`".into(),
        };
        let outcome = CheckOutcome::from_command(bad).unwrap();
        assert!(!outcome.passed);
        assert!(outcome.report.contains("E0425"), "report carries the compiler text");

        let good = ide_core::CommandOutput {
            exit_code: 0,
            stdout: "ok".into(),
            stderr: String::new(),
        };
        assert!(CheckOutcome::from_command(good).unwrap().passed);
    }
}
