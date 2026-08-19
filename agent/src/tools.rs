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
                Box::new(ListFiles),
                Box::new(WebFetch),
                Box::new(ReadFile),
                Box::new(EditFile),
                Box::new(RunCommand),
                Box::new(CheckCode),
                Box::new(RunTests),
            ],
        }
    }

    /// Everything except shell access, for the mode where the user runs commands.
    pub fn no_shell() -> Self {
        Self {
            tools: vec![
                Box::new(ListFiles),
                Box::new(WebFetch),
                Box::new(ReadFile),
                Box::new(EditFile),
                Box::new(CheckCode),
                Box::new(RunTests),
            ],
        }
    }

    pub fn read_only() -> Self {
        Self {
            tools: vec![
                Box::new(ListFiles),
                Box::new(WebFetch),
                Box::new(ReadFile),
                Box::new(CheckCode),
                Box::new(RunTests),
            ],
        }
    }

    pub fn none() -> Self {
        Self { tools: vec![] }
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

struct ListFiles;
struct WebFetch;
struct ReadFile;
struct EditFile;
struct RunCommand;
struct CheckCode;
struct RunTests;

#[async_trait]
impl Tool for ListFiles {
    fn name(&self) -> &'static str {
        "list_files"
    }
    fn description(&self) -> &'static str {
        "List files and folders in the opened workspace. Call this first to see what the \
         project actually contains before reading or describing any file."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Workspace-relative folder to list. Omit for the project root."
                }
            }
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, _cancel: &CancellationToken) -> Result<String> {
        let sub = input["path"].as_str();
        let files = crate::project::list_files(ctx.ws.root(), sub, 400);
        if files.is_empty() {
            return Ok("(empty)".into());
        }
        Ok(files.join("\n"))
    }
}

/// Largest page body we will pull into the model's context.
const WEB_LIMIT: usize = 120_000;

#[async_trait]
impl Tool for WebFetch {
    fn name(&self) -> &'static str {
        "web_fetch"
    }
    fn description(&self) -> &'static str {
        "Fetch a public http(s) URL and return its text, with HTML reduced to readable          content. Use for documentation, changelogs, API references and error messages."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "Absolute http:// or https:// URL" }
            },
            "required": ["url"]
        })
    }
    async fn call(&self, _ctx: &ToolCtx, input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let url = input["url"].as_str().unwrap_or("").trim().to_string();
        check_public_url(&url)?;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent("ide-ai/0.1")
            .build()?;
        let res = client.get(&url).send().await?;
        let status = res.status();
        let ctype = res
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = res.text().await.unwrap_or_default();

        let text = if ctype.contains("html") || body.trim_start().starts_with('<') {
            html_to_text(&body)
        } else {
            body
        };
        let mut text: String = text.chars().take(WEB_LIMIT).collect();
        if text.is_empty() {
            text.push_str("(empty response)");
        }
        Ok(format!("{status} {ctype}\n\n{text}"))
    }
}

/// Only public http(s). The agent runs beside a local API and on the user's LAN,
/// so loopback and private ranges stay off limits.
fn check_public_url(url: &str) -> Result<()> {
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        anyhow::bail!("only http:// and https:// URLs are allowed");
    }
    let host = lower
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .map(|h| h.split('@').next_back().unwrap_or(h))
        .map(|h| h.rsplit(':').next_back().unwrap_or(h))
        .unwrap_or("")
        .trim_matches(['[', ']'])
        .to_string();

    let blocked = host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".internal")
        || host == "::1"
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host.starts_with("169.254.")
        || host.starts_with("fd")
        || host.starts_with("fe80:")
        || (host.starts_with("172.")
            && host
                .split('.')
                .nth(1)
                .and_then(|o| o.parse::<u8>().ok())
                .is_some_and(|o| (16..=31).contains(&o)));

    if blocked {
        anyhow::bail!("refusing to fetch a local or private address: {host}");
    }
    Ok(())
}

/// Crude but dependency-free: drop script/style, strip tags, decode the few
/// entities that actually matter, and collapse whitespace.
fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let bytes: Vec<char> = html.chars().collect();
    let lower: String = html.to_ascii_lowercase();
    let mut i = 0usize;
    let mut in_tag = false;

    while i < bytes.len() {
        if !in_tag && lower[i..].starts_with("<script") {
            i = skip_block(&lower, i, "</script>");
            continue;
        }
        if !in_tag && lower[i..].starts_with("<style") {
            i = skip_block(&lower, i, "</style>");
            continue;
        }
        match bytes[i] {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
        i += 1;
    }

    let out = out
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");

    let mut collapsed = String::with_capacity(out.len());
    let mut blank = 0;
    for line in out.lines() {
        let t = line.split_whitespace().collect::<Vec<_>>().join(" ");
        if t.is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        collapsed.push_str(&t);
        collapsed.push('\n');
    }
    collapsed.trim().to_string()
}

