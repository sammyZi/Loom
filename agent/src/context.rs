use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

const MAX_CHARS: usize = 24_000;

pub fn clip_file(path: &str, src: &str) -> String {
    if src.len() <= MAX_CHARS {
        return src.to_string();
    }
    if looks_rust(path) {
        if let Some(s) = rust_outline(src) {
            return s;
        }
    }
    let mut s = src.chars().take(MAX_CHARS).collect::<String>();
    s.push_str("\n\n[truncated]");
    s
}

fn looks_rust(path: &str) -> bool {
    path.ends_with(".rs")
}

fn rust_outline(src: &str) -> Option<String> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_rust::LANGUAGE.into()).ok()?;
    let tree = parser.parse(src, None)?;
    let q = Query::new(
        &tree_sitter_rust::LANGUAGE.into(),
        r#"
        (function_item name: (identifier) @name)
        (struct_item name: (type_identifier) @name)
        (enum_item name: (type_identifier) @name)
        (impl_item type: (type_identifier) @name)
        (mod_item name: (identifier) @name)
        "#,
    )
    .ok()?;
    let mut cursor = QueryCursor::new();
    let mut names = Vec::new();
    let mut matches = cursor.matches(&q, tree.root_node(), src.as_bytes());
    while let Some(m) = matches.next() {
        for cap in m.captures {
            if let Ok(t) = cap.node.utf8_text(src.as_bytes()) {
                let start = cap.node.start_position().row + 1;
                names.push(format!("L{start} {t}"));
            }
        }
    }
    if names.is_empty() {
        return None;
    }
    let mut out = String::from("[tree-sitter outline — file too large to send whole]\n");
    out.push_str(&names.join("\n"));
    if out.len() > MAX_CHARS {
        truncate_chars(&mut out, MAX_CHARS);
        out.push_str("\n[truncated]");
    }
    Some(out)
}

/// Cut to at most `max` bytes without splitting a multi-byte character.
/// `String::truncate` panics when the cut lands mid-character, which used to
/// kill the whole agent task on a Unicode-heavy file.
fn truncate_chars(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut idx = max;
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    s.truncate(idx);
}
