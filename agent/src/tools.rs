use anyhow::{Context, Result};
use async_trait::async_trait;
use ide_core::{AgentEvent, CommandOutput, WorkspaceRoot};
use sandbox::Sandbox;
use serde_json::{json, Value};
use similar::{ChangeTag, TextDiff};
use std::net::ToSocketAddrs;
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
                Box::new(SearchFiles),
                Box::new(WebSearch),
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
struct SearchFiles;
struct WebSearch;
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
        let existing = viewer::read_file(&ctx.ws, path).ok();
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
        let diff = unified_diff(path, &before, &after);
        let _ = ctx.events.send(AgentEvent::Diff {
            path: path.to_string(),
            diff: diff.clone(),
        });
        Ok(diff)
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
        format_cmd(ctx.sandbox.run(&ctx.ws, program, &args, Duration::from_secs(120), cancel).await?)
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
