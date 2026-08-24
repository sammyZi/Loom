//! Provider registry and streaming engines — the Rust counterpart of
//! opencode's provider layer. One OpenAI-compatible engine serves most
//! providers (OpenAI, DeepSeek, Groq, Mistral, xAI, OpenRouter, Together,
//! Fireworks, Cerebras, Ollama, LM Studio, Google's OpenAI endpoint); Anthropic
//! gets a native Messages-API engine because its wire format differs.

use crate::settings::Settings;
use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use tokio_util::sync::CancellationToken;

/// Used when nothing is configured anywhere; keeps `.env` development working.
pub const DEFAULT_MODEL: &str = "deepseek/deepseek-chat";

const MAX_OUTPUT_TOKENS: u64 = 8192;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// POST {base}/chat/completions, SSE `chat.completion.chunk` frames.
    OpenAiCompat,
    /// Native Messages API at {base}/v1/messages, SSE typed events.
    Anthropic,
}

#[derive(Clone, Copy)]
pub struct ModelDef {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub context: u64,
}

#[derive(Clone, Copy)]
pub struct ProviderDef {
    pub id: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    /// Base URL; the engines append `/chat/completions` (compat) or `/v1/messages`… appropriately. See `endpoint_for`.
    pub base_url: &'static str,
    pub env_keys: &'static [&'static str],
    /// Local runtimes work without any key and are always selectable.
    pub key_optional: bool,
    /// Sends OpenAI's `reasoning_effort` parameter.
    pub reasoning_param: bool,
    /// DeepSeek-style `"thinking": {"type":"enabled"}` body flag.
    pub thinking_flag: bool,
    /// Asks for `stream_options.include_usage`.
    pub stream_usage: bool,
    pub models: &'static [ModelDef],
}

macro_rules! models {
    ($(($id:literal, $label:literal, $hint:literal, $ctx:literal)),+ $(,)?) => {
        &[$(ModelDef { id: $id, label: $label, hint: $hint, context: $ctx }),+]
    };
}

