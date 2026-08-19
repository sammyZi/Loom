use crate::deepseek::Message;

const BUDGET: usize = 80_000;

pub fn compact(messages: &mut Vec<Message>) {
    let size: usize = messages.iter().map(|m| m.preview().len()).sum();
    if size <= BUDGET || messages.len() < 6 {
        return;
    }
    let keep = 4;
    if messages.len() <= keep + 1 {
        return;
    }
    let mut split = messages.len() - keep;
    // Never cut inside a tool-call group. A `tool` message is only legal when the
    // assistant message carrying its tool_calls is still present, and the provider
    // rejects the entire request otherwise:
    //   "Messages with role 'tool' must be a response to a preceding message with 'tool_calls'"
    while split < messages.len() && messages[split].role == "tool" {
        split += 1;
    }
    if split >= messages.len() {
        return; // nothing safe to drop this round
    }
    let old: Vec<_> = messages.drain(..split).collect();
    let summary = summarize(&old);
    messages.insert(0, Message::user_text(format!("[compacted earlier turns]\n{summary}")));
}

fn summarize(msgs: &[Message]) -> String {
    let mut s = String::new();
    for m in msgs {
        let clip: String = m.preview().chars().take(400).collect();
        s.push_str(&format!("- {}: {clip}\n", m.role));
        if s.len() > 8_000 {
            s.push_str("…");
            break;
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn big(role: &str, n: usize) -> Message {
        Message {
            role: role.into(),
            content: Some(json!("x".repeat(n))),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    fn assistant_with_calls(n: usize) -> Message {
        Message {
            role: "assistant".into(),
            content: None,
            tool_calls: Some(json!([{ "id": "c1", "type": "function" }])),
            tool_call_id: Some("c1".into()),
        }
        .tap(n)
    }

    trait Tap {
        fn tap(self, n: usize) -> Self;
    }
    impl Tap for Message {
        fn tap(mut self, n: usize) -> Self {
            self.tool_calls = Some(json!([{ "id": "c1", "args": "y".repeat(n) }]));
            self
        }
    }

    fn tool_reply(n: usize) -> Message {
        Message {
            role: "tool".into(),
            content: Some(json!("t".repeat(n))),
            tool_calls: None,
            tool_call_id: Some("c1".into()),
        }
    }

    /// Every `tool` message must be preceded by an assistant message with tool_calls,
    /// allowing consecutive tool replies to the same assistant turn.
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
    fn does_not_split_a_tool_group() {
        // A history whose naive "keep the last 4" cut lands mid tool-group.
        let mut msgs = vec![big("user", 40_000)];
        for _ in 0..4 {
            msgs.push(assistant_with_calls(12_000));
            msgs.push(tool_reply(12_000));
            msgs.push(tool_reply(12_000));
        }
        assert!(tool_messages_are_anchored(&msgs), "fixture itself is malformed");

        compact(&mut msgs);

        assert!(
            tool_messages_are_anchored(&msgs),
            "compaction left a tool message with no tool_calls before it: {:?}",
            msgs.iter().map(|m| m.role.clone()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn leaves_short_histories_alone() {
        let mut msgs = vec![big("user", 10), big("assistant", 10)];
        let before = msgs.len();
        compact(&mut msgs);
        assert_eq!(msgs.len(), before);
    }

    #[test]
    fn still_shrinks_when_it_can() {
        let mut msgs = vec![big("user", 30_000)];
        for _ in 0..6 {
            msgs.push(big("assistant", 15_000));
            msgs.push(big("user", 15_000));
        }
        let before = msgs.len();
        compact(&mut msgs);
        assert!(msgs.len() < before, "compaction should have dropped something");
        assert_eq!(msgs[0].role, "user", "summary is inserted as a user message");
    }
}
