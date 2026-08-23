use anyhow::Result;
use async_trait::async_trait;
use ide_core::{CommandOutput, WorkspaceRoot};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
mod windows_impl;
#[cfg(not(windows))]
mod unsupported;

/// Receives output as it is produced. Implementors also return the full text in
/// `CommandOutput`, so a caller that does not care about progress can ignore it.
pub type OutputSink = tokio::sync::mpsc::UnboundedSender<String>;

/// Keystrokes for a running command's stdin. Without one the child gets no
/// stdin at all, so anything that prompts (`date`, `npm init`) dies instead of
/// waiting. Dropping the sender closes the pipe, which the child sees as EOF.
pub type InputSource = tokio::sync::mpsc::UnboundedReceiver<String>;

#[async_trait]
pub trait Sandbox: Send + Sync {
    /// Run to completion, or until `timeout` elapses, or until `cancel` fires —
    /// whichever comes first. The whole process tree is killed on every one of
    /// those exits, so a caller that stops waiting never leaks a child.
    ///
    /// `on_output` receives stdout and stderr chunks as they arrive.
    async fn run_streaming(
        &self,
        ws: &WorkspaceRoot,
        program: &str,
        args: &[String],
        timeout: Duration,
        cancel: &CancellationToken,
        on_output: Option<OutputSink>,
        stdin: Option<InputSource>,
    ) -> Result<CommandOutput>;

    /// Wait for the whole output instead of following it. Most callers want this.
    async fn run(
        &self,
        ws: &WorkspaceRoot,
        program: &str,
        args: &[String],
        timeout: Duration,
        cancel: &CancellationToken,
    ) -> Result<CommandOutput> {
        self.run_streaming(ws, program, args, timeout, cancel, None, None).await
    }
}

pub fn native() -> Arc<dyn Sandbox> {
    #[cfg(windows)]
    {
        Arc::new(windows_impl::WindowsSandbox)
    }
    #[cfg(not(windows))]
    {
        Arc::new(unsupported::UnsupportedSandbox)
    }
}

