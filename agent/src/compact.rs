//! Context compaction, modelled on opencode's two-stage approach:
//! 1. **Prune** — clear old tool outputs (the biggest dead weight), keeping a
//!    recent tail intact. Pure bookkeeping, no LLM call.
//! 2. **Summarize** — fold everything older than a preserved recent window
//!    into one structured handoff summary produced by a plain LLM call.
//!
//! Both stages respect the provider rule that a `tool` message may only exist
//! while the assistant message carrying its `tool_calls` stays in place.

use crate::provider::{self, Message};
use crate::settings::Settings;
use anyhow::Result;
use serde_json::json;
use tokio_util::sync::CancellationToken;

/// Compaction starts once this share of the model's real window is in use.
const TRIGGER_SHARE: f64 = 0.85;

/// Tool outputs newer than this many tokens (counted from the end of the
/// history) are always kept verbatim by the prune pass.
const PRUNE_PROTECT_TOKENS: u64 = 40_000;

const PRUNED_PLACEHOLDER: &str = "[old tool result cleared]";

/// Rough token estimate: ~4 characters per token for code-heavy text. Good
/// enough to decide *when* to compact; the provider's own usage report is the
/// accurate number and feeds back through StreamKind::Usage.
pub fn estimate_tokens(messages: &[Message]) -> u64 {
    let chars: usize = messages.iter().map(|m| m.preview().len()).sum();
    (chars as f64 / 4.0).ceil() as u64
}

/// Everything needed to run the summarizing LLM call.
pub struct CompactDeps<'a> {
    pub client: &'a reqwest::Client,
    pub model: String,
    pub settings: &'a Settings,
    pub cancel: &'a CancellationToken,
}

/// Returns true when any compaction happened, so the caller can surface it.
pub async fn compact(
    messages: &mut Vec<Message>,
    limit_tokens: u64,
    deps: &CompactDeps<'_>,
    mut notify: impl FnMut(&str),
) -> Result<bool> {
    let trigger = (limit_tokens as f64 * TRIGGER_SHARE) as u64;
    if estimate_tokens(messages) < trigger || messages.len() < 6 {
        return Ok(false);
    }
    notify("compacting context…");
    let pruned = prune_tool_outputs(messages, PRUNE_PROTECT_TOKENS);
    let mut did_something = pruned > 0;

    if estimate_tokens(messages) >= trigger {
        match summarize_pass(messages, limit_tokens, deps).await {
            Ok(true) => did_something = true,
            // No safe cut or summarizer failure: fall back to a silent trim so
            // an oversized request can never wedge the loop.
            _ => hard_trim(messages),
        }
    }
    Ok(did_something)
}

/// Clears outputs of tool messages beyond the protected recent tail. Returns
/// how many were cleared. Never touches structure — only message content.
pub fn prune_tool_outputs(messages: &mut [Message], protect_tail_tokens: u64) -> usize {
    let mut seen_from_end = 0u64;
    let mut cleared = 0usize;
    for m in messages.iter_mut().rev() {
        if m.role != "tool" {
            continue;
        }
        let len = m.preview().len() as u64 / 4;
        if seen_from_end < protect_tail_tokens {
            seen_from_end += len;
            continue;
        }
        if m.preview().contains(PRUNED_PLACEHOLDER) {
            continue; // already cleared on a previous pass
        }
        m.content = Some(json!(PRUNED_PLACEHOLDER));
        cleared += 1;
    }
    cleared
}

