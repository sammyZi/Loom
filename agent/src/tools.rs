use anyhow::Context as _;
use anyhow::Result;
use async_trait::async_trait;
use ide_core::{
    AgentEvent, CommandOutput, Permission, PermissionSet, ShellEvent, ShellRegistry, WorkspaceRoot,
};
use sandbox::Sandbox;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};
use std::collections::HashMap;
use std::net::ToSocketAddrs;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

/// Shared queue of pending approval requests. The tool registers a oneshot
/// under an id, announces it with AgentEvent::Ask, and the HTTP layer answers
/// via POST /agent/permission. Manual mode wires this in; auto modes run free.
#[derive(Clone, Default)]
pub struct PermGate {
    pending: Arc<std::sync::Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
}

impl PermGate {
    pub fn new() -> Self {
        Self::default()
    }

    fn ask(&self, id: String) -> tokio::sync::oneshot::Receiver<bool> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.lock().unwrap().insert(id, tx);
        rx
    }

    /// Answer a pending request. Unknown ids are ignored: the request may have
    /// been cancelled between the Ask event and the user's click.
    pub fn answer(&self, id: &str, allow: bool) {
        if let Some(tx) = self.pending.lock().unwrap().remove(id) {
            let _ = tx.send(allow);
        }
    }
}

pub struct ToolCtx {
    pub ws: WorkspaceRoot,
    pub sandbox: Arc<dyn Sandbox>,
    pub events: broadcast::Sender<AgentEvent>,
    /// Terminal output channel: the agent's shell commands stream live into
    /// the terminal panel's Agent tab under the id `"agent"`.
    pub shell_tx: broadcast::Sender<ShellEvent>,
    /// Registry of running commands, so background agent jobs are killable
    /// from the terminal panel and by the global stop button.
    pub shells: ShellRegistry,
    /// Present when every shell command needs explicit user approval first.
    pub perm: Option<PermGate>,
    /// What this agent may do. Consulted per call, so one tool can be free for
    /// `git status` and refused for `git push`.
    pub perms: PermissionSet,
    /// Present only for agents allowed to delegate. A subagent gets None, so a
    /// child cannot spawn children and recurse forever.
    pub spawn_subagent: Option<SubagentRunner>,
    /// path -> hash of what was last handed to the model. Shared across the
    /// whole run, including the planner/coder/reviewer roles, because each of
    /// them starts with a fresh context and would otherwise re-read the same
    /// files: one task re-read fifteen files three times over for ~30k tokens
    /// of pure duplication.
    pub reads: Arc<std::sync::Mutex<HashMap<String, u64>>>,
    /// Paths written this task, so "done" can be told from "changed nothing".
    pub writes: Arc<std::sync::Mutex<std::collections::HashSet<String>>>,
    /// Bytes of file content handed over this task. Telling the model to read
    /// selectively did not work — one run pulled thirteen whole files, 43 KB,
    /// before attempting any edit — so past this budget whole-file reads are
    /// refused and it has to search or ask for a range.
    pub read_budget: Arc<std::sync::atomic::AtomicUsize>,
    /// Content each written path had the first time this task touched it —
    /// `None` means it did not exist yet. Backs "undo this message".
    pub before: Arc<std::sync::Mutex<HashMap<String, Option<String>>>>,
}

/// Roughly 15k tokens of source. Enough to read a handful of files whole, not
/// enough to swallow a project.
pub const READ_BUDGET_BYTES: usize = 60_000;

fn content_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

impl ToolCtx {
    /// Record that a file was actually written. Also drops it from the read
    /// cache, so a later read returns the new contents rather than the
    /// "unchanged" note.
    pub fn note_write(&self, path: &str) {
        if let Ok(mut w) = self.writes.lock() {
            w.insert(path.to_string());
        }
        if let Ok(mut r) = self.reads.lock() {
            r.remove(path);
        }
    }

    /// Record what a path held right before its first write this task, so a
    /// later "undo this message" has something to restore. Only the first
    /// call for a given path counts — a second edit to the same file must
    /// still revert all the way back to how the task found it, not to the
    /// intermediate state the first edit left behind.
    pub fn note_before(&self, path: &str, existing: Option<&str>) {
        if let Ok(mut b) = self.before.lock() {
            b.entry(path.to_string()).or_insert_with(|| existing.map(str::to_string));
        }
    }

