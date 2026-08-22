use anyhow::{bail, Result};
use async_trait::async_trait;
use ide_core::{CommandOutput, WorkspaceRoot};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

pub struct UnsupportedSandbox;

#[async_trait]
impl crate::Sandbox for UnsupportedSandbox {
    async fn run_streaming(
        &self,
        _ws: &WorkspaceRoot,
        _program: &str,
        _args: &[String],
        _timeout: Duration,
        _cancel: &CancellationToken,
        _on_output: Option<crate::OutputSink>,
    ) -> Result<CommandOutput> {
        bail!("OS-native sandbox is implemented for Windows only in v1")
    }
}