/// Index where the preserved recent window should start, or None when nothing
/// safely fits. The cut lands between turns and never inside a tool-call
/// group: a `tool` message is only legal while the assistant message carrying
/// its `tool_calls` is still present.
pub fn summarize_split(messages: &[Message], preserve_budget_tokens: u64) -> Option<usize> {
    if messages.len() < 4 {
        return None;
    }
    let mut size = 0u64;
    let mut split = messages.len();
    for i in (0..messages.len()).rev() {
        let m = &messages[i];
        let len = m.preview().len() as u64 / 4;
        if i != messages.len() - 1 && size + len > preserve_budget_tokens {
            break;
        }
        size += len;
        split = i;
    }
    // Walk forward past any tool messages orphaned by the cut.
    while split < messages.len() && messages[split].role == "tool" {
        split += 1;
    }
    // The assistant message right before the cut must not carry tool_calls
    // that would then dangle without their replies.
    while split > 0 && messages[split - 1].role == "assistant" && messages[split - 1].tool_calls.is_some()
    {
        split -= 1;
        while split > 0 && messages[split - 1].role == "tool" {
            split -= 1;
        }
        while split < messages.len() && messages[split].role == "tool" {
            split += 1;
        }
    }
    if split == 0 || split >= messages.len() {
        return None;
    }
    Some(split)
}

/// Serializes the head of the conversation for the summarizer, opencode-style:
/// role-tagged lines with long content clipped.
fn serialize(msgs: &[Message]) -> String {
    let mut s = String::new();
    for m in msgs {
        let clip: String = m.preview().chars().take(500).collect();
        match m.role.as_str() {
            "assistant" if m.tool_calls.is_some() => {
                s.push_str(&format!("- assistant ran tools: {clip}\n"));
            }
            role => s.push_str(&format!("- {role}: {clip}\n")),
        }
        if s.len() > 24_000 {
            s.push_str("…\n");
            break;
        }
    }
    s
}

const SUMMARIZER_SYSTEM: &str = "\
You compress a coding-agent conversation into a handoff summary for the next \
session. Reply with short sections exactly titled: Goal, Decisions, Changed files, \
Tool results worth keeping, Open items. Keep every file path and command name \
verbatim. Omit filler. Under 350 words.";

async fn summarize_pass(
    messages: &mut Vec<Message>,
    limit_tokens: u64,
    deps: &CompactDeps<'_>,
) -> Result<bool> {
    let preserve = ((limit_tokens as f64 * 0.25) as u64).clamp(2_000, 15_000);
    let Some(split) = summarize_split(messages, preserve) else {
        return Ok(false);
    };
    let head: Vec<Message> = messages.drain(..split).collect();

    let convo = serialize(&head);
    let prompt = format!(
        "Conversation to compress:\n\n{convo}\n\nProduce the handoff summary now."
    );
    let turn = provider::stream(
        deps.client,
        &deps.model,
        SUMMARIZER_SYSTEM,
        &[],
        &[Message::user_text(prompt)],
        "low",
        deps.settings,
        deps.cancel,
        |_| {},
    )
    .await?;
    // Put the history back rather than losing it when the model says nothing.
    if turn.text.trim().is_empty() {
        *messages = head;
        anyhow::bail!("summarizer returned nothing");
    }
    let note = format!(
        "[compacted earlier turns]\n{}",
        turn.text.trim()
    );
    messages.insert(0, Message::user_text(note));
    Ok(true)
}