    /// Decide one call, and for `ask` block until the user answers. Returns the
    /// refusal text to hand back to the model, or None to go ahead.
    ///
    /// Handing a refusal back as a *result* rather than an error matters: the
    /// model reads it, adapts, and carries on, instead of the run dying.
    pub async fn gate(
        &self,
        tool: &str,
        detail: &str,
        cancel: &CancellationToken,
    ) -> Result<Option<String>> {
        match self.perms.decide(tool, detail) {
            Permission::Allow => Ok(None),
            Permission::Deny => Ok(Some(format!(
                "refused: `{tool}` is denied for this agent{}. Do not retry it; say what you \
                 needed it for and continue without it.",
                if detail.is_empty() { String::new() } else { format!(" on `{detail}`") }
            ))),
            Permission::Ask => {
                let Some(gate) = &self.perm else {
                    // No UI is listening, so an unanswerable prompt would hang
                    // the run forever. Treat it as allowed and move on.
                    return Ok(None);
                };
                let id = format!("perm-{}", uuid_like());
                let _ = self.events.send(AgentEvent::Ask {
                    id: id.clone(),
                    program: tool.to_string(),
                    args: detail.to_string(),
                });
                let rx = gate.ask(id);
                let allowed = tokio::select! {
                    r = rx => r.unwrap_or(false),
                    _ = cancel.cancelled() => anyhow::bail!("cancelled"),
                };
                Ok((!allowed).then(|| {
                    "user declined this. Do not retry it; adapt: describe what you needed and \
                     continue without it."
                        .to_string()
                }))
            }
        }
    }
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
                Box::new(SearchFiles),
                Box::new(WebSearch),
                Box::new(WebFetch),
                Box::new(ReadFile),
                Box::new(EditFile),
                Box::new(WriteFile),
                Box::new(RunCommand),
                Box::new(CheckCode),
                Box::new(RunTests),
                Box::new(TodoWrite),
                Box::new(AskUser),
                Box::new(Task),
                Box::new(LoadSkill),
                Box::new(OpenBrowser),
            ],
        }
    }

    /// Everything except shell access, for the mode where the user runs commands.
    pub fn no_shell() -> Self {
        Self {
            tools: vec![
                Box::new(ListFiles),
                Box::new(SearchFiles),
                Box::new(WebSearch),
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
                Box::new(SearchFiles),
                Box::new(WebSearch),
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

    /// Every tool the permissions still allow. This replaces the fixed
    /// `full`/`no_shell`/`read_only` trio: a denied tool is left out of the
    /// schema entirely, so the model never calls it and never has to explain
    /// that it cannot — which is exactly what made the old Approve mode read
    /// as a broken agent rather than a deliberate setting.
    pub fn for_permissions(perms: &PermissionSet) -> Self {
        Self {
            tools: Self::full()
                .tools
                .into_iter()
                .filter(|t| perms.offers(t.name()))
                .collect(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &dyn Tool> {
        self.tools.iter().map(|t| t.as_ref())
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.iter().find(|t| t.name() == name).map(|t| t.as_ref())
    }

    pub fn schemas(&self) -> Vec<Value> {
        self.schemas_with(&[])
    }

    /// Convenience for callers that have a workspace rather than a skill list.
    pub fn schemas_for(&self, ws: Option<&WorkspaceRoot>) -> Vec<Value> {
        let skills = ws.map(crate::skills::discover).unwrap_or_default();
        self.schemas_with(&skills)
    }

    /// Schemas, with the workspace's skills listed inside the `skill` tool's
    /// description. That listing is the *only* thing the model sees about a
    /// skill until it asks for one — which is what keeps a shelf of playbooks
    /// costing a line each instead of a document each.
    ///
    /// With no skills installed the tool is dropped entirely, so an empty shelf
    /// costs nothing and the model is never tempted to call it.
    pub fn schemas_with(&self, skills: &[crate::skills::Skill]) -> Vec<Value> {
        self.schemas_with_loaded(skills, &[])
    }

    /// `loaded` names skills already pasted into the system prompt; they are
    /// dropped from the catalogue so the model is not invited to fetch what it
    /// already has.
    pub fn schemas_with_loaded(
        &self,
        skills: &[crate::skills::Skill],
        loaded: &[String],
    ) -> Vec<Value> {
        let catalogue = crate::skills::catalogue_excluding(skills, loaded);
        self.tools
            .iter()
            // Offered only when something is left to fetch. With every skill
            // preloaded the catalogue is empty, and a tool advertising nothing
            // is an invitation to waste a call.
            .filter(|t| t.name() != "skill" || !catalogue.is_empty())
            .map(|t| {
                let mut description = t.description().to_string();
                if t.name() == "skill" {
                    description.push_str(&catalogue);
                }
                json!({
                    "name": t.name(),
                    "description": description,
                    "input_schema": t.input_schema(),
                })
            })
            .collect()
    }
}

struct ListFiles;
struct SearchFiles;
struct WebSearch;
struct WebFetch;
struct ReadFile;
struct EditFile;
struct RunCommand;
struct CheckCode;
struct RunTests;
struct WriteFile;
struct TodoWrite;
struct AskUser;
struct Task;
struct LoadSkill;
struct OpenBrowser;

/// Runs one subagent to completion and returns its final message. Boxed as a
/// callback because the orchestrator owns the pipeline; the tools crate cannot
/// depend on it without a cycle.
pub type SubagentRunner = Arc<
    dyn Fn(
            &'static crate::agents::AgentDef,
            String,
            CancellationToken,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send>>
        + Send
        + Sync,
>;

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
const MAX_REDIRECTS: usize = 5;

#[async_trait]
impl Tool for WebFetch {
    fn name(&self) -> &'static str {
        "web_fetch"
    }
    fn description(&self) -> &'static str {
        "Fetch a public http(s) URL over the internet and return its text, with HTML reduced \
         to readable content. Use for documentation, changelogs, API references and error pages."
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

        // Redirects are followed by hand so every hop passes the same public-host
        // check. A naive client would happily follow a public URL that 302s to
        // http://127.0.0.1:8080 — i.e. straight into our own API.
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Mozilla/5.0 (compatible; LoomAgent/0.2)")
            .build()?;
        let mut target = reqwest::Url::parse(&url)?;
        let mut res = None;
        for _ in 0..=MAX_REDIRECTS {
            assert_public_target(&target).await?;
            let r = client.get(target.clone()).send().await?;
            if r.status().is_redirection() {
                let loc = r
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| anyhow::anyhow!("redirect without a Location header"))?
                    .to_string();
                target = target.join(&loc)?;
                continue;
            }
            res = Some(r);
            break;
        }
        let Some(res) = res else {
            anyhow::bail!("too many redirects (>{MAX_REDIRECTS})");
        };
        let status = res.status();
        let final_url = res.url().to_string();
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
        Ok(format!("{status} {ctype}\n{final_url}\n\n{text}"))
    }
}

/// Lexical + DNS-level gate applied to every URL we are about to touch,
/// including every redirect hop.
async fn assert_public_target(u: &reqwest::Url) -> Result<()> {
    let (host, port) = url_host_port(u)?;
    check_public_host(&host)?;
    resolve_public(&host, port).await
}

fn url_host_port(u: &reqwest::Url) -> Result<(String, u16)> {
    match u.scheme() {
        "http" | "https" => {}
        other => anyhow::bail!("only http:// and https:// URLs are allowed, got {other}:"),
    }
    let host = u
        .host_str()
        .unwrap_or("")
        .trim_matches(['[', ']'])
        .to_ascii_lowercase();
    if host.is_empty() {
        anyhow::bail!("URL has no host");
    }
    Ok((host, u.port_or_known_default().unwrap_or(80)))
}

/// Cheap string-level rejects before any DNS traffic leaves the machine.
fn check_public_host(host: &str) -> Result<()> {
    let blocked = host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".internal")
        || host.ends_with(".local")
        || host == "::1"
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || host.starts_with("169.254.")
        || host.starts_with("fd")
        || host.starts_with("fe80:");
    if blocked {
        anyhow::bail!("refusing to fetch a local or private address: {host}");
    }
    Ok(())
}

/// The agent runs beside a local API and on the user's LAN, so every resolved
/// address must be globally reachable. This catches hostnames that merely look
/// public but point at loopback or private space (DNS rebinding).
async fn resolve_public(host: &str, port: u16) -> Result<()> {
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if !is_public_ip(ip) {
            anyhow::bail!("refusing to fetch a non-public address: {ip}");
        }
        return Ok(());
    }
    let owned = host.to_string();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let addrs = (owned.as_str(), port)
            .to_socket_addrs()
            .with_context(|| format!("resolve {owned}"))?;
        for a in addrs {
            if !is_public_ip(a.ip()) {
                anyhow::bail!("{owned} resolves to a non-public address ({})", a.ip());
            }
        }
        Ok(())
    })
    .await
    .context("resolve join")?
}

/// Everything that should never be reached through a model-supplied URL:
/// loopback, RFC1918, CGNAT (Tailscale), link-local/metadata, multicast,
/// documentation ranges, broadcast, and their IPv6 equivalents including
/// IPv4-mapped addresses like [::ffff:127.0.0.1].
fn is_public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let [a, b, _, _] = v4.octets();
            !(a == 0
                || a == 10
                || a == 127
                || (a == 100 && (64..=127).contains(&b)) // CGNAT 100.64/10
                || (a == 169 && b == 254) // link-local / cloud metadata
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && b == 168)
                || (a == 192 && b == 0) // protocol assignments + TEST-NET-1
                || (a == 198 && (b == 18 || b == 19 || b == 51)) // benchmarks, TEST-NET-2
                || (a == 203 && b == 0) // TEST-NET-3
                || a >= 224) // multicast + reserved + broadcast
        }
        std::net::IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_ip(std::net::IpAddr::V4(mapped));
            }
            let seg = v6.segments();
            !(v6.is_loopback()
                || v6.is_unspecified()
                || (seg[0] & 0xfe00) == 0xfc00 // unique local fc00::/7
                || (seg[0] & 0xffc0) == 0xfe80 // link local fe80::/10
                || (seg[0] & 0xff00) == 0xff00 // multicast ff00::/8
                || (seg[0] == 0x2001 && seg[1] == 0x0db8) // documentation
                || (seg[0] == 0x0100 && seg[1] == 0 && seg[2] == 0 && seg[3] == 0)) // discard-only
        }
    }
}

/// Crude but dependency-free: drop script/style, strip tags, decode the few
/// entities that actually matter, and collapse whitespace.
fn html_to_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    // ASCII-only lowering keeps byte offsets identical between both strings,
    // so `i` indexes them interchangeably.
    let lower = html.to_ascii_lowercase();
    let mut i = 0usize;
    let mut in_tag = false;

    while i < html.len() {
        if !in_tag && lower[i..].starts_with("<script") {
            i = skip_block(&lower, i, "</script>");
            continue;
        }
        if !in_tag && lower[i..].starts_with("<style") {
            i = skip_block(&lower, i, "</style>");
            continue;
        }
        let c = html[i..].chars().next().unwrap_or('\u{fffd}');
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            c if !in_tag => out.push(c),
            _ => {}
        }
        i += c.len_utf8();
    }

    let out = decode_entities(&out);

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

/// Decode the HTML entities that actually show up in scraped text: the common
/// named ones plus decimal/hex numeric forms like `&#39;` and `&#x27;`.
/// Unknown entities pass through untouched.
fn decode_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut i = 0usize;
    while i < input.len() {
        if input.as_bytes()[i] == b'&' {
            if let Some(semi) = input[i..].find(';') {
                let ent = &input[i + 1..i + semi];
                let decoded = if let Some(hex) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                    u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
                } else if let Some(dec) = ent.strip_prefix('#') {
                    dec.parse::<u32>().ok().and_then(char::from_u32)
                } else {
                    match ent {
                        "amp" => Some('&'),
                        "lt" => Some('<'),
                        "gt" => Some('>'),
                        "quot" => Some('"'),
                        "apos" => Some('\''),
                        "nbsp" => Some(' '),
                        _ => None,
                    }
                };
                if let Some(c) = decoded {
                    out.push(c);
                    i += semi + 1;
                    continue;
                }
            }
        }
        let c = input[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(c);
        i += c.len_utf8();
    }
    out
}

