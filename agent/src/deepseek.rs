use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const API: &str = "https://api.deepseek.com/chat/completions";

pub const FLASH: &str = "deepseek-v4-flash";
pub const PRO: &str = "deepseek-v4-pro";

pub fn catalog() -> Vec<Value> {
    vec![
        json!({ "id": FLASH, "label": "V4 Flash", "hint": "Fast" }),
        json!({ "id": PRO, "label": "V4 Pro", "hint": "Stronger coding" }),
    ]
}

pub fn normalize_model(s: &str) -> &'static str {
    match s.trim() {
        "deepseek-v4-flash" | "flash" | "v4-flash" => FLASH,
        _ => PRO,
    }
}

pub fn api_key() -> Result<String> {
    std::env::var("DEEPSEEK_API_KEY").context("DEEPSEEK_API_KEY is not set")
}

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

    pub fn preview(&self) -> String {
        self.content
            .as_ref()
            .map(|c| c.to_string())
            .or_else(|| self.tool_calls.as_ref().map(|c| c.to_string()))
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub enum StreamKind {
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

pub async fn stream(
    client: &reqwest::Client,
    model: &str,
    system: &str,
    tools: &[Value],
    messages: &[Message],
    mut on_event: impl FnMut(StreamKind),
) -> Result<AssistantTurn> {
    let model = normalize_model(model);
    let mut full_msgs = vec![Message {
        role: "system".into(),
        content: Some(Value::String(system.to_string())),
        tool_calls: None,
        tool_call_id: None,
    }];
    full_msgs.extend(messages.iter().cloned());

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

    let effort = if model == FLASH { "low" } else { "high" };
    let mut body = json!({
        "model": model,
        "messages": full_msgs,
        "stream": true,
        "temperature": if openai_tools.is_empty() { 0.6 } else { 0.2 },
        "max_tokens": 8192,
        "reasoning_effort": if openai_tools.is_empty() { "low" } else { effort },
    });
    if !openai_tools.is_empty() {
        body["tools"] = json!(openai_tools);
        body["tool_choice"] = json!("auto");
        body["thinking"] = json!({ "type": "enabled" });
    }

    let res = client
        .post(API)
        .header("authorization", format!("Bearer {}", api_key()?))
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !res.status().is_success() {
        let status = res.status();
        let text = res.text().await.unwrap_or_default();
        anyhow::bail!("DeepSeek {status}: {text}");
    }

    let mut text_acc = String::new();
    let mut tools_map: BTreeMap<u64, AccTool> = BTreeMap::new();
    let mut buf = String::new();
    let mut byte_stream = res.bytes_stream();

    while let Some(chunk) = byte_stream.next().await {
        buf.push_str(&String::from_utf8_lossy(&chunk?));
        while let Some(i) = buf.find("\n\n") {
            let block = buf[..i].to_string();
            buf = buf[i + 2..].to_string();
            handle_block(&block, &mut text_acc, &mut tools_map, &mut on_event)?;
        }
    }
    if !buf.trim().is_empty() {
        handle_block(&buf, &mut text_acc, &mut tools_map, &mut on_event)?;
    }

    let mut tools_acc = Vec::new();
    for t in tools_map.into_values() {
        let input = serde_json::from_str(&t.args).unwrap_or(json!({}));
        on_event(StreamKind::ToolUse {
            name: t.name.clone(),
            input: input.clone(),
        });
        tools_acc.push(ToolUse {
            id: t.id,
            name: t.name,
            input,
        });
    }

    Ok(AssistantTurn {
        text: text_acc,
        tools: tools_acc,
    })
}

struct AccTool {
    id: String,
    name: String,
    args: String,
}

fn handle_block(
    block: &str,
    text_acc: &mut String,
    tools_map: &mut BTreeMap<u64, AccTool>,
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
    let Some(choice) = v["choices"].as_array().and_then(|a| a.first()) else {
        return Ok(());
    };
    let delta = &choice["delta"];
    if let Some(t) = delta["reasoning_content"].as_str() {
        on_event(StreamKind::ThinkDelta(t.to_string()));
    }
    if let Some(t) = delta["content"].as_str() {
        text_acc.push_str(t);
        on_event(StreamKind::TextDelta(t.to_string()));
    }
    if let Some(calls) = delta["tool_calls"].as_array() {
        for call in calls {
            let idx = call["index"].as_u64().unwrap_or(0);
            let entry = tools_map.entry(idx).or_insert_with(|| AccTool {
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

#[cfg(test)]
mod tests {
    #[test]
    fn models_are_v4_flash_and_pro() {
        assert_eq!(super::normalize_model("flash"), super::FLASH);
        assert_eq!(super::normalize_model("deepseek-v4-pro"), super::PRO);
        assert_eq!(super::normalize_model(""), super::PRO);
        assert_eq!(super::catalog().len(), 2);
    }
}