fn skip_block(lower: &str, from: usize, end: &str) -> usize {
    match lower[from..].find(end) {
        Some(rel) => from + rel + end.len(),
        None => lower.len(),
    }
}

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
        "Run a program in the workspace sandbox and wait for it to finish. Give the executable \
         and args separately, not a shell line. Times out after 120s and the whole process tree \
         is killed on return, so use it for installs, builds, tests and checks, never for a \
         server or watcher that is meant to keep running."
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
        "Type-check or lint the project using whatever toolchain it actually uses. \
         Says so if the project has no check configured."
    }
    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn call(&self, ctx: &ToolCtx, _input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        use crate::project::Stack;
        match Stack::detect(&ctx.ws) {
            Stack::Cargo => {}
            Stack::Node => {
                if !ctx.ws.root().join("tsconfig.json").exists() {
                    return Ok("no type-check configured (no tsconfig.json)".into());
                }
                return format_cmd(
                    ctx.sandbox
                        .run(
                            &ctx.ws,
                            "npx",
                            &["tsc".into(), "--noEmit".into()],
                            Duration::from_secs(240),
                        )
                        .await?,
                );
            }
            Stack::Go => {
                return format_cmd(
                    ctx.sandbox
                        .run(&ctx.ws, "go", &["build".into(), "./...".into()], Duration::from_secs(240))
                        .await?,
                );
            }
            Stack::Python | Stack::Unknown => {
                return Ok("no check configured for this project type".into());
            }
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
        "Run the project's test suite using its own toolchain. \
         Says so if the project has no tests configured."
    }
    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }
    async fn call(&self, ctx: &ToolCtx, _input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        use crate::project::Stack;
        let (program, args): (&str, Vec<String>) = match Stack::detect(&ctx.ws) {
            Stack::Cargo => ("cargo", vec!["test".into()]),
            Stack::Node => ("npm", vec!["test".into(), "--silent".into()]),
            Stack::Go => ("go", vec!["test".into(), "./...".into()]),
            Stack::Python => ("python", vec!["-m".into(), "pytest".into(), "-q".into()]),
            Stack::Unknown => return Ok("no test runner configured for this project type".into()),
        };
        format_cmd(
            ctx.sandbox
                .run(&ctx.ws, program, &args, Duration::from_secs(300))
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

#[cfg(test)]
mod web_tests {
    use super::*;

    #[test]
    fn allows_ordinary_public_urls() {
        for u in [
            "https://doc.rust-lang.org/std/",
            "http://example.com/page?q=1",
            "https://api-docs.deepseek.com/guides/thinking_mode/",
            "https://sub.domain.co.uk:8443/path",
        ] {
            assert!(check_public_url(u).is_ok(), "should allow {u}");
        }
    }

    #[test]
    fn refuses_loopback_and_private_ranges() {
        // The agent sits next to its own API and on the user's LAN; these must not
        // be reachable through a URL the model made up.
        for u in [
            "http://localhost:8080/sessions",
            "http://127.0.0.1:8080/sessions",
            "http://[::1]/",
            "http://10.0.0.5/admin",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://172.16.0.9/",
            "http://172.31.255.1/",
            "http://db.internal/",
        ] {
            assert!(check_public_url(u).is_err(), "should refuse {u}");
        }
    }

    #[test]
    fn public_172_addresses_are_still_allowed() {
        // 172.15 and 172.32 are outside the private 172.16-31 block
        assert!(check_public_url("http://172.15.0.1/").is_ok());
        assert!(check_public_url("http://172.32.0.1/").is_ok());
    }

    #[test]
    fn refuses_non_http_schemes() {
        for u in ["file:///etc/passwd", "ftp://host/x", "javascript:alert(1)", ""] {
            assert!(check_public_url(u).is_err(), "should refuse {u}");
        }
    }

    #[test]
    fn credentials_in_url_do_not_smuggle_a_private_host() {
        assert!(check_public_url("http://user@127.0.0.1/").is_err());
        assert!(check_public_url("http://evil.com@localhost/").is_err());
    }

    #[test]
    fn html_becomes_readable_text() {
        let html = "<html><head><style>body{color:red}</style>\
                    <script>var x = 1 < 2;</script></head>\
                    <body><h1>Title</h1><p>Hello &amp; welcome</p></body></html>";
        let text = html_to_text(html);
        assert!(text.contains("Title"), "{text}");
        assert!(text.contains("Hello & welcome"), "{text}");
        assert!(!text.contains("color:red"), "style must be dropped: {text}");
        assert!(!text.contains("var x"), "script must be dropped: {text}");
        assert!(!text.contains('<'), "tags must be stripped: {text}");
    }
}
