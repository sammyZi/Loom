use crate::anthropic::Message;
use serde_json::json;

const BUDGET: usize = 80_000;

pub fn compact(messages: &mut Vec<Message>) {
    let size: usize = messages.iter().map(msg_len).sum();
    if size <= BUDGET || messages.len() < 6 {
        return;
    }
    let keep = 4;
    if messages.len() <= keep + 1 {
        return;
    }
    let split = messages.len() - keep;
    let old: Vec<_> = messages.drain(..split).collect();
    let summary = summarize(&old);
    messages.insert(
        0,
        Message {
            role: "user".into(),
            content: json!([{
                "type": "text",
                "text": format!("[compacted earlier turns]\n{summary}")
            }]),
        },
    );
}

fn msg_len(m: &Message) -> usize {
    m.content.to_string().len()
}

fn summarize(msgs: &[Message]) -> String {
    let mut s = String::new();
    for m in msgs {
        let role = &m.role;
        let text = m.content.to_string();
        let clip: String = text.chars().take(400).collect();
        s.push_str(&format!("- {role}: {clip}\n"));
        if s.len() > 8_000 {
            s.push_str("…");
            break;
        }
    }
    s
}