const SEARCH_RESULTS: usize = 8;

/// Internet search without an API key: DuckDuckGo's classic HTML endpoint,
/// parsed with plain string scanning.
#[async_trait]
impl Tool for WebSearch {
    fn name(&self) -> &'static str {
        "web_search"
    }
    fn description(&self) -> &'static str {
        "Search the public internet and return the top results with titles, URLs and snippets. \
         Use it for current information, library docs you do not know, or unfamiliar error \
         messages — then web_fetch the most promising link for details."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Search keywords" }
            },
            "required": ["query"]
        })
    }
    async fn call(&self, _ctx: &ToolCtx, input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let q = input["query"].as_str().unwrap_or("").trim();
        if q.is_empty() {
            anyhow::bail!("query required");
        }
        let target = reqwest::Url::parse_with_params("https://html.duckduckgo.com/html/", &[("q", q)])?;
        assert_public_target(&target).await?;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("Mozilla/5.0 (compatible; LoomAgent/0.2)")
            .build()?;
        let res = client.get(target).send().await?;
        let body = res.text().await.unwrap_or_default();

        let titles = extract_anchors(&body, "result__a");
        let snippets = extract_anchors(&body, "result__snippet");
        if titles.is_empty() {
            anyhow::bail!("search returned no parseable results (endpoint may be rate-limiting)");
        }
        let mut out = String::new();
        let mut shown = 0;
        for (i, (_href, title)) in titles.into_iter().enumerate() {
            if shown >= SEARCH_RESULTS {
                break;
            }
            let Some(snippet) = snippets
                .get(i)
                .map(|(_, s)| s.split_whitespace().collect::<Vec<_>>().join(" "))
            else {
                continue;
            };
            out.push_str(&format!(
                "{}. {}\n   {}\n",
                shown + 1,
                collapse_ws(&title),
                snippet
            ));
            shown += 1;
        }
        Ok(out)
    }
}

/// Pull `(href, inner_text)` for every `<a …class="…MARKER…"…>text</a>`.
fn extract_anchors(html: &str, marker: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let lower = html.to_ascii_lowercase();
    let mut from = 0usize;
    while let Some(hit) = lower[from..].find(marker) {
        let tag_start = match lower[..from + hit].rfind('<') {
            Some(p) => p,
            None => break,
        };
        let Some(anchor_end) = lower[tag_start..].find("</a>") else {
            break;
        };
        let block = &html[tag_start..tag_start + anchor_end];
        let href = block
            .find("href=\"")
            .and_then(|p| {
                let rest = &block[p + 6..];
                rest.find('"').map(|e| rest[..e].to_string())
            })
            .unwrap_or_default();
        let inner_start = block.find('>').map(|p| p + 1).unwrap_or(0);
        let inner_end = block.rfind("</a>").unwrap_or(block.len());
        let text = html_to_text(block.get(inner_start..inner_end).unwrap_or(""));
        out.push((real_url(&href), text));
        from = tag_start + anchor_end + 4;
    }
    out
}

/// DuckDuckGo wraps result URLs in its own redirector (`…/l/?uddg=<encoded>`).
fn real_url(href: &str) -> String {
    if let Some(pos) = href.find("uddg=") {
        let enc = &href[pos + 5..];
        let end = enc.find('&').unwrap_or(enc.len());
        return percent_decode(&enc[..end]);
    }
    if href.starts_with("//") {
        format!("https:{href}")
    } else {
        href.to_string()
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => {
                let hi = (b[i + 1] as char).to_digit(16);
                let lo = (b[i + 2] as char).to_digit(16);
                if let (Some(h), Some(l)) = (hi, lo) {
                    out.push(((h << 4) | l) as u8);
                    i += 3;
                } else {
                    out.push(b[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

const SEARCH_MATCH_CAP: usize = 400;
const SEARCH_OUT_CAP: usize = 24_000;
const SEARCH_FILE_CAP: u64 = 512_000;

/// Grep-style content search over the workspace. Substring matching rather than
/// regex keeps this dependency-free, and agents can narrow with more terms.
#[async_trait]
impl Tool for SearchFiles {
    fn name(&self) -> &'static str {
        "search_files"
    }
    fn description(&self) -> &'static str {
        "Search the contents of every workspace file for a text substring and get back \
         path:line hits. Case-insensitive unless asked otherwise. Much cheaper than reading \
         files one by one when hunting for where something is defined or used."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Text to look for" },
                "ext": {
                    "type": "string",
                    "description": "Comma-separated extensions to restrict to, e.g. \"rs\" or \"ts,tsx\". Omit for all files."
                },
                "case_sensitive": { "type": "boolean", "description": "Match case exactly (default false)" }
            },
            "required": ["query"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let query = input["query"].as_str().unwrap_or("").trim().to_string();
        if query.is_empty() {
            anyhow::bail!("query required");
        }
        let exts: Option<Vec<String>> = input["ext"].as_str().map(|s| {
            s.split(',')
                .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
                .filter(|e| !e.is_empty())
                .collect()
        });
        let ci = !input["case_sensitive"].as_bool().unwrap_or(false);
        let root = ctx.ws.root().to_path_buf();
        let cancel = cancel.clone();

        tokio::task::spawn_blocking(move || -> Result<String> {
            let files = crate::project::list_files(&root, None, 4_000);
            let needle = if ci { query.to_lowercase() } else { query.clone() };
            let mut out = String::new();
            let mut total = 0usize;
            let mut files_hit = 0usize;
            for f in files {
                if total >= SEARCH_MATCH_CAP || out.len() >= SEARCH_OUT_CAP {
                    break;
                }
                if f.ends_with('/') {
                    continue;
                }
                if let Some(exts) = &exts {
                    match std::path::Path::new(&f).extension().and_then(|e| e.to_str()) {
                        Some(e) if exts.contains(&e.to_ascii_lowercase()) => {}
                        _ => continue,
                    }
                }
                let path = root.join(&f);
                let Ok(meta) = std::fs::metadata(&path) else { continue };
                if meta.len() > SEARCH_FILE_CAP {
                    continue;
                }
                let Ok(content) = std::fs::read_to_string(&path) else {
                    continue; // binary or non-UTF-8
                };
                let hay = if ci { content.to_lowercase() } else { content.clone() };
                let mut per_file = 0usize;
                for (line_no, (raw, low)) in content.lines().zip(hay.lines()).enumerate() {
                    if !low.contains(&needle) {
                        continue;
                    }
                    out.push_str(&format!("{}:{}: {}\n", f, line_no + 1, raw.trim()));
                    total += 1;
                    per_file += 1;
                    if per_file == 25 {
                        out.push_str(&format!("{f}: 25+ matches, further hits suppressed\n"));
                        break;
                    }
                    if total >= SEARCH_MATCH_CAP || out.len() >= SEARCH_OUT_CAP {
                        break;
                    }
                }
                if per_file > 0 {
                    files_hit += 1;
                    if cancel.is_cancelled() {
                        anyhow::bail!("cancelled");
                    }
                }
            }
            if total == 0 {
                return Ok(format!("no matches for {query:?}"));
            }
            out.push_str(&format!(
                "\n{total} match(es) across {files_hit} file(s)"
            ));
            if total >= SEARCH_MATCH_CAP {
                out.push_str(" (capped — refine the query)");
            }
            Ok(out)
        })
        .await
        .context("search join")?
    }
}

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &'static str {
        "read_file"
    }
    fn description(&self) -> &'static str {
        "Read a text file in the opened workspace. Path is relative to the workspace root. \
         Give offset and limit to read one slice of a large file instead of all of it."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Workspace-relative path" },
                "offset": { "type": "integer", "description": "First line to return, 1-based" },
                "limit": { "type": "integer", "description": "How many lines to return" }
            },
            "required": ["path"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, _cancel: &CancellationToken) -> Result<String> {
        let path = input["path"].as_str().unwrap_or("");
        let raw = viewer::read_file(&ctx.ws, path)?;
        let offset = input["offset"].as_u64();
        let limit = input["limit"].as_u64();
        if offset.is_some() || limit.is_some() {
            // An explicit slice is always a fresh request; the model is asking
            // for a specific window, not the file it already has.
            return Ok(slice_lines(&raw, offset, limit));
        }
        let hash = content_hash(&raw);
        let seen = {
            let mut reads = ctx.reads.lock().unwrap();
            reads.insert(path.to_string(), hash) == Some(hash)
        };
        tracing::info!(
            "read_file {path}: {} ({} bytes)",
            if seen { "CACHED" } else { "full" },
            raw.len()
        );
        if seen {
            return Ok(format!(
                "{path} is unchanged since it was read earlier in this task ({} lines) — its \
                 contents are already above. Do not read it again; use search_files to find a \
                 detail, or read_file with offset/limit for one region.",
                raw.lines().count()
            ));
        }
        // Cached repeats are free; only new content spends the budget. Checked
        // before adding, so the read that would *cross* the line is the one
        // refused — charging first let one more whole file through.
        use std::sync::atomic::Ordering::Relaxed;
        let spent = ctx.read_budget.load(Relaxed);
        if spent + raw.len() > READ_BUDGET_BYTES {
            return Ok(format!(
                "refused: this task has already read {} KB of source and {path} would take it \
                 past the whole-file budget ({} lines). Use search_files to locate what you \
                 need, then read_file with offset and limit for that region — or edit what you \
                 have already read.",
                spent / 1000,
                raw.lines().count()
            ));
        }
        ctx.read_budget.fetch_add(raw.len(), Relaxed);
        Ok(crate::context::clip_file(path, &raw))
    }
}

/// A 1-based line window, numbered so the model can cite what it read and ask
/// for the next slice. Reading a 5000-line file whole just to see one function
/// is what burns a context window.
pub fn slice_lines(text: &str, offset: Option<u64>, limit: Option<u64>) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = offset.unwrap_or(1).max(1) as usize - 1;
    if start >= lines.len() {
        return format!("(file has {} lines; offset {} is past the end)", lines.len(), start + 1);
    }
    let take = limit.unwrap_or(2000).max(1) as usize;
    let end = (start + take).min(lines.len());
    let mut out = String::new();
    for (i, line) in lines[start..end].iter().enumerate() {
        out.push_str(&format!("{:>6}\t{}\n", start + i + 1, line));
    }
    if end < lines.len() {
        out.push_str(&format!("… {} more lines (next offset {})\n", lines.len() - end, end + 1));
    }
    out
}

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &'static str {
        "write_file"
    }
    fn description(&self) -> &'static str {
        "Create a file, or replace one whole. Use edit_file to change part of an existing \
         file — this overwrites everything and is for new files or full rewrites."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Workspace-relative path" },
                "content": { "type": "string" }
            },
            "required": ["path", "content"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, _cancel: &CancellationToken) -> Result<String> {
        let path = input["path"].as_str().unwrap_or("").trim();
        if path.is_empty() {
            anyhow::bail!("path required");
        }
        let content = input["content"].as_str().unwrap_or("");
        let before = viewer::read_file(&ctx.ws, path).ok();
        ctx.note_before(path, before.as_deref());
        let existed = before.is_some();
        viewer::write_file(&ctx.ws, path, content)?;
        ctx.note_write(path);
        Ok(format!(
            "{} {path} ({} lines)",
            if existed { "overwrote" } else { "created" },
            content.lines().count()
        ))
    }
}