pub static CATALOG: &[ProviderDef] = &[
    ProviderDef {
        id: "deepseek",
        label: "DeepSeek",
        kind: Kind::OpenAiCompat,
        base_url: "https://api.deepseek.com",
        env_keys: &["DEEPSEEK_API_KEY"],
        key_optional: false,
        reasoning_param: true,
        thinking_flag: true,
        stream_usage: true,
        models: models![
            ("deepseek-chat", "DeepSeek Chat", "Fast", 128_000),
            ("deepseek-reasoner", "DeepSeek Reasoner", "Strong reasoning", 128_000),
        ],
    },
    ProviderDef {
        id: "anthropic",
        label: "Anthropic",
        kind: Kind::Anthropic,
        base_url: "https://api.anthropic.com",
        env_keys: &["ANTHROPIC_API_KEY"],
        key_optional: false,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("claude-sonnet-4-5", "Claude Sonnet 4.5", "Best coding", 200_000),
            ("claude-opus-4-6", "Claude Opus 4.6", "Most capable", 200_000),
            ("claude-haiku-4-5", "Claude Haiku 4.5", "Fastest", 200_000),
        ],
    },
    ProviderDef {
        id: "openai",
        label: "OpenAI",
        kind: Kind::OpenAiCompat,
        base_url: "https://api.openai.com/v1",
        env_keys: &["OPENAI_API_KEY"],
        key_optional: false,
        reasoning_param: true,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("gpt-5.2", "GPT-5.2", "Flagship", 400_000),
            ("gpt-5.2-mini", "GPT-5.2 mini", "Fast", 400_000),
            ("gpt-4.1", "GPT-4.1", "Long context", 1_000_000),
        ],
    },
    ProviderDef {
        id: "google",
        label: "Google Gemini",
        kind: Kind::OpenAiCompat,
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai",
        env_keys: &["GEMINI_API_KEY", "GOOGLE_API_KEY"],
        key_optional: false,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("gemini-3-pro", "Gemini 3 Pro", "Multimodal", 1_000_000),
            ("gemini-3-flash", "Gemini 3 Flash", "Fast", 1_000_000),
        ],
    },
    ProviderDef {
        id: "groq",
        label: "Groq",
        kind: Kind::OpenAiCompat,
        base_url: "https://api.groq.com/openai/v1",
        env_keys: &["GROQ_API_KEY"],
        key_optional: false,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("llama-3.3-70b-versatile", "Llama 3.3 70B", "Very fast", 128_000),
            ("openai/gpt-oss-120b", "GPT-OSS 120B", "Open weights", 131_072),
        ],
    },
    ProviderDef {
        id: "xai",
        label: "xAI",
        kind: Kind::OpenAiCompat,
        base_url: "https://api.x.ai/v1",
        env_keys: &["XAI_API_KEY"],
        key_optional: false,
        reasoning_param: true,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("grok-4", "Grok 4", "Frontier", 256_000),
            ("grok-code-fast-1", "Grok Code Fast", "Fast coding", 256_000),
        ],
    },
    ProviderDef {
        id: "mistral",
        label: "Mistral",
        kind: Kind::OpenAiCompat,
        base_url: "https://api.mistral.ai/v1",
        env_keys: &["MISTRAL_API_KEY"],
        key_optional: false,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("mistral-large-latest", "Mistral Large", "Flagship", 128_000),
            ("codestral-latest", "Codestral", "Code", 256_000),
        ],
    },
    // opencode's own gateway. It is a curated, multi-vendor endpoint, so it is
    // registered as a gateway like OpenRouter rather than a first-party vendor.
    // Only its OpenAI-compatible surface is reachable from here: Zen routes
    // Anthropic models through /messages and Google's through /models/{id},
    // which this engine does not speak. Base URL is editable in settings.
    ProviderDef {
        id: "opencode",
        label: "OpenCode Zen",
        kind: Kind::OpenAiCompat,
        base_url: "https://opencode.ai/zen/v1",
        env_keys: &["OPENCODE_API_KEY", "OPENCODE_ZEN_API_KEY"],
        key_optional: false,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("gpt-5.5", "GPT-5.5", "Via OpenCode Zen", 400_000),
            ("gpt-5.4-mini", "GPT-5.4 Mini", "Cheaper, via Zen", 400_000),
            ("claude-sonnet-5", "Claude Sonnet 5", "Via OpenCode Zen", 200_000),
            ("claude-opus-5", "Claude Opus 5", "Via OpenCode Zen", 200_000),
        ],
    },
    ProviderDef {
        id: "openrouter",
        label: "OpenRouter",
        kind: Kind::OpenAiCompat,
        base_url: "https://openrouter.ai/api/v1",
        env_keys: &["OPENROUTER_API_KEY"],
        key_optional: false,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("openrouter/auto", "Auto", "Picks best per request", 200_000),
            ("anthropic/claude-sonnet-4.5", "Claude Sonnet 4.5", "Via OpenRouter", 200_000),
            ("openai/gpt-5.2", "GPT-5.2", "Via OpenRouter", 400_000),
        ],
    },
    ProviderDef {
        id: "together",
        label: "Together",
        kind: Kind::OpenAiCompat,
        base_url: "https://api.together.xyz/v1",
        env_keys: &["TOGETHER_API_KEY"],
        key_optional: false,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("meta-llama/Llama-3.3-70B-Instruct-Turbo", "Llama 3.3 70B Turbo", "Open weights", 128_000),
            ("deepseek-ai/DeepSeek-V3", "DeepSeek V3", "Open weights", 128_000),
        ],
    },
    ProviderDef {
        id: "fireworks",
        label: "Fireworks",
        kind: Kind::OpenAiCompat,
        base_url: "https://api.fireworks.ai/inference/v1",
        env_keys: &["FIREWORKS_API_KEY"],
        key_optional: false,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("accounts/fireworks/models/deepseek-v3", "DeepSeek V3", "Hosted", 128_000),
            ("accounts/fireworks/models/llama4-maverick-instruct-basic", "Llama 4 Maverick", "Hosted", 128_000),
        ],
    },
    ProviderDef {
        id: "cerebras",
        label: "Cerebras",
        kind: Kind::OpenAiCompat,
        base_url: "https://api.cerebras.ai/v1",
        env_keys: &["CEREBRAS_API_KEY"],
        key_optional: false,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: true,
        models: models![
            ("llama-3.3-70b", "Llama 3.3 70B", "Wafer-fast", 128_000),
        ],
    },
    ProviderDef {
        id: "ollama",
        label: "Ollama",
        kind: Kind::OpenAiCompat,
        base_url: "http://localhost:11434/v1",
        env_keys: &[],
        key_optional: true,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: false,
        models: models![
            ("qwen2.5-coder", "Qwen 2.5 Coder", "Local code model", 32_768),
            ("llama3.3", "Llama 3.3", "Local general", 32_768),
        ],
    },
    ProviderDef {
        id: "lmstudio",
        label: "LM Studio",
        kind: Kind::OpenAiCompat,
        base_url: "http://localhost:1234/v1",
        env_keys: &[],
        key_optional: true,
        reasoning_param: false,
        thinking_flag: false,
        stream_usage: false,
        models: models![
            ("local-model", "Loaded model", "Whatever LM Studio has loaded", 32_768),
        ],
    },
];

pub fn provider_def(id: &str) -> Option<&'static ProviderDef> {
    CATALOG.iter().find(|p| p.id == id)
}

/// Effective base URL: config.json override wins over the built-in default.
/// Ollama additionally honours OLLAMA_HOST so remote runtimes are one env var
/// away, matching its own CLI conventions.
/// The provider's root URL, before any endpoint path is appended. `base_url_for`
/// tacks `/chat/completions` (or `/v1/messages`) on for the streaming call, so
/// anything else — the model list, for one — has to start from here instead.
pub fn provider_root(def: &ProviderDef, settings: &Settings) -> String {
    settings
        .cfg_for(def.id)
        .and_then(|c| c.base_url.clone())
        .map(|u| u.trim().trim_end_matches('/').to_string())
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| {
            if def.id == "ollama" {
                if let Ok(host) = std::env::var("OLLAMA_HOST") {
                    let host = host.trim().trim_start_matches("http://").trim_end_matches('/').to_string();
                    if !host.is_empty() {
                        return format!("http://{host}");
                    }
                }
            }
            def.base_url.to_string()
        })
}

