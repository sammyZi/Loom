use anyhow::{bail, Result};
use async_trait::async_trait;
use core::{CommandOutput, WorkspaceRoot};
use std::time::Duration;

pub struct UnsupportedSandbox;

#[async_trait]
impl crate::Sandbox for UnsupportedSandbox {
    async fn run(
        &self,
        _ws: &WorkspaceRoot,
        _program: &str,
        _args: &[String],
        _timeout: Duration,
    ) -> Result<CommandOutput> {
        bail!("OS-native sandbox is implemented for Windows only in v1")
    }
}