#[async_trait]
impl Tool for TodoWrite {
    fn name(&self) -> &'static str {
        "todo_write"
    }
    fn description(&self) -> &'static str {
        "Record the task list for a multi-step job and keep it current. Call it once when you \
         start with every step pending, then again after each step to mark it done. Skip it \
         for anything that is one or two steps."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "The whole list every time, not just the changes.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "text": { "type": "string" },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "running", "done"]
                            }
                        },
                        "required": ["text", "status"]
                    }
                }
            },
            "required": ["todos"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, _cancel: &CancellationToken) -> Result<String> {
        let todos = input["todos"].as_array().cloned().unwrap_or_default();
        let items: Vec<(String, String)> = todos
            .iter()
            .filter_map(|t| {
                Some((
                    t["text"].as_str()?.to_string(),
                    t["status"].as_str().unwrap_or("pending").to_string(),
                ))
            })
            .collect();
        // Surfaced as an event so the feed can render progress; the model gets
        // the same list back so it stays anchored to it.
        let _ = ctx.events.send(AgentEvent::Todos {
            items: items
                .iter()
                .map(|(text, status)| ide_core::TodoItem {
                    text: text.clone(),
                    status: status.clone(),
                })
                .collect(),
        });
        Ok(render_todos(&items))
    }
}

/// The list as the model should see it back: compact, and unambiguous about
/// what is left. Returned rather than a bare "ok" so the plan survives a
/// context compaction — the tool result carries it.
pub fn render_todos(items: &[(String, String)]) -> String {
    if items.is_empty() {
        return "todo list cleared".into();
    }
    let done = items.iter().filter(|(_, s)| s == "done").count();
    let mut out = format!("{done}/{} done\n", items.len());
    for (text, status) in items {
        let mark = match status.as_str() {
            "done" => "x",
            "running" => ">",
            _ => " ",
        };
        out.push_str(&format!("[{mark}] {text}\n"));
    }
    out
}

#[async_trait]
impl Tool for AskUser {
    fn name(&self) -> &'static str {
        "ask_user"
    }
    fn description(&self) -> &'static str {
        "Ask the user one question when the task genuinely cannot proceed without their \
         answer — a missing credential, or a choice only they can make. Do not use it for \
         anything you could decide yourself or find in the repo."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": { "type": "string" }
            },
            "required": ["question"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, cancel: &CancellationToken) -> Result<String> {
        let question = input["question"].as_str().unwrap_or("").trim().to_string();
        if question.is_empty() {
            anyhow::bail!("question required");
        }
        let Some(gate) = &ctx.perm else {
            // Nothing is listening, so waiting would hang the run forever.
            return Ok("no one is available to answer; decide it yourself and say what you \
                       assumed."
                .into());
        };
        let id = format!("ask-{}", uuid_like());
        let _ = ctx.events.send(AgentEvent::Ask {
            id: id.clone(),
            program: "question".into(),
            args: question.clone(),
        });
        let rx = gate.ask(id);
        let yes = tokio::select! {
            r = rx => r.unwrap_or(false),
            _ = cancel.cancelled() => anyhow::bail!("cancelled"),
        };
        Ok(if yes { "user answered yes".into() } else { "user answered no".into() })
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
        let existing = viewer::read_file(&ctx.ws, path).ok();
        ctx.note_before(path, existing.as_deref());
        if old.is_empty() {
            // Empty old_text means "create". Overwriting a real file wholesale
            // used to pass silently — silent data loss driven by model drift.
            match existing.as_deref() {
                Some(prev) if !prev.trim().is_empty() => anyhow::bail!(
                    "{path} already exists with content; to change it send the exact \
                     text to replace in old_text, or read it first"
                ),
                _ => {}
            }
        }
        let before = if old.is_empty() {
            String::new()
        } else {
            existing.unwrap_or_default()
        };
        let after = if old.is_empty() {
            new.to_string()
        } else {
            if !before.contains(old) {
                anyhow::bail!("old_text not found in {path}");
            }
            before.replacen(old, new, 1)
        };
        if before == after {
            return Ok("no changes".into());
        }
        viewer::write_file(&ctx.ws, path, &after)?;
        ctx.note_write(path);
        let diff = unified_diff(path, &before, &after);
        let _ = ctx.events.send(AgentEvent::Diff {
            path: path.to_string(),
            diff: diff.clone(),
        });
        Ok(diff)
    }
}