pub fn base_url_for(def: &ProviderDef, settings: &Settings) -> String {
    let raw = provider_root(def, settings);
    match def.kind {
        Kind::OpenAiCompat => format!("{raw}/chat/completions"),
        Kind::Anthropic => format!("{raw}/v1/messages"),
    }
}

/// True when the provider can actually be called right now.
pub fn is_available(def: &ProviderDef, settings: &Settings) -> bool {
    def.key_optional || settings.api_key_for(def.id, def.env_keys).is_some()
}

// ---------------------------------------------------------------------------
// Wire types shared by every engine
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn user_text(text: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: Some(Value::String(text.into())),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// A user turn built from content blocks (OpenAI's `text` / `image_url`
    /// shape) instead of a single string — the only way to carry an attached
    /// image. `openai_stream` sends this shape through unchanged;
    /// `to_anthropic_messages` translates each block for the Anthropic engine.
    pub fn user_blocks(blocks: Vec<Value>) -> Self {
        Self {
            role: "user".into(),
            content: Some(Value::Array(blocks)),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// Plain-text view used for size accounting and summaries.
    pub fn preview(&self) -> String {
        self.content
            .as_ref()
            .map(|c| c.to_string())
            .or_else(|| self.tool_calls.as_ref().map(|c| c.to_string()))
            .unwrap_or_default()
    }

    fn text(&self) -> String {
        match self.content.as_ref() {
            Some(Value::String(s)) => s.clone(),
            // Content blocks (an attached image alongside text): join the
            // text parts and drop the image, for callers that only want words
            // — `other.to_string()` here used to dump the raw base64 payload.
            Some(Value::Array(blocks)) => blocks
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("\n"),
            Some(other) => other.to_string(),
            None => String::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum StreamKind {
    /// Tokens reported by the provider: `input` is what this call consumed
    /// (the live conversation size), `output` is what it generated.
    Usage { input: Option<u64>, output: Option<u64> },
    ThinkDelta(String),
    TextDelta(String),
    ToolUse { name: String, input: Value },
    Error(String),
}

#[derive(Debug, Clone)]
pub struct ToolUse {
    pub id: String,
    pub name: String,
    pub input: Value,
}

#[derive(Debug, Clone)]
pub struct AssistantTurn {
    pub text: String,
    pub tools: Vec<ToolUse>,
}

// ---------------------------------------------------------------------------
// Model selection
// ---------------------------------------------------------------------------

/// Accepts `provider/model`, bare model ids, and the legacy short ids from the
/// single-provider era (`flash`, `pro`, `deepseek-v4-*`).
pub fn normalize_model(sel: &str, settings: &Settings) -> String {
    let s = sel.trim();
    if s.is_empty() {
        return default_model(settings);
    }
    if s.contains('/') {
        return s.to_string();
    }
    match s {
        "flash" | "deepseek-v4-flash" => return "deepseek/deepseek-chat".into(),
        "pro" | "deepseek-v4-pro" => return "deepseek/deepseek-reasoner".into(),
        _ => {}
    }
    for p in CATALOG {
        for m in p.models {
            if m.id == s {
                return format!("{}/{s}", p.id);
            }
        }
    }
    format!("deepseek/{s}")
}

fn default_model(settings: &Settings) -> String {
    let configured = settings.default_model.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    for p in CATALOG {
        if is_available(p, settings) {
            return format!("{}/{}", p.id, p.models[0].id);
        }
    }
    DEFAULT_MODEL.to_string()
}

pub fn context_limit(model_sel: &str, settings: &Settings) -> u64 {
    let sel = normalize_model(model_sel, settings);
    let Some((pid, mid)) = sel.split_once('/') else {
        return 128_000;
    };
    if let Some(p) = provider_def(pid) {
        if let Some(m) = p.models.iter().find(|m| m.id == mid) {
            return m.context;
        }
    }
    128_000
}

/// Clamp to the three documented levels; anything else falls back to medium.
pub fn normalize_effort(s: &str) -> &'static str {
    match s.trim().to_ascii_lowercase().as_str() {
        "low" => "low",
        "high" => "high",
        _ => "medium",
    }
}

/// Catalog payload for the model picker. Every provider is listed so users can
/// discover them; unconfigured ones are flagged and greyed out client-side.
/// Ask the provider which models it actually serves. Gateways like OpenRouter
/// host hundreds and add more weekly, so a hardcoded list can never be right —
/// CATALOG is only the fallback for when this call fails or is not supported.
pub async fn remote_models(def: &ProviderDef, settings: &Settings) -> Option<Vec<Value>> {
    let base = provider_root(def, settings);
    let base = base.trim_end_matches('/');
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let key = settings.api_key_for(def.id, def.env_keys);
    // Both shapes answer `{ "data": [ { "id": … } ] }`; only the auth headers
    // and the path differ, so one parser covers the lot.
    let req = match def.kind {
        Kind::OpenAiCompat => {
            let mut r = client.get(format!("{base}/models"));
            if let Some(k) = key {
                r = r.bearer_auth(k);
            }
            r
        }
        Kind::Anthropic => client
            .get(format!("{base}/v1/models"))
            .header("x-api-key", key?)
            .header("anthropic-version", "2023-06-01"),
    };
    let res = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("{}: model list request failed: {e}", def.id);
            return None;
        }
    };
    if !res.status().is_success() {
        tracing::warn!("{}: model list returned {}", def.id, res.status());
        return None;
    }
    let body: Value = match res.json().await {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!("{}: model list was not JSON: {e}", def.id);
            return None;
        }
    };
    let Some(arr) = body.get("data").and_then(|d| d.as_array()) else {
        tracing::warn!("{}: model list had no `data` array", def.id);
        return None;
    };

    let mut out: Vec<Value> = arr
        .iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?;
            if id.is_empty() {
                return None;
            }
            let label = m
                .get("name")
                .or_else(|| m.get("display_name")) // Anthropic's spelling
                .and_then(|v| v.as_str())
                .unwrap_or(id)
                .to_string();
            // OpenRouter reports context_length; plain OpenAI reports nothing.
            let context = m
                .get("context_length")
                .or_else(|| m.get("context_window"))
                .and_then(|v| v.as_u64())
                .unwrap_or(128_000);
            Some(json!({
                "id": format!("{}/{}", def.id, id),
                "label": label,
                "hint": "",
                "context": context,
            }))
        })
        .collect();
    if out.is_empty() {
        return None;
    }
    out.sort_by(|a, b| a["label"].as_str().unwrap_or("").cmp(b["label"].as_str().unwrap_or("")));
    Some(out)
}

