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
    let split = messages.len() - keep;
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