/// Foreground cap for the agent's own commands. Was 120s and killed ordinary
/// `npm install` / first-time dependency fetches outright, well short of done.
/// A terminal a person is watching has no such cap (see routes.rs); this one
/// exists only so a truly hung foreground call cannot block the run forever.
const FOREGROUND_TIMEOUT: Duration = Duration::from_secs(600);

#[async_trait]
impl Tool for RunCommand {
    fn name(&self) -> &'static str {
        "run_command"
    }
    fn description(&self) -> &'static str {
        "Run a program in the workspace sandbox. Output streams live into the terminal panel's \
         Agent tab while it runs. Give program and args separately, not a shell line. Foreground \
         calls wait for completion (10 minute cap, process tree killed on return) and suit \
         installs, builds, tests and checks. For a dev server or watcher set background:true: the \
         call returns immediately and the process keeps running until stopped from the terminal \
         panel."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "program": { "type": "string", "description": "Executable name, e.g. npm or git" },
                "args": { "type": "array", "items": { "type": "string" } },
                "background": {
                    "type": "boolean",
                    "description": "Keep it running after returning (dev servers, watchers). Default false."
                }
            },
            "required": ["program"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let program = input["program"].as_str().unwrap_or("").trim().to_string();
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
        let background = input["background"].as_bool().unwrap_or(false);

        // A server started in the foreground is doomed: run_foreground caps at
        // FOREGROUND_TIMEOUT and kills the tree on return, so `npm run dev` came
        // up and died the moment the tool replied. Asking for background:true in
        // the prompt was not enough, so the wrong call is refused with the fix
        // in hand.
        if !background && looks_long_running(&program, &args) {
            return Ok(format!(
                "refused: `{program} {}` looks like a server or watcher, and a foreground run is \
                 killed when this call returns ({}s cap). Call run_command again with \
                 background:true — it will keep running, get its own terminal tab, and print the \
                 port it bound to.",
                args.join(" "),
                FOREGROUND_TIMEOUT.as_secs(),
            ));
        }

        // Manual mode: every shell command needs an explicit yes, the way
        // coding agents ask before touching a terminal.
        if let Some(gate) = &ctx.perm {
            let id = format!("perm-{}", uuid_like());
            let _ = ctx.events.send(AgentEvent::Ask {
                id: id.clone(),
                program: program.to_string(),
                args: args.join(" "),
            });
            let rx = gate.ask(id);
            let allowed = tokio::select! {
                r = rx => r.unwrap_or(false),
                _ = cancel.cancelled() => anyhow::bail!("cancelled"),
            };
            if !allowed {
                return Ok("user declined to run this command. Do not retry it; adapt: \
                           describe what you needed and continue without it."
                    .into());
            }
        }

        if background {
            self.spawn_background(ctx, &program, &args, cancel).await
        } else {
            self.run_foreground(ctx, &program, &args, cancel).await
        }
    }
}

/// The id every agent shell chunk streams under; the terminal panel maps it to
/// its read-only Agent tab.
pub const AGENT_STREAM_ID: &str = "agent";

/// Loopback only. `web_fetch` refuses every private address to stop the agent
/// being talked into probing the LAN, but the one thing it therefore could not
/// do was check the dev server it had just started. This opens exactly that
/// hole and no more: 127.0.0.0/8, ::1 and `localhost`, nothing else private.
fn is_loopback_host(host: &str) -> bool {
    let h = host.trim_matches(['[', ']']).to_ascii_lowercase();
    h == "localhost" || h.ends_with(".localhost") || h == "::1" || h.starts_with("127.")
}

#[async_trait]
impl Tool for OpenBrowser {
    fn name(&self) -> &'static str {
        "browser_open"
    }
    fn description(&self) -> &'static str {
        "Open a URL in the app's Browser panel so the user can see it, and report back the \
         status code and page title. Use it to check a dev server you started actually serves \
         (http://localhost:PORT — read the real port from the server's own output) and to show \
         the user the running result. Localhost and public http(s) URLs are both allowed."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "http:// or https:// URL" }
            },
            "required": ["url"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let raw = input["url"].as_str().unwrap_or("").trim().to_string();
        let url = reqwest::Url::parse(&raw).with_context(|| format!("bad URL `{raw}`"))?;
        let (host, _) = url_host_port(&url)?;
        if !is_loopback_host(&host) {
            // Anything not loopback goes through the same guard web_fetch uses.
            assert_public_target(&url).await?;
        }

        // Show it first: even a failing page is worth putting in front of the
        // user, and the panel is how they see what the agent built.
        let _ = ctx.events.send(AgentEvent::Browse { url: url.to_string() });

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()?;
        let res = match client.get(url.clone()).send().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(format!(
                    "opened {url} in the Browser panel, but the request failed: {e}. If this is \
                     a dev server, check it is still running and that the port matches the one \
                     it printed."
                ))
            }
        };
        let status = res.status();
        let body = res.text().await.unwrap_or_default();
        let title = page_title(&body).unwrap_or_else(|| "(no <title>)".into());
        Ok(format!(
            "{url} → {} {}\ntitle: {title}\nOpened in the Browser panel.",
            status.as_u16(),
            status.canonical_reason().unwrap_or(""),
        ))
    }
}

/// First `<title>` in the document, whitespace-collapsed.
pub fn page_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let gt = lower[open..].find('>')? + open + 1;
    let close = lower[gt..].find("</title>")? + gt;
    let raw = html[gt..close].trim();
    if raw.is_empty() {
        return None;
    }
    Some(raw.split_whitespace().collect::<Vec<_>>().join(" "))
}

#[async_trait]
impl Tool for LoadSkill {
    fn name(&self) -> &'static str {
        "skill"
    }
    fn description(&self) -> &'static str {
        // The catalogue cannot go here — descriptions are &'static and skills
        // are per-workspace — so `describe_for` appends it at schema time.
        "Load a skill: a written playbook for a particular kind of work, kept out of your \
         context until you ask for it. Call it before starting a task one of them covers, and \
         follow what it says in place of your default approach."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": { "type": "string", "description": "Skill name, exactly as listed" }
            },
            "required": ["name"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, _cancel: &CancellationToken) -> Result<String> {
        let want = input["name"].as_str().unwrap_or("").trim().to_ascii_lowercase();
        let found = crate::skills::discover(&ctx.ws);
        let Some(skill) = found.iter().find(|s| s.name == want) else {
            let names: Vec<&str> = found.iter().map(|s| s.name.as_str()).collect();
            return Ok(if names.is_empty() {
                "no skills are installed in this workspace".into()
            } else {
                format!("no skill named `{want}`. Available: {}", names.join(", "))
            });
        };
        crate::skills::load_body(skill)
    }
}

#[async_trait]
impl Tool for Task {
    fn name(&self) -> &'static str {
        "task"
    }
    fn description(&self) -> &'static str {
        "Hand one self-contained job to a subagent, which works in its own context and returns \
         only its final answer. Use `explore` to find where something lives in this repo, \
         `scout` to research external docs or a dependency, and `general` for a multi-step job \
         you want kept out of your own context. Give it everything it needs in one prompt — it \
         cannot see this conversation."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "agent": {
                    "type": "string",
                    "enum": ["explore", "scout", "general"],
                    "description": "Which subagent to run"
                },
                "prompt": {
                    "type": "string",
                    "description": "The whole task, self-contained."
                }
            },
            "required": ["agent", "prompt"]
        })
    }
    async fn call(&self, ctx: &ToolCtx, input: Value, cancel: &CancellationToken) -> Result<String> {
        if cancel.is_cancelled() {
            anyhow::bail!("cancelled");
        }
        let id = input["agent"].as_str().unwrap_or("").trim().to_ascii_lowercase();
        let prompt = input["prompt"].as_str().unwrap_or("").trim().to_string();
        if prompt.is_empty() {
            anyhow::bail!("prompt required");
        }
        let Some(def) = crate::agents::agent_def(&id) else {
            anyhow::bail!("unknown agent `{id}`");
        };
        if !matches!(def.mode, crate::agents::AgentMode::Subagent | crate::agents::AgentMode::All) {
            anyhow::bail!("`{id}` is a primary agent and cannot be delegated to");
        }
        // The parent's own permissions still bound the child: a plan-mode agent
        // must not be able to reach the shell by delegating to `general`.
        if ctx.perms.decide("task", &id) == Permission::Deny {
            return Ok(format!("refused: delegating to `{id}` is denied for this agent."));
        }
        let _ = ctx.events.send(AgentEvent::Status {
            message: format!("{} subagent", def.label),
        });
        let runner = ctx
            .spawn_subagent
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("subagents are not available in this run"))?;
        let out = runner(def, prompt, cancel.clone()).await?;
        // Only the final answer comes back; the child's tool traffic stays in
        // its own context, which is the whole point of delegating.
        Ok(out)
    }
}