/// Catalog with each provider's live model list merged in where we could get
/// one. Providers we cannot query keep their curated defaults.
pub async fn groups_json_live(settings: &Settings) -> Value {
    let mut base = groups_json(settings);
    let Some(groups) = base.get_mut("groups").and_then(|g| g.as_array_mut()) else {
        return base;
    };
    for group in groups.iter_mut() {
        let Some(id) = group.get("id").and_then(|v| v.as_str()).map(String::from) else {
            continue;
        };
        let Some(def) = provider_def(&id) else { continue };
        // Only bother for providers that are actually usable; an unconfigured
        // one would just fail the request and slow the response down.
        if !def.key_optional && settings.api_key_for(def.id, def.env_keys).is_none() {
            continue;
        }
        if let Some(models) = remote_models(def, settings).await {
            group["models"] = Value::Array(models);
        }
    }
    base
}

pub fn groups_json(settings: &Settings) -> Value {
    let groups: Vec<Value> = CATALOG
        .iter()
        .map(|p| {
            let models: Vec<Value> = p
                .models
                .iter()
                .map(|m| {
                    json!({
                        "id": format!("{}/{}", p.id, m.id),
                        "label": m.label,
                        "hint": m.hint,
                        "context": m.context,
                    })
                })
                .collect();
            json!({
                "id": p.id,
                "label": p.label,
                "key_set": settings.api_key_for(p.id, p.env_keys).is_some(),
                "key_optional": p.key_optional,
                "kind": if p.kind == Kind::Anthropic { "anthropic" } else { "openai" },
                "base_url": base_url_for(p, settings),
                "default_base_url": p.base_url,
                "env_keys": p.env_keys,
                "models": models,
            })
        })
        .collect();
    json!({ "groups": groups, "default": default_model(settings) })
}

// ---------------------------------------------------------------------------
// Streaming dispatch
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub async fn stream(
    client: &reqwest::Client,
    model_sel: &str,
    system: &str,
    tools: &[Value],
    messages: &[Message],
    effort: &str,
    settings: &Settings,
    cancel: &CancellationToken,
    on_event: impl FnMut(StreamKind),
) -> Result<AssistantTurn> {
    let sel = normalize_model(model_sel, settings);
    let Some((pid, mid)) = sel.split_once('/') else {
        bail!("invalid model `{sel}`");
    };
    let def = provider_def(pid).unwrap_or(provider_def("deepseek").unwrap());
    let key = settings.api_key_for(def.id, def.env_keys);
    if !def.key_optional && key.is_none() {
        bail!(
            "{} has no API key — add one in Settings (gear icon) or set {}",
            def.label,
            def.env_keys.first().copied().unwrap_or("its API key variable")
        );
    }
    let endpoint = base_url_for(def, settings);
    let effort = normalize_effort(effort);

    match def.kind {
        Kind::OpenAiCompat => {
            openai_stream(
                client, &endpoint, def, key.as_deref(), mid, system, tools, messages, effort,
                cancel, on_event,
            )
            .await
        }
        Kind::Anthropic => {
            anthropic_stream(
                client, &endpoint, key.as_deref(), mid, system, tools, messages, cancel, on_event,
            )
            .await
        }
    }
}

async fn post_sse(
    client: &reqwest::Client,
    endpoint: &str,
    headers: &[(&str, String)],
    body: Value,
    cancel: &CancellationToken,
    label: &str,
) -> Result<reqwest::Response> {
    let mut req = client.post(endpoint).json(&body);
    for (k, v) in headers {
        req = req.header(*k, v);
    }
    let res = tokio::select! {
        r = req.send() => r.with_context(|| format!("{label} request"))?,
        _ = cancel.cancelled() => anyhow::bail!("cancelled"),
    };
    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        bail!("{label} {status}: {}", truncate_err(&text));
    }
    Ok(res)
}

fn truncate_err(s: &str) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    flat.chars().take(600).collect()
}

