use anyhow::Result;
use async_trait::async_trait;
use ide_core::{AgentEvent, CommandOutput, WorkspaceRoot};
use sandbox::Sandbox;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

pub struct ToolCtx {
    pub ws: WorkspaceRoot,
    pub sandbox: Arc<dyn Sandbox>,
    pub events: broadcast::Sender<AgentEvent>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> Value;
    async fn call(&self, ctx: &ToolCtx, input: Value, cancel: &CancellationToken) -> Result<String>;
}

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn full() -> Self {
        Self {
            tools: vec![
                Box::new(ReadFile),
                Box::new(EditFile),
                Box::new(RunCommand),
                Box::new(CheckCode),
                Box::new(RunTests),
            ],
        }
    }

    pub fn read_only() -> Self {
        Self {
            tools: vec![Box::new(ReadFile), Box::new(CheckCode), Box::new(RunTests)],
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Tool> {
        self.tools.iter().map(|t| t.as_ref())
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    pub fn schemas(&self) -> Vec<Value> {
        self.tools
            .iter()
            .map(|t| {
                json!({
                    "name": t.name(),
                    "description": t.description(),
                    "input_schema": t.input_schema(),
                })
            })
            .collect()
    }
}

struct ReadFile;
struct EditFile;
struct RunCommand;
struct CheckCode;
struct RunTests;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn description(&self) -> &'static str {
        "Read a text file in the opened workspace. Path is relative to the workspace root."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Workspace-relative path" }
            },
            "required": ["path"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, _cancel: &CancellationToken) -> Result<String> {
        let path = input["path"].as_str().unwrap_or("");
        let raw = viewer::read_file(&ctx.ws, path)?;
        Ok(crate::context::clip_file(path, &raw))
    }
}

#[async_trait]
impl Tool for EditFile {
    fn name(&self) -> &'static str {
        "edit_file"
    }
    fn description(&self) -> &'static str {
        "Replace exact text in a workspace file. Creates the file if old_text is empty."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "old_text": { "type": "string", "description": "Exact text to find; empty to create/overwrite" },
                "new_text": { "type": "string" }
            },
            "required": ["path", "new_text"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, _cancel: &CancellationToken) -> Result<String> {
        let path = input["path"].as_str().unwrap_or("");
        let old = input["old_text"].as_str().unwrap_or("");
        let new = input["new_text"].as_str().unwrap_or("");
        let before = if old.is_empty() {
            String::new()
        } else {
            viewer::read_file(&ctx.ws, path).unwrap_or_default()
        };
        let after = if old.is_empty() {
            new.to_string()
        } else {
            if !before.contains(old) {
                anyhow::bail!("old_text not found in {path}");
            }
            before.replacen(old, new, 1)
        };
        viewer::write_file(&ctx.ws, path, &after)?;
        let diff = unified_diff(path, &before, &after);
        let _ = ctx.events.send(AgentEvent::Diff {
            path: path.to_string(),
            diff: diff.clone(),
        });
        Ok(if diff.is_empty() {
            "no changes".into()
        } else {
            diff
        })
    }
}

#[async_trait]
impl Tool for RunCommand {
    fn name(&self) -> &'static str {
        "run_command"
    }
    fn description(&self) -> &'static str {
        "Run a program in the workspace sandbox (no raw shell). Give the executable name and args separately."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "program": { "type": "string", "description": "Executable name, e.g. cargo or git" },
                "args": { "type": "array", "items": { "type": "string" } }
            },
            "required": ["program"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let program = input["program"].as_str().unwrap_or("");
        if program.is_empty() {
            anyhow::bail!("program required");
        }
        let args: Vec<String> = input["args"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        format_cmd(ctx.sandbox.run(&ctx.ws, program, &args, Duration::from_secs(120)).await?)
    }
}

#[async_trait]
impl Tool for CheckCode {
    fn name(&self) -> &'static str {
        "check_code"
    }
    fn description(&self) -> &'static str {
        "Run cargo check, then cargo clippy if check succeeds."
    }
    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn call(&self, ctx: &ToolCtx, _input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let check = ctx
            .sandbox
            .run(
                &ctx.ws,
                "cargo",
                &["check".into(), "--message-format=short".into()],
                Duration::from_secs(180),
            )
            .await?;
        if check.exit_code != 0 {
            return format_cmd(check);
        }
        let clippy = ctx
            .sandbox
            .run(
                &ctx.ws,
                "cargo",
                &[
                    "clippy".into(),
                    "--message-format=short".into(),
                    "--".into(),
                    "-W".into(),
                    "clippy::all".into(),
                ],
                Duration::from_secs(180),
            )
            .await;
        match clippy {
            Ok(c) => format_cmd(c),
            Err(_) => format_cmd(check),
        }
    }
}

#[async_trait]
impl Tool for RunTests {
    fn name(&self) -> &'static str {
        "run_tests"
    }
    fn description(&self) -> &'static str {
        "Run cargo test in the sandbox."
    }
    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn call(&self, ctx: &ToolCtx, _input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        format_cmd(
            ctx.sandbox
                .run(&ctx.ws, "cargo", &["test".into()], Duration::from_secs(300))
                .await?,
        )
    }
}

fn format_cmd(out: CommandOutput) -> Result<String> {
    Ok(format!(
        "exit {}\nstdout:\n{}\nstderr:\n{}",
        out.exit_code, out.stdout, out.stderr
    ))
}

fn unified_diff(path: &str, before: &str, after: &str) -> String {
    let diff = TextDiff::from_lines(before, after);
    let mut s = format!("--- a/{path}\n+++ b/{path}\n");
    for change in diff.iter_all_changes() {
        let sign = match change.tag() {
            ChangeTag::Delete => "-",
            ChangeTag::Insert => "+",
            ChangeTag::Equal => " ",
        };
        s.push_str(sign);
        s.push_str(change.value());
        if !change.value().ends_with('\n') {
            s.push('\n');
        }
    }
    s
}
