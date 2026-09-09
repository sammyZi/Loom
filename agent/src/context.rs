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

/// Ceiling on one tool result. Claude Code caps tool responses at ~25k tokens;
/// this is the same idea in bytes, applied to every provider.
pub const MAX_OUTPUT_CHARS: usize = 24_000;

/// Clip a tool result to something a context window can afford, keeping the
/// head and the tail. Build output is shaped that way on purpose: the command
/// and its early progress are at the top, the errors and the summary line are
/// at the bottom, and the middle is thousands of lines of "Compiling …".
///
/// Returns `s` untouched when it already fits, so small results cost nothing.
pub fn clip_output(s: &str) -> String {
    if s.len() <= MAX_OUTPUT_CHARS {
        return s.to_string();
    }
    // Two thirds head, one third tail: a failure usually announces itself at
    // the end, but the start is what says which command produced it.
    let head_max = MAX_OUTPUT_CHARS * 2 / 3;
    let tail_max = MAX_OUTPUT_CHARS - head_max;
    let head_end = floor_boundary(s, head_max);
    let tail_start = ceil_boundary(s, s.len() - tail_max);
    let dropped = tail_start - head_end;
    format!(
        "{}\n\n[… {dropped} characters of output omitted — rerun with a narrower command, or \
         grep the log, if you need the middle …]\n\n{}",
        &s[..head_end],
        &s[tail_start..]
    )
}

/// Largest char boundary at or below `at`.
fn floor_boundary(s: &str, at: usize) -> usize {
    let mut i = at.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Smallest char boundary at or above `at`.
fn ceil_boundary(s: &str, at: usize) -> usize {
    let mut i = at.min(s.len());
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

#[cfg(test)]
mod clip_tests {
    use super::*;

    #[test]
    fn short_output_is_untouched() {
        assert_eq!(clip_output("exit 0\nstdout:\nok"), "exit 0\nstdout:\nok");
    }

    #[test]
    fn long_output_keeps_both_ends_and_says_what_went() {
        let mut s = String::from("FIRST-LINE\n");
        s.push_str(&"filler\n".repeat(20_000));
        s.push_str("LAST-LINE");
        let out = clip_output(&s);
        assert!(out.len() < s.len(), "must actually shrink");
        assert!(out.starts_with("FIRST-LINE"), "head kept: {}", &out[..40]);
        assert!(out.ends_with("LAST-LINE"), "tail kept");
        assert!(out.contains("characters of output omitted"), "says what it dropped");
    }

    /// The cut must never land mid-character: String slicing panics there, and
    /// that used to take the whole run down on output with any Unicode in it.
    #[test]
    fn multibyte_output_does_not_panic() {
        let s = "é".repeat(MAX_OUTPUT_CHARS);
        let out = clip_output(&s);
        assert!(out.contains("omitted"));
    }
}