// ---------------------------------------------------------------------------
// OpenAI-compatible engine (also DeepSeek/Groq/Mistral/xAI/Ollama/Gemini/…)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn openai_stream(
    client: &reqwest::Client,
    endpoint: &str,
    def: &ProviderDef,
    api_key: Option<&str>,
    model: &str,
    system: &str,
    tools: &[Value],
    messages: &[Message],
    effort: &str,
    cancel: &CancellationToken,
    mut on_event: impl FnMut(StreamKind),
) -> Result<AssistantTurn> {
    let mut body_msgs = vec![Message {
        role: "system".into(),
        content: Some(Value::String(system.to_string())),
        tool_calls: None,
        tool_call_id: None,
    }];
    body_msgs.extend(messages.iter().cloned());

    let openai_tools: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t["name"],
                    "description": t["description"],
                    "parameters": t["input_schema"],
                }
            })
        })
        .collect();

    let mut body = json!({
        "model": model,
        "messages": body_msgs,
        "stream": true,
        "max_tokens": MAX_OUTPUT_TOKENS,
    });
    if openai_tools.is_empty() {
        body["temperature"] = json!(0.6);
    } else {
        body["temperature"] = json!(0.2);
        body["tools"] = json!(openai_tools);
        body["tool_choice"] = json!("auto");
    }
    if def.reasoning_param {
        body["reasoning_effort"] = json!(if openai_tools.is_empty() { "low" } else { effort });
    }
    if def.thinking_flag && !openai_tools.is_empty() {
        body["thinking"] = json!({ "type": "enabled" });
    }
    if def.stream_usage {
        body["stream_options"] = json!({ "include_usage": true });
    }

    let auth: Vec<(&str, String)> = match api_key {
        Some(k) => vec![("authorization", format!("Bearer {k}"))],
        None => vec![],
    };
    let res = post_sse(
        client,
        endpoint,
        &auth,
        body,
        cancel,
        def.label,
    )
    .await?;

    let mut acc = SseAccumulator::new();
    consume_sse(res, cancel, |block| {
        acc.handle_openai_block(block, &mut on_event)?;
        Ok(())
    })
    .await?;

    Ok(acc.finish(&mut on_event))
}

impl SseAccumulator {
    fn handle_openai_block(
        &mut self,
        block: &str,
        on_event: &mut impl FnMut(StreamKind),
    ) -> Result<()> {
        let mut data = String::new();
        for line in block.lines() {
            if let Some(rest) = line.strip_prefix("data: ") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(rest);
            }
        }
        if data.is_empty() || data == "[DONE]" {
            return Ok(());
        }
        let v: Value = match serde_json::from_str(&data) {
            Ok(v) => v,
            Err(_) => return Ok(()),
        };
        if let Some(msg) = v["error"]["message"].as_str() {
            on_event(StreamKind::Error(msg.to_string()));
            anyhow::bail!(msg.to_string());
        }
        if let Some(u) = v["usage"].as_object() {
            let input = u.get("prompt_tokens").and_then(|x| x.as_u64());
            let output = u.get("completion_tokens").and_then(|x| x.as_u64());
            // Prompt caching is the single biggest lever on input cost — a hit
            // is billed at roughly a tenth — but it is invisible unless the
            // provider's own counters are read back. DeepSeek reports these
            // directly; OpenAI nests the same idea under prompt_tokens_details.
            let hit = u
                .get("prompt_cache_hit_tokens")
                .and_then(|x| x.as_u64())
                .or_else(|| u["prompt_tokens_details"]["cached_tokens"].as_u64());
            if let (Some(i), Some(h)) = (input, hit) {
                let pct = if i > 0 { h * 100 / i } else { 0 };
                tracing::info!("prompt cache: {h}/{i} input tokens reused ({pct}%)");
            }
            if input.is_some() || output.is_some() {
                on_event(StreamKind::Usage { input, output });
            }
        }
        let Some(choice) = v["choices"].as_array().and_then(|a| a.first()) else {
            return Ok(());
        };
        let delta = &choice["delta"];
        if let Some(t) = delta["reasoning_content"].as_str() {
            on_event(StreamKind::ThinkDelta(t.to_string()));
        }
        if let Some(t) = delta["content"].as_str() {
            self.text.push_str(t);
            on_event(StreamKind::TextDelta(t.to_string()));
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for call in calls {
                let idx = call["index"].as_u64().unwrap_or(0);
                let entry = self.tools.entry(idx).or_insert_with(|| AccTool {
                    id: String::new(),
                    name: String::new(),
                    args: String::new(),
                });
                if let Some(id) = call["id"].as_str() {
                    entry.id = id.to_string();
                }
                if let Some(name) = call["function"]["name"].as_str() {
                    entry.name = name.to_string();
                }
                if let Some(args) = call["function"]["arguments"].as_str() {
                    entry.args.push_str(args);
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Anthropic native engine
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn anthropic_stream(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: Option<&str>,
    model: &str,
    system: &str,
    tools: &[Value],
    messages: &[Message],
    cancel: &CancellationToken,
    mut on_event: impl FnMut(StreamKind),
) -> Result<AssistantTurn> {
    // Cache the two blocks that never change within a run: the system prompt
    // and the tool schemas. Anthropic bills a cache read at about a tenth of a
    // fresh input token, and this prefix is re-sent on every single turn of the
    // agent loop — it is the largest repeated cost in the whole system.
    // OpenAI-compatible providers do the same thing automatically on a stable
    // prefix, which is why only this engine needs the markers.
    let cached_system = json!([{
        "type": "text",
        "text": system,
        "cache_control": { "type": "ephemeral" },
    }]);
    let mut tool_defs: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "name": t["name"],
                "description": t["description"],
                "input_schema": t["input_schema"],
            })
        })
        .collect();
    // The breakpoint goes on the last tool, so everything above it is cached.
    if let Some(last) = tool_defs.last_mut() {
        last["cache_control"] = json!({ "type": "ephemeral" });
    }
    let body = json!({
        "model": model,
        "max_tokens": MAX_OUTPUT_TOKENS,
        "system": cached_system,
        "messages": to_anthropic_messages(messages),
        "stream": true,
        "temperature": if tools.is_empty() { 1.0 } else { 0.2 },
        "tools": tool_defs,
    });
    let headers: Vec<(&str, String)> = vec![
        ("x-api-key", api_key.unwrap_or_default().to_string()),
        ("anthropic-version", "2023-06-01".into()),
    ];
    let res = post_sse(client, endpoint, &headers, body, cancel, "Anthropic").await?;