/// The thing a permission rule matches against: the command line for shell
/// calls, the path for file calls, the URL for web calls. Without this, rules
/// could only say yes or no to a whole tool.
pub fn call_subject(tool: &str, input: &Value) -> String {
    match tool {
        "run_command" => {
            let program = input["program"].as_str().unwrap_or("");
            let args: Vec<&str> = input["args"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            if args.is_empty() {
                program.to_string()
            } else {
                format!("{program} {}", args.join(" "))
            }
        }
        "read_file" | "edit_file" | "write_file" | "list_files" => {
            input["path"].as_str().unwrap_or("").to_string()
        }
        "web_fetch" => input["url"].as_str().unwrap_or("").to_string(),
        "web_search" => input["query"].as_str().unwrap_or("").to_string(),
        "search_files" => input["query"].as_str().unwrap_or("").to_string(),
        _ => String::new(),
    }
}

/// Commands that serve or watch rather than finish. Deliberately conservative:
/// a false positive only costs the model one retry with background:true, while
/// a false negative silently kills the user's dev server.
pub fn looks_long_running(program: &str, args: &[String]) -> bool {
    const SERVER_EXES: &[&str] = &[
        "next", "vite", "nodemon", "webpack-dev-server", "http-server", "serve", "live-server",
    ];
    const SERVER_WORDS: &[&str] = &["dev", "serve", "start", "watch", "preview"];

    let exe = std::path::Path::new(program)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(program)
        .to_ascii_lowercase();
    if SERVER_EXES.contains(&exe.as_str()) {
        return true;
    }
    // `npm run dev`, `pnpm dev`, `yarn start`, `bun run serve`…
    let runner = matches!(exe.as_str(), "npm" | "pnpm" | "yarn" | "bun" | "npx" | "deno");
    if !runner {
        return false;
    }
    args.iter().any(|a| {
        let a = a.trim().to_ascii_lowercase();
        // A runner can also name the server binary outright: `npx vite`.
        SERVER_WORDS.contains(&a.as_str()) || SERVER_EXES.contains(&a.as_str())
    })
}

fn echo_line(ws: &WorkspaceRoot, program: &str, args: &[String]) -> String {
    format!("{}> {program} {}\n", ws.root().display(), args.join(" "))
}

impl RunCommand {
    /// Stream output live into the Agent tab, wait for exit, hand the full
    /// text back to the model.
    async fn run_foreground(
        &self,
        ctx: &ToolCtx,
        program: &str,
        args: &[String],
        cancel: &CancellationToken,
    ) -> Result<String> {
        send_chunk(ctx, echo_line(&ctx.ws, program, args).to_string());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let fwd_tx = ctx.shell_tx.clone();
        let forwarder = tokio::spawn(async move {
            while let Some(text) = rx.recv().await {
                let _ = fwd_tx.send(ShellEvent::Chunk { id: AGENT_STREAM_ID.into(), text });
            }
        });
        let out = ctx
            .sandbox
            .run_streaming(&ctx.ws, program, args, FOREGROUND_TIMEOUT, cancel, Some(tx), None)
            .await;
        drop_forwarder(forwarder).await;
        match out {
            Ok(o) => {
                send_chunk(ctx, format!("\n[exited code {}]\n", o.exit_code));
                format_cmd(o)
            }
            Err(e) => {
                send_chunk(ctx, format!("\n[failed] {e}\n"));
                Err(e)
            }
        }
    }

    /// Fire-and-forget for dev servers and watchers: registers under an id in
    /// the shared registry (killable from the terminal panel or by Stop), then
    /// returns at once so the model does not block on a server that never exits.
    async fn spawn_background(
        &self,
        ctx: &ToolCtx,
        program: &str,
        args: &[String],
        run_cancel: &CancellationToken,
    ) -> Result<String> {
        let job_id = format!("agent-bg-{}", uuid_like());
        let child = CancellationToken::new();
        // Two ways to die: the user kills this specific job, or the whole run
        // is stopped — both funnel into the child token the sandbox watches.
        let watcher = {
            let child = child.clone();
            let run_cancel = run_cancel.clone();
            tokio::spawn(async move {
                let _ = run_cancel.cancelled().await;
                child.cancel();
            })
        };
        // No stdin for agent jobs: nothing is watching to answer a prompt, so a
        // closed pipe (EOF) is better than blocking until the timeout.
        let Some(gen) = ctx.shells.begin(&job_id, &child, None) else {
            anyhow::bail!("too many background jobs are already running");
        };
        // Its own tab, not the shared Agent one: a dev server keeps printing for
        // as long as it lives, and every later command would be lost in it.
        let label = format!("{program} {}", args.join(" "));
        let _ = ctx.shell_tx.send(ShellEvent::Opened {
            id: job_id.clone(),
            label: label.trim().chars().take(28).collect(),
        });
        let _ = ctx.shell_tx.send(ShellEvent::Chunk {
            id: job_id.clone(),
            text: echo_line(&ctx.ws, program, args).to_string(),
        });

        let sandbox = ctx.sandbox.clone();
        let ws = ctx.ws.clone();
        let shell_tx = ctx.shell_tx.clone();
        let shells = ctx.shells.clone();
        let job_for_task = job_id.clone();
        let stream_id = job_id.clone();
        let program_owned = program.to_string();
        let args_owned = args.to_vec();
        // Background jobs outlive the HTTP call by design, so they get a long
        // deadline instead of the foreground cap.
        const BG_TIMEOUT: Duration = Duration::from_secs(24 * 60 * 60);
        tokio::spawn(async move {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let fwd_tx = shell_tx.clone();
            let forwarder = tokio::spawn(async move {
                while let Some(text) = rx.recv().await {
                    let _ = fwd_tx.send(ShellEvent::Chunk { id: stream_id.clone(), text });
                }
            });
            let out = sandbox
                .run_streaming(&ws, &program_owned, &args_owned, BG_TIMEOUT, &child, Some(tx), None)
                .await;
            drop_forwarder(forwarder).await;
            watcher.abort();
            let note = match out {
                Ok(o) => format!("\n[exited code {}]\n", o.exit_code),
                Err(_) => "\n[stopped]\n".to_string(),
            };
            let _ = shell_tx.send(ShellEvent::Chunk { id: job_for_task.clone(), text: note });
            shells.release(&job_for_task, gen);
        });

        Ok(format!(
            "started `{program}` in the background as job {job_id}; it now has its own tab in \
             the terminal panel and keeps running after this reply — do not start it again. \
             Its output there names the port it actually bound to; read that rather than \
             assuming a default."
        ))
    }
}

async fn drop_forwarder(handle: tokio::task::JoinHandle<()>) {
    handle.await.ok();
}

fn send_chunk(ctx: &ToolCtx, text: String) {
    let _ = ctx.shell_tx.send(ShellEvent::Chunk { id: AGENT_STREAM_ID.into(), text });
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
                            cancel,
                        )
                        .await?,
                );
            }
            Stack::Go => {
                return format_cmd(
                    ctx.sandbox
                        .run(&ctx.ws, "go", &["build".into(), "./...".into()], Duration::from_secs(240), cancel)
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
                cancel,
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
                cancel,
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
                .run(&ctx.ws, program, &args, Duration::from_secs(300), cancel)
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

/// Random-enough id for permission requests; not security-sensitive.
fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0);
    format!("{nanos:x}-{:04x}", (nanos as u32) & 0xffff ^ std::process::id())
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
mod tool_tests {
    use super::*;

    #[test]
    fn read_slices_are_numbered_and_say_what_is_left() {
        let text = (1..=10).map(|i| format!("line{i}")).collect::<Vec<_>>().join("\n");
        let out = slice_lines(&text, Some(3), Some(2));
        assert!(out.contains("     3\tline3"), "{out}");
        assert!(out.contains("     4\tline4"), "{out}");
        assert!(!out.contains("line5"), "limit must be respected: {out}");
        // The model needs to know how to ask for the rest.
        assert!(out.contains("6 more lines"), "{out}");
        assert!(out.contains("next offset 5"), "{out}");
    }

    #[test]
    fn read_slice_past_the_end_says_so_rather_than_returning_nothing() {
        let out = slice_lines("a\nb", Some(99), None);
        assert!(out.contains("2 lines"), "{out}");
        assert!(out.contains("past the end"), "{out}");
    }

    /// Offset 0 is a common off-by-one from a model; clamp instead of panicking.
    #[test]
    fn read_slice_clamps_a_zero_offset() {
        let out = slice_lines("a\nb", Some(0), Some(1));
        assert!(out.contains("     1\ta"), "{out}");
    }

    #[test]
    fn todos_render_progress_the_model_can_act_on() {
        let items = vec![
            ("wire the route".to_string(), "done".to_string()),
            ("add the test".to_string(), "running".to_string()),
            ("update docs".to_string(), "pending".to_string()),
        ];
        let out = render_todos(&items);
        assert!(out.starts_with("1/3 done"), "{out}");
        assert!(out.contains("[x] wire the route"), "{out}");
        assert!(out.contains("[>] add the test"), "{out}");
        assert!(out.contains("[ ] update docs"), "{out}");
        assert_eq!(render_todos(&[]), "todo list cleared");
    }

    /// Permission rules match this string, so getting it wrong silently makes
    /// every `bash` rule fire on an empty subject and match `*` only.
    #[test]
    fn call_subject_extracts_what_rules_match_on() {
        let cmd = json!({ "program": "git", "args": ["push", "--force"] });
        assert_eq!(call_subject("run_command", &cmd), "git push --force");
        // no args: still the bare program, not "git "
        assert_eq!(call_subject("run_command", &json!({ "program": "date" })), "date");
        assert_eq!(
            call_subject("read_file", &json!({ "path": "src/main.rs" })),
            "src/main.rs"
        );
        assert_eq!(call_subject("web_fetch", &json!({ "url": "https://x.dev" })), "https://x.dev");
        assert_eq!(call_subject("check_code", &json!({})), "");
    }

    /// The registry is now derived, so a denied tool must genuinely disappear
    /// from the schema the model sees.
    #[test]
    fn denied_tools_are_absent_from_the_schema() {
        let perms = PermissionSet::from_pairs(&[
            ("run_command", Permission::Deny),
            ("edit_file", Permission::Deny),
        ]);
        let reg = ToolRegistry::for_permissions(&perms);
        let names: Vec<&str> = reg.iter().map(|t| t.name()).collect();
        assert!(!names.contains(&"run_command"), "{names:?}");
        assert!(!names.contains(&"edit_file"), "{names:?}");
        assert!(names.contains(&"read_file"), "{names:?}");
        // The schema list matches iter(), except `skill` which is only offered
        // when the workspace actually has skills installed.
        let schema_names: Vec<String> = reg
            .schemas()
            .iter()
            .map(|s| s["name"].as_str().unwrap_or("").to_string())
            .collect();
        let expected: Vec<&str> = names.iter().copied().filter(|n| *n != "skill").collect();
        assert_eq!(schema_names, expected);
    }

    /// The catalogue is how the model learns a skill exists. If it stops being
    /// injected, skills become invisible and the feature silently does nothing.
    #[test]
    fn skill_schema_carries_the_catalogue_and_vanishes_when_empty() {
        let reg = ToolRegistry::full();

        // Empty shelf: the tool is not offered at all.
        let names: Vec<String> = reg
            .schemas_with(&[])
            .iter()
            .map(|s| s["name"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(!names.contains(&"skill".to_string()), "{names:?}");

        // With one installed it appears, description inline.
        let installed = [crate::skills::Skill {
            name: "code-review".into(),
            description: "House review checklist".into(),
            path: std::path::PathBuf::from("SKILL.md"),
        }];
        let schemas = reg.schemas_with(&installed);
        let skill = schemas
            .iter()
            .find(|s| s["name"] == "skill")
            .expect("skill tool should be offered once a skill exists");
        let desc = skill["description"].as_str().unwrap();
        assert!(desc.contains("code-review"), "{desc}");
        assert!(desc.contains("House review checklist"), "{desc}");
    }

    /// Build a context against a real temp workspace so the file tools can be
    /// exercised end to end.
    fn test_ctx(root: &std::path::Path) -> ToolCtx {
        let (events, _) = tokio::sync::broadcast::channel(64);
        let (shell_tx, _) = tokio::sync::broadcast::channel(64);
        ToolCtx {
            ws: WorkspaceRoot::open(root).unwrap(),
            sandbox: sandbox::native(),
            events,
            shell_tx,
            shells: ShellRegistry::new(),
            perm: None,
            perms: PermissionSet::allow_all(),
            spawn_subagent: None,
            reads: Default::default(),
            writes: Default::default(),
            read_budget: Default::default(),
            before: Default::default(),
        }
    }

    /// The reported waste: a run pulled thirteen whole files (43 KB) before
    /// attempting a single edit. Telling the model to read selectively did not
    /// work, so the budget refuses further whole-file reads once spent.
    #[tokio::test]
    async fn whole_file_reads_stop_at_the_budget() {
        let root = std::env::temp_dir().join("ide-ai-read-budget-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let big = "x".repeat(25_000);
        for n in ["a.txt", "b.txt", "c.txt", "d.txt"] {
            std::fs::write(root.join(n), &big).unwrap();
        }
        let ctx = test_ctx(&root);
        let cancel = CancellationToken::new();
        let read = |name: &str| {
            let v = json!({ "path": name });
            async { ReadFile.call(&ctx, v, &cancel).await.unwrap() }
        };

        // Under budget: real content.
        assert!(read("a.txt").await.contains("xxx"));
        assert!(read("b.txt").await.contains("xxx"));
        // 50 KB spent; the next whole-file read crosses 60 KB and is refused.
        let refused = read("c.txt").await;
        assert!(refused.starts_with("refused:"), "{}", &refused[..80.min(refused.len())]);
        assert!(refused.contains("budget"), "{refused}");
        assert!(refused.contains("search_files"), "must say what to do instead: {refused}");

        // A range read still works — the budget bounds bulk reading, not access.
        let sliced = ReadFile
            .call(&ctx, json!({ "path": "d.txt", "offset": 1, "limit": 1 }), &cancel)
            .await
            .unwrap();
        assert!(sliced.contains("xxx"), "ranges stay available: {sliced}");

        std::fs::remove_dir_all(&root).ok();
    }

    /// The reported waste: the same fifteen files read three times over for
    /// ~30k tokens. A second read of unchanged content must return a pointer,
    /// not the file — and an edit must bring the real contents back.
    #[tokio::test]
    async fn a_file_is_only_sent_once_until_it_changes() {
        let root = std::env::temp_dir().join("ide-ai-read-cache-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "hello\nworld\n").unwrap();
        let ctx = test_ctx(&root);
        let cancel = CancellationToken::new();
        let arg = json!({ "path": "a.txt" });

        let first = ReadFile.call(&ctx, arg.clone(), &cancel).await.unwrap();
        assert!(first.contains("hello"), "first read returns the file: {first}");

        let second = ReadFile.call(&ctx, arg.clone(), &cancel).await.unwrap();
        assert!(!second.contains("hello"), "second read must not resend it: {second}");
        assert!(second.contains("unchanged"), "{second}");
        assert!(second.contains("2 lines"), "{second}");

        // Changing the file on disk makes it fresh again.
        std::fs::write(root.join("a.txt"), "hello\nthere\n").unwrap();
        let third = ReadFile.call(&ctx, arg.clone(), &cancel).await.unwrap();
        assert!(third.contains("there"), "changed content must be resent: {third}");

        // An explicit slice is always honoured — the model is asking for a
        // specific window, not the copy it already has.
        let sliced = ReadFile
            .call(&ctx, json!({ "path": "a.txt", "offset": 1, "limit": 1 }), &cancel)
            .await
            .unwrap();
        assert!(sliced.contains("hello"), "{sliced}");

        std::fs::remove_dir_all(&root).ok();
    }

    /// Writing must both record the write and invalidate the read cache, or the
    /// next read would claim the file is unchanged when the agent just edited it.
    #[tokio::test]
    async fn writing_records_the_change_and_refreshes_the_cache() {
        let root = std::env::temp_dir().join("ide-ai-write-cache-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("b.txt"), "one\n").unwrap();
        let ctx = test_ctx(&root);
        let cancel = CancellationToken::new();
        let arg = json!({ "path": "b.txt" });

        ReadFile.call(&ctx, arg.clone(), &cancel).await.unwrap();
        assert!(ctx.writes.lock().unwrap().is_empty(), "reading is not writing");

        WriteFile
            .call(&ctx, json!({ "path": "b.txt", "content": "two\n" }), &cancel)
            .await
            .unwrap();
        assert!(
            ctx.writes.lock().unwrap().contains("b.txt"),
            "a write must be recorded so 'done with no changes' can be caught"
        );

        let after = ReadFile.call(&ctx, arg, &cancel).await.unwrap();
        assert!(after.contains("two"), "post-write read must return new content: {after}");

        std::fs::remove_dir_all(&root).ok();
    }

    /// Backs "undo this message": the snapshot must hold what a file had
    /// *before this task*, not before its most recent edit — a second write
    /// to the same path must not overwrite the recorded baseline.
    #[tokio::test]
    async fn before_snapshot_keeps_the_original_not_the_intermediate_state() {
        let root = std::env::temp_dir().join("ide-ai-before-snapshot-test");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("b.txt"), "original\n").unwrap();
        let ctx = test_ctx(&root);
        let cancel = CancellationToken::new();

        WriteFile
            .call(&ctx, json!({ "path": "b.txt", "content": "first edit\n" }), &cancel)
            .await
            .unwrap();
        WriteFile
            .call(&ctx, json!({ "path": "b.txt", "content": "second edit\n" }), &cancel)
            .await
            .unwrap();

        assert_eq!(
            ctx.before.lock().unwrap().get("b.txt"),
            Some(&Some("original\n".to_string())),
        );

        // A brand new file: reverting it means deleting it, so `None` records that.
        WriteFile
            .call(&ctx, json!({ "path": "new.txt", "content": "hi\n" }), &cancel)
            .await
            .unwrap();
        assert_eq!(ctx.before.lock().unwrap().get("new.txt"), Some(&None));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ask_keeps_the_tool_available() {
        let perms = PermissionSet::from_pairs(&[("run_command", Permission::Ask)]);
        let names: Vec<&str> = ToolRegistry::for_permissions(&perms)
            .iter()
            .map(|t| t.name())
            .collect();
        assert!(names.contains(&"run_command"), "ask must not hide the tool");
    }
}

#[cfg(test)]
mod long_running_tests {
    use super::looks_long_running;

    fn a(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// The bug: a dev server run in the foreground is killed the moment the
    /// tool returns, so "start the app" started it and immediately stopped it.
    #[test]
    fn catches_the_dev_server_shapes_that_died() {
        assert!(looks_long_running("npm", &a(&["run", "dev"])));
        assert!(looks_long_running("npm", &a(&["start"])));
        assert!(looks_long_running("pnpm", &a(&["dev"])));
        assert!(looks_long_running("yarn", &a(&["serve"])));
        assert!(looks_long_running("bun", &a(&["run", "preview"])));
        assert!(looks_long_running("npx", &a(&["vite", "--host"])));
        // bare binaries, and with a path or extension
        assert!(looks_long_running("vite", &[]));
        assert!(looks_long_running("next", &a(&["dev"])));
        assert!(looks_long_running("nodemon", &a(&["server.js"])));
        assert!(looks_long_running("C:\\bin\\npm.cmd", &a(&["run", "dev"])));
    }

    /// One-shot commands must still run in the foreground — the agent needs
    /// their output, and refusing them would break installs and builds.
    #[test]
    fn leaves_one_shot_commands_alone() {
        assert!(!looks_long_running("npm", &a(&["install"])));
        assert!(!looks_long_running("npm", &a(&["run", "build"])));
        assert!(!looks_long_running("npm", &a(&["run", "lint"])));
        assert!(!looks_long_running("git", &a(&["status"])));
        assert!(!looks_long_running("cargo", &a(&["test"])));
        assert!(!looks_long_running("date", &[]));
        // "start" only counts behind a package runner, not any binary at all
        assert!(!looks_long_running("git", &a(&["start"])));
    }
}

#[cfg(test)]
mod web_tests {
    use super::*;

    /// The string-level half of the guard, without DNS: parse the URL and run
    /// the lexical host checks plus literal-IP classification.
    fn lexical_check(url: &str) -> Result<()> {
        let u = reqwest::Url::parse(url)?;
        let (host, _port) = url_host_port(&u)?;
        check_public_host(&host)?;
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            if !is_public_ip(ip) {
                anyhow::bail!("non-public address: {ip}");
            }
        }
        Ok(())
    }

    #[test]
    fn allows_ordinary_public_urls() {
        for u in [
            "https://doc.rust-lang.org/std/",
            "http://example.com/page?q=1",
            "https://api-docs.deepseek.com/guides/thinking_mode/",
            "https://sub.domain.co.uk:8443/path",
            "http://172.15.0.1/",
            "http://172.32.0.1/",
        ] {
            assert!(lexical_check(u).is_ok(), "should allow {u}");
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
            "http://100.64.0.1/",   // CGNAT — Tailscale and friends
            "http://100.127.255.254/",
            "http://224.0.0.1/",
            "http://0.0.0.0/",
            "http://db.internal/",
            "http://printer.local/",
            "http://[::ffff:127.0.0.1]/",  // IPv4-mapped loopback
            "http://[fe80::1]/",
            "http://[fd00::1]/",
        ] {
            assert!(lexical_check(u).is_err(), "should refuse {u}");
        }
    }

    #[test]
    fn public_172_addresses_are_still_allowed() {
        // 172.15 and 172.32 are outside the private 172.16-31 block
        assert!(lexical_check("http://172.15.0.1/").is_ok());
        assert!(lexical_check("http://172.32.0.1/").is_ok());
    }

    #[test]
    fn refuses_non_http_schemes() {
        for u in ["file:///etc/passwd", "ftp://host/x", "javascript:alert(1)", ""] {
            assert!(lexical_check(u).is_err(), "should refuse {u}");
        }
    }

    #[test]
    fn credentials_in_url_do_not_smuggle_a_private_host() {
        assert!(lexical_check("http://user@127.0.0.1/").is_err());
        assert!(lexical_check("http://evil.com@localhost/").is_err());
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

    /// Regression: the old scanner indexed a Vec<char> with byte offsets, so any
    /// page with multi-byte characters panicked or failed to strip script tags.
    #[test]
    fn html_with_multibyte_characters_does_not_panic() {
        let html = "<p>héllo wörld ✓ 日本語</p><script>alert('x')</script><style>a{}</style>";
        let text = html_to_text(html);
        assert!(text.contains("日本語"), "{text}");
        assert!(!text.contains("alert"), "script must be dropped: {text}");
    }

    #[test]
    fn named_and_numeric_entities_decode() {
        assert_eq!(
            html_to_text("<p>don&#x27;t &amp; can&#39;t &quot;q&quot;</p>"),
            "don't & can't \"q\""
        );
        // unknown entities pass through
        assert_eq!(html_to_text("<p>&unknown;</p>"), "&unknown;");
    }
}
