use anyhow::Result;
use async_trait::async_trait;
use core::{CommandOutput, WorkspaceRoot};
use std::sync::Arc;
use std::time::Duration;

#[cfg(windows)]
mod windows_impl;
#[cfg(not(windows))]
mod unsupported;

#[async_trait]
pub trait Sandbox: Send + Sync {
    async fn run(
        &self,
        ws: &WorkspaceRoot,
        program: &str,
        args: &[String],
        timeout: Duration,
    ) -> Result<CommandOutput>;
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