    let mut text = String::new();
    let mut tools_acc: BTreeMap<u64, AccTool> = BTreeMap::new();

    consume_sse(res, cancel, |block| {
        for line in block.lines() {
            let Some(rest) = line.strip_prefix("data: ") else { continue };
            if rest.trim() == "[DONE]" {
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(rest) else { continue };
            match v["type"].as_str() {
                Some("error") => {
                    let msg = v["error"]["message"].as_str().unwrap_or("unknown error");
                    on_event(StreamKind::Error(msg.to_string()));
                    anyhow::bail!(msg.to_string());
                }
                Some("message_start") => {
                    let u = &v["message"]["usage"];
                    let input = u["input_tokens"].as_u64();
                    // Anthropic bills cache reads separately, so they are not
                    // in input_tokens; without logging them a working cache
                    // looks like a shrinking prompt rather than a cheaper one.
                    let read = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                    let written = u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                    if read > 0 || written > 0 {
                        tracing::info!(
                            "prompt cache: {read} read, {written} written, {} fresh",
                            input.unwrap_or(0)
                        );
                    }
                    if let Some(input) = input {
                        on_event(StreamKind::Usage { input: Some(input), output: None });
                    }
                }
                Some("content_block_start") => {
                    let idx = v["index"].as_u64().unwrap_or(0);
                    if v["content_block"]["type"] == "tool_use" {
                        let e = tools_acc.entry(idx).or_insert_with(|| AccTool {
                            id: String::new(),
                            name: String::new(),
                            args: String::new(),
                        });
                        e.id = v["content_block"]["id"].as_str().unwrap_or_default().into();
                        e.name = v["content_block"]["name"].as_str().unwrap_or_default().into();
                    }
                }
                Some("content_block_delta") => {
                    let idx = v["index"].as_u64().unwrap_or(0);
                    match v["delta"]["type"].as_str() {
                        Some("text_delta") => {
                            if let Some(t) = v["delta"]["text"].as_str() {
                                text.push_str(t);
                                on_event(StreamKind::TextDelta(t.to_string()));
                            }
                        }
                        Some("thinking_delta") => {
                            if let Some(t) = v["delta"]["thinking"].as_str() {
                                on_event(StreamKind::ThinkDelta(t.to_string()));
                            }
                        }
                        Some("input_json_delta") => {
                            if let Some(part) = v["delta"]["partial_json"].as_str() {
                                tools_acc.entry(idx).or_insert_with(|| AccTool {
                                    id: String::new(),
                                    name: String::new(),
                                    args: String::new(),
                                }).args.push_str(part);
                            }
                        }
                        _ => {}
                    }
                }
                Some("message_delta") => {
                    if let Some(output) = v["usage"]["output_tokens"].as_u64() {
                        on_event(StreamKind::Usage { input: None, output: Some(output) });
                    }
                }
                _ => {}
            }
        }
        Ok(())
    })
    .await?;

    let tools_out = finalize_tools(tools_acc, &mut on_event);
    Ok(AssistantTurn { text, tools: tools_out })
}

/// OpenAI-style history → Anthropic turns: roles strictly alternate, tool
/// results ride inside `user` messages as `tool_result` blocks, consecutive
/// same-role turns merge (Anthropic rejects adjacent duplicates).
/// Splits a `data:<mime>;base64,<data>` URL into its media type and raw
/// base64 payload. Anthropic wants those as two separate fields; the browser
/// (and OpenAI's `image_url` shape) hands us one string.
fn parse_data_url(url: &str) -> Option<(&str, &str)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mime = meta.split(';').next().unwrap_or("application/octet-stream");
    Some((mime, data))
}