/// Last-resort drop of oldest non-tool messages until under half the window.
/// Only reached when both prune and summarize failed to make room.
fn hard_trim(messages: &mut Vec<Message>) {
    let cap = 32_000_u64;
    while messages.len() > 2 && estimate_tokens(messages) > cap {
        let idx = messages
            .iter()
            .position(|m| m.role != "tool")
            .unwrap_or(messages.len() - 1);
        // Keep tool/assistant pairing intact: dropping an assistant that owns
        // following tool replies requires dropping those too.
        let owned = idx
            + 1
            + messages[idx + 1..]
                .iter()
                .take_while(|m| m.role == "tool")
                .count();
        messages.drain(..owned.max(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn big(role: &str, n_chars: usize) -> Message {
        Message {
            role: role.into(),
            content: Some(json!("x".repeat(n_chars))),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn assistant_with_calls(n_chars: usize) -> Message {
        Message {
            role: "assistant".into(),
            content: Some(json!("y".repeat(n_chars))),
            tool_calls: Some(json!([{ "id": "c1", "type": "function" }])),
            tool_call_id: None,
        }
    }

    fn tool_reply(n_chars: usize) -> Message {
        Message {
            role: "tool".into(),
            content: Some(json!("t".repeat(n_chars))),
            tool_calls: None,
            tool_call_id: Some("c1".into()),
        }
    }

    /// Every `tool` message must be preceded by an assistant message with
    /// tool_calls, allowing consecutive tool replies to the same turn.
    fn tool_messages_are_anchored(msgs: &[Message]) -> bool {
        let mut anchored = false;
        for m in msgs {
            match m.role.as_str() {
                "tool" if !anchored => return false,
                "tool" => {}
                "assistant" => anchored = m.tool_calls.is_some(),
                _ => anchored = false,
            }
        }
        true
    }

    #[test]
    fn prune_clears_only_the_old_tail() {
        let fresh_output = "t".repeat(160_000); // ~40k tokens
        let old_output = "o".repeat(80_000); // ~20k tokens
        let mut msgs = vec![
            big("user", 10),
            assistant_with_calls(10),
            Message {
                role: "tool".into(),
                content: Some(json!(old_output)),
                tool_calls: None,
                tool_call_id: Some("c1".into()),
            },
            assistant_with_calls(10),
            Message {
                role: "tool".into(),
                content: Some(json!(fresh_output)),
                tool_calls: None,
                tool_call_id: Some("c1".into()),
            },
        ];
        let cleared = prune_tool_outputs(&mut msgs, PRUNE_PROTECT_TOKENS);
        assert_eq!(cleared, 1, "only the older output is pruned");
        assert_eq!(
            msgs[2].content.as_ref().unwrap(),
            &json!(PRUNED_PLACEHOLDER),
            "old output replaced by placeholder"
        );
        assert!(msgs[4].preview().len() > 100_000, "recent tail untouched");

        // Idempotent: a second pass finds nothing left to clear.
        assert_eq!(prune_tool_outputs(&mut msgs, PRUNE_PROTECT_TOKENS), 0);
    }

    #[test]
    fn split_never_lands_inside_a_tool_group() {
        let msgs = vec![
            big("user", 40_000),
            assistant_with_calls(12_000),
            tool_reply(12_000),
            tool_reply(12_000),
            assistant_with_calls(12_000),
            tool_reply(12_000),
            big("user", 12_000),
        ];
        assert!(tool_messages_are_anchored(&msgs), "fixture malformed");
        // Budget small enough that the recent window cannot swallow the whole
        // conversation — the cut has to land somewhere.
        let split = summarize_split(&msgs, 20_000).expect("should find a split");
        assert!(tool_messages_are_anchored(&msgs[..split]), "head anchored");
        assert!(tool_messages_are_anchored(&msgs[split..]), "tail anchored");
        assert_eq!(split, 1, "cut lands right after the opening user turn");
    }

    #[test]
    fn split_refuses_tiny_histories() {
        let msgs = vec![big("user", 10), big("assistant", 10)];
        assert_eq!(summarize_split(&msgs, 5_000), None);
    }

    #[test]
    fn estimate_matches_chars_over_four() {
        // preview() is the JSON encoding, so a 400-char payload gains two quote
        // characters before dividing.
        let msgs = vec![big("user", 398)];
        assert_eq!(estimate_tokens(&msgs), 100);
    }

    #[test]
    fn hard_trim_keeps_pairs_together() {
        let mut msgs = vec![
            big("user", 90_000),
            assistant_with_calls(90_000),
            tool_reply(90_000),
            big("user", 10),
        ];
        hard_trim(&mut msgs);
        assert!(tool_messages_are_anchored(&msgs));
        assert_eq!(msgs.len(), 1, "everything but the final user msg goes");
    }
}