pub fn deny_network_env() -> Vec<(String, String)> {
    [
        ("HTTPS_PROXY", "http://127.0.0.1:9"),
        ("HTTP_PROXY", "http://127.0.0.1:9"),
        ("ALL_PROXY", "http://127.0.0.1:9"),
        ("NO_PROXY", "localhost,127.0.0.1,::1"),
        ("GIT_HTTPS_PROXY", "http://127.0.0.1:9"),
        ("GIT_HTTP_PROXY", "http://127.0.0.1:9"),
        ("GIT_SSH_COMMAND", "cmd /c exit 1"),
        ("CARGO_NET_OFFLINE", "true"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect()
}

pub fn passthrough_env() -> Vec<(String, String)> {
    const KEEP: &[&str] = &[
        "PATH",
        "PATHEXT",
        "SYSTEMROOT",
        "SYSTEMDRIVE",
        "WINDIR",
        "COMSPEC",
        "NUMBER_OF_PROCESSORS",
        "PROCESSOR_ARCHITECTURE",
        "PROCESSOR_IDENTIFIER",
        "OS",
        "HOMEDRIVE",
        "HOMEPATH",
        "USERPROFILE",
        "USERNAME",
        "USERDOMAIN",
        "RUSTUP_HOME",
        "CARGO_HOME",
        "ProgramFiles",
        "ProgramFiles(x86)",
        "ProgramData",
        "LOCALAPPDATA",
        "APPDATA",
    ];
    KEEP.iter()
        .filter_map(|k| std::env::var(k).ok().map(|v| ((*k).to_string(), v)))
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn network_env_points_at_discard() {
        let env = super::deny_network_env();
        let https = env.iter().find(|(k, _)| k == "HTTPS_PROXY").unwrap();
        assert_eq!(https.1, "http://127.0.0.1:9");
    }

    /// A slow command that emits lines as it goes. Ping used to be the fixture,
    /// but under the restricted token some machines cannot even resolve a
    /// numeric address, so PowerShell printing in a loop keeps this portable.
    #[cfg(windows)]
    fn slow_command() -> Vec<String> {
        vec![
            "-NoProfile".into(),
            "-Command".into(),
            "1..40 | ForEach-Object { \"tick-$_\"; Start-Sleep -Milliseconds 250 }".into(),
        ]
    }

    /// The bug this guards: `ping -t` never exits, so the terminal hung until
    /// the timeout. Cancelling has to kill the tree and return straight away.
    #[cfg(windows)]
    #[tokio::test]
    async fn cancel_kills_a_command_that_never_exits() {
        use super::*;
        use tokio_util::sync::CancellationToken;
        use ide_core::WorkspaceRoot;

        let dir = std::env::temp_dir().join("ide-ai-sandbox-cancel-test");
        std::fs::create_dir_all(&dir).unwrap();
        let ws = WorkspaceRoot::open(&dir).unwrap();
        let sandbox = native();
        let cancel = CancellationToken::new();

        let c = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(700)).await;
            c.cancel();
        });

        // Timeout is far beyond the cancel, so finishing early proves the
        // cancel — not the deadline — is what stopped it.
        let started = std::time::Instant::now();
        let out = sandbox
            .run(&ws, "powershell", &slow_command(), Duration::from_secs(60), &cancel)
            .await
            .expect("run should return, not hang");
        let took = started.elapsed();

        // The command would have run for ~10s, so returning early is proof the
        // tree was killed.
        assert!(took < Duration::from_secs(20), "took {took:?}, so it waited for the timeout");
        assert!(
            !out.stderr.contains("[sandbox]"),
            "a deliberate cancel should stay quiet, got stderr: {:?}",
            out.stderr
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// The bug this guards: the sandbox used to put its scratch dir inside the
    /// project, which left `.ide-ai-tmp` as untracked noise and — worse — made
    /// `create-next-app` refuse to scaffold, because it demands an empty
    /// directory. Running a command must leave the workspace untouched.
    #[cfg(windows)]
    #[tokio::test]
    async fn running_a_command_creates_nothing_in_the_workspace() {
        use super::*;
        use ide_core::WorkspaceRoot;
        use tokio_util::sync::CancellationToken;

        let dir = std::env::temp_dir().join("ide-ai-clean-workspace-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let ws = WorkspaceRoot::open(&dir).unwrap();

        let out = native()
            .run(
                &ws,
                "powershell",
                &["-NoProfile".into(), "-Command".into(), "Write-Output ok".into()],
                Duration::from_secs(60),
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert!(out.stdout.contains("ok"), "command should have run: {out:?}");

        let left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            left.is_empty(),
            "workspace must stay empty so scaffolding tools work, found: {left:?}"
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    /// The bug this guards: output only appeared once the command exited, so a
    /// long `ping` showed nothing but "running…" the whole time.
    #[cfg(windows)]
    #[tokio::test]
    async fn output_arrives_while_the_command_is_still_running() {
        use super::*;
        use ide_core::WorkspaceRoot;
        use tokio_util::sync::CancellationToken;

        let dir = std::env::temp_dir().join("ide-ai-sandbox-stream-test");
        std::fs::create_dir_all(&dir).unwrap();
        let ws = WorkspaceRoot::open(&dir).unwrap();
        let sandbox = native();
        let cancel = CancellationToken::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

        // Prints every 250ms for ~10s, so anything received proves streaming,
        // not completion.
        let c = cancel.clone();
        let run = tokio::spawn(async move {
            sandbox
                .run_streaming(&ws, "powershell", &slow_command(), Duration::from_secs(60), &c, Some(tx), None)
                .await
        });

        let first = tokio::time::timeout(Duration::from_secs(15), rx.recv())
            .await
            .expect("a chunk should arrive long before the command ends")
            .expect("sender should still be open");
        assert!(!first.is_empty(), "streamed chunk should carry text");

        cancel.cancel();
        let out = run.await.unwrap().unwrap();
        // The full text is still returned, so non-streaming callers lose nothing.
        assert!(out.stdout.contains("tick-"), "got stdout: {:?}", out.stdout);
        std::fs::remove_dir_all(&dir).ok();
    }
}