fn to_anthropic_messages(messages: &[Message]) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    let push = |role: &str, block: Value, out: &mut Vec<Value>| {
        if let Some(last) = out.last_mut() {
            if last["role"] == *role {
                last["content"].as_array_mut().unwrap().push(block);
                return;
            }
        }
        out.push(json!({ "role": role, "content": [block] }));
    };
    for m in messages {
        match m.role.as_str() {
            "user" => {
                // A plain string is the common case; content blocks only show
                // up on a turn carrying an attached image, and Anthropic wants
                // its own image block shape rather than OpenAI's image_url.
                if let Some(Value::Array(blocks)) = m.content.as_ref() {
                    for b in blocks {
                        match b.get("type").and_then(|t| t.as_str()) {
                            Some("text") => {
                                if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                                    if !t.is_empty() {
                                        push("user", json!({ "type": "text", "text": t }), &mut out);
                                    }
                                }
                            }
                            Some("image_url") => {
                                let url = b["image_url"]["url"].as_str().unwrap_or("");
                                if let Some((mime, data)) = parse_data_url(url) {
                                    push(
                                        "user",
                                        json!({
                                            "type": "image",
                                            "source": {
                                                "type": "base64",
                                                "media_type": mime,
                                                "data": data,
                                            }
                                        }),
                                        &mut out,
                                    );
                                }
                            }
                            _ => {}
                        }
                    }
                } else {
                    let text = m.text();
                    if !text.is_empty() {
                        push("user", json!({ "type": "text", "text": text }), &mut out);
                    }
                }
            }
            "assistant" => {
                if let Some(t) = m.content.as_ref().and_then(|c| c.as_str()) {
                    if !t.is_empty() {
                        push("assistant", json!({ "type": "text", "text": t }), &mut out);
                    }
                }
                if let Some(calls) = m.tool_calls.as_ref().and_then(|c| c.as_array()) {
                    for c in calls {
                        let input: Value =
                            serde_json::from_str(c["function"]["arguments"].as_str().unwrap_or("{}"))
                                .unwrap_or(json!({}));
                        push(
                            "assistant",
                            json!({
                                "type": "tool_use",
                                "id": c["id"],
                                "name": c["function"]["name"],
                                "input": input,
                            }),
                            &mut out,
                        );
                    }
                }
            }
            "tool" => {
                push(
                    "user",
                    json!({
                        "type": "tool_result",
                        "tool_use_id": m.tool_call_id.clone().unwrap_or_default(),
                        "content": m.text(),
                    }),
                    &mut out,
                );
            }
            _ => {}
        }
    }
    // An empty exchange would be rejected outright.
    if out.is_empty() {
        out.push(json!({ "role": "user", "content": [{"type": "text", "text": "(empty)"}] }));
    }
    out
}

// ---------------------------------------------------------------------------
// Shared SSE plumbing
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct AccTool {
    id: String,
    name: String,
    args: String,
}

/// Accumulates streamed assistant state across provider-specific parsers so
/// both engines share the same finalize step.
struct SseAccumulator {
    text: String,
    tools: BTreeMap<u64, AccTool>,
}

impl SseAccumulator {
    fn new() -> Self {
        Self { text: String::new(), tools: BTreeMap::new() }
    }

    fn finish(&self, on_event: &mut impl FnMut(StreamKind)) -> AssistantTurn {
        AssistantTurn {
            text: self.text.clone(),
            tools: finalize_tools(self.tools.clone(), on_event),
        }
    }
}

fn finalize_tools(
    tools: BTreeMap<u64, AccTool>,
    on_event: &mut impl FnMut(StreamKind),
) -> Vec<ToolUse> {
    tools
        .into_values()
        .filter(|t| !t.name.is_empty())
        .map(|t| {
            let input: Value =
                serde_json::from_str(if t.args.is_empty() { "{}" } else { &t.args })
                    .unwrap_or(json!({}));
            on_event(StreamKind::ToolUse { name: t.name.clone(), input: input.clone() });
            ToolUse { id: t.id, name: t.name, input }
        })
        .collect()
}

