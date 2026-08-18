use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const API: &str = "https://api.anthropic.com/v1/messages";

pub fn model() -> String {
    std::env::var("ANTHROPIC_MODEL").unwrap_or_else(|_| "claude-sonnet-4-20250514".into())
}

pub fn api_key() -> Result<String> {
    std::env::var("ANTHROPIC_API_KEY").context("ANTHROPIC_API_KEY is not set")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: Value,
}

#[derive(Debug, Clone)]
pub enum StreamKind {
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

pub async fn stream(
    client: &reqwest::Client,
    system: &str,
    tools: &[Value],
    messages: &[Message],
    mut on_event: impl FnMut(StreamKind),
) -> Result<AssistantTurn> {
    let key = api_key()?;
    let body = json!({
        "model": model(),
        "max_tokens": 8192,
        "stream": true,
        "system": [{
            "type": "text",
            "text": system,
            "cache_control": { "type": "ephemeral" }
        }],
        "tools": tools.iter().map(|t| {
            let mut t = t.clone();
            t["cache_control"] = json!({ "type": "ephemeral" });
            t
        }).collect::<Vec<_>>(),
        "messages": messages,
    });

    let res = client
        .post(API)
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("Anthropic {status}: {text}");
    }

    let mut text_acc = String::new();
    let mut tools_acc: Vec<ToolUse> = Vec::new();
    let mut cur_tool: Option<ToolUse> = None;
    let mut json_acc = String::new();
    let mut buf = String::new();
    let mut byte_stream = res.bytes_stream();

    while let Some(chunk) = byte_stream.next().await {
        buf.push_str(&String::from_utf8_lossy(&chunk?));
        while let Some(i) = buf.find("\n\n") {
            let block = buf[..i].to_string();
            buf = buf[i + 2..].to_string();
            handle_block(
                &block,
                &mut text_acc,
                &mut tools_acc,
                &mut cur_tool,
                &mut json_acc,
                &mut on_event,
            )?;
        }
    }
    if !buf.trim().is_empty() {
        handle_block(
            &buf,
            &mut text_acc,
            &mut tools_acc,
            &mut cur_tool,
            &mut json_acc,
            &mut on_event,
        )?;
    }
    Ok(AssistantTurn {
        text: text_acc,
        tools: tools_acc,
    })
}

fn handle_block(
    block: &str,
    text_acc: &mut String,
    tools_acc: &mut Vec<ToolUse>,
    cur_tool: &mut Option<ToolUse>,
    json_acc: &mut String,
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
    match v["type"].as_str().unwrap_or("") {
        "content_block_start" => {
            let b = &v["content_block"];
            if b["type"] == "tool_use" {
                *cur_tool = Some(ToolUse {
                    id: b["id"].as_str().unwrap_or("").into(),
                    name: b["name"].as_str().unwrap_or("").into(),
                    input: json!({}),
                });
                json_acc.clear();
            }
        }
        "content_block_delta" => {
            let d = &v["delta"];
            if let Some(t) = d["text"].as_str() {
                text_acc.push_str(t);
                on_event(StreamKind::TextDelta(t.to_string()));
            }
            if let Some(pj) = d["partial_json"].as_str() {
                json_acc.push_str(pj);
            }
        }
        "content_block_stop" => {
            if let Some(mut t) = cur_tool.take() {
                t.input = serde_json::from_str(json_acc).unwrap_or(json!({}));
                on_event(StreamKind::ToolUse {
                    name: t.name.clone(),
                    input: t.input.clone(),
                });
                tools_acc.push(t);
            }
        }
        "error" => {
            let msg = v["error"]["message"]
                .as_str()
                .unwrap_or("api error")
                .to_string();
            on_event(StreamKind::Error(msg.clone()));
            anyhow::bail!(msg);
        }
        _ => {}
    }
    Ok(())
}