/// Reads the SSE byte stream, splitting on `\n\n` boundaries in byte space
/// (`\n` never appears inside a multi-byte UTF-8 sequence), feeding each event
/// block to the parser, and aborting instantly when `cancel` fires.
async fn consume_sse(
    res: reqwest::Response,
    cancel: &CancellationToken,
    mut handle: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    let mut buf: Vec<u8> = Vec::new();
    let mut byte_stream = res.bytes_stream();
    loop {
        let chunk = tokio::select! {
            c = byte_stream.next() => match c {
                Some(x) => x?,
                None => break,
            },
            _ = cancel.cancelled() => anyhow::bail!("cancelled"),
        };
        buf.extend_from_slice(&chunk);
        while let Some(i) = buf.windows(2).position(|w| w == b"\n\n") {
            let block: Vec<u8> = buf.drain(..i + 2).collect();
            handle(&String::from_utf8_lossy(&block))?;
        }
    }
    if !buf.iter().all(|b| b.is_ascii_whitespace()) {
        handle(&String::from_utf8_lossy(&buf))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings_with(default_model: &str) -> Settings {
        let mut s = Settings::default();
        s.default_model = default_model.into();
        s
    }

    #[test]
    fn legacy_ids_still_resolve() {
        assert_eq!(
            super::normalize_model("flash", &Settings::default()),
            "deepseek/deepseek-chat"
        );
        assert_eq!(
            super::normalize_model("", &settings_with("anthropic/claude-sonnet-4-5")),
            "anthropic/claude-sonnet-4-5"
        );
        assert_eq!(
            super::normalize_model("claude-haiku-4-5", &Settings::default()),
            "anthropic/claude-haiku-4-5"
        );
    }

    #[test]
    fn qualified_ids_pass_through() {
        assert_eq!(
            super::normalize_model("ollama/qwen2.5-coder", &Settings::default()),
            "ollama/qwen2.5-coder"
        );
    }

    #[test]
    fn context_limits_come_from_the_catalog() {
        assert_eq!(
            super::context_limit("anthropic/claude-sonnet-4-5", &Settings::default()),
            200_000
        );
        assert_eq!(
            super::context_limit("nope/nothing", &Settings::default()),
            128_000,
            "unknown models fall back to a sane window"
        );
    }

    #[test]
    fn endpoints_append_paths_by_kind() {
        let s = Settings::default();
        let ds = super::provider_def("deepseek").unwrap();
        assert_eq!(
            super::base_url_for(ds, &s),
            "https://api.deepseek.com/chat/completions"
        );
        let cl = super::provider_def("anthropic").unwrap();
        assert_eq!(super::base_url_for(cl, &s), "https://api.anthropic.com/v1/messages");
    }

    #[test]
    fn base_url_override_replaces_host_but_keeps_path_convention() {
        let mut s = Settings::default();
        s.providers.insert(
            "openai".into(),
            crate::settings::ProviderCfg {
                api_key: None,
                base_url: Some("http://my-proxy:9000/v7/".into()),
            },
        );
        let def = super::provider_def("openai").unwrap();
        assert_eq!(
            super::base_url_for(def, &s),
            "http://my-proxy:9000/v7/chat/completions"
        );
    }

    #[test]
    fn anthropic_history_merges_tool_results_and_alternates_roles() {
        let msgs = vec![
            Message::user_text("list files"),
            Message {
                role: "assistant".into(),
                content: Some(json!("checking")),
                tool_calls: Some(json!([{
                    "id": "t1",
                    "type": "function",
                    "function": { "name": "list_files", "arguments": "{}" }
                }])),
                tool_call_id: None,
            },
            Message {
                role: "tool".into(),
                content: Some(json!("a.rs\nb.rs")),
                tool_calls: None,
                tool_call_id: Some("t1".into()),
            },
            Message {
                role: "tool".into(),
                content: Some(json!("second result")),
                tool_calls: None,
                tool_call_id: Some("t1".into()),
            },
        ];
        let out = super::to_anthropic_messages(&msgs);
        assert_eq!(out.len(), 3, "two tool replies merge into one user turn");
        assert_eq!(out[1]["role"], "assistant");
        assert_eq!(out[1]["content"].as_array().unwrap().len(), 2);
        let merged = out[2]["content"].as_array().unwrap();
        assert_eq!(merged.len(), 2, "both tool_results land in the same user message");
        assert_eq!(merged[0]["type"], "tool_result");
    }

    #[test]
    fn anthropic_tool_input_parses_from_argument_strings() {
        let msgs = vec![Message {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(json!([{
                "id": "t9",
                "type": "function",
                "function": { "name": "read_file", "arguments": "{\"path\":\"a.rs\"}" }
            }])),
            tool_call_id: None,
        }];
        let out = super::to_anthropic_messages(&msgs);
        assert_eq!(out[0]["content"][0]["input"], json!({ "path": "a.rs" }));
    }

    /// An attached image: OpenAI's `image_url`/`text` blocks must translate to
    /// Anthropic's own `image`/`text` shape, base64 payload split from its
    /// media type — not get stringified whole as one text block (which used
    /// to dump the raw base64 at the model as if it were prose).
    #[test]
    fn image_attachment_translates_to_anthropics_block_shape() {
        let msg = Message::user_blocks(vec![
            json!({ "type": "text", "text": "use this logo" }),
            json!({ "type": "image_url", "image_url": { "url": "data:image/png;base64,QUJD" } }),
        ]);
        let out = super::to_anthropic_messages(&[msg]);
        assert_eq!(out.len(), 1, "one user turn");
        let blocks = out[0]["content"].as_array().unwrap();
        assert_eq!(blocks[0], json!({ "type": "text", "text": "use this logo" }));
        assert_eq!(
            blocks[1],
            json!({
                "type": "image",
                "source": { "type": "base64", "media_type": "image/png", "data": "QUJD" }
            })
        );
    }

    #[test]
    fn effort_clamps_to_three_levels() {
        assert_eq!(super::normalize_effort("HIGH"), "high");
        assert_eq!(super::normalize_effort(""), "medium");
        assert_eq!(super::normalize_effort("nonsense"), "medium");
    }

    #[test]
    fn groups_list_every_provider_with_configuration_flags() {
        let j = super::groups_json(&Settings::default());
        let groups = j["groups"].as_array().unwrap();
        assert!(groups.len() >= 12);
        let ollama = groups.iter().find(|g| g["id"] == "ollama").unwrap();
        assert_eq!(ollama["key_optional"], true);
        let anthropic = groups.iter().find(|g| g["id"] == "anthropic").unwrap();
        assert_eq!(anthropic["key_set"], false);
        assert_eq!(anthropic["kind"], "anthropic");
    }
}
