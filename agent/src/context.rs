const MAX_CHARS: usize = 60_000;

/// Clip large file content cleanly at a line boundary, preserving the code
/// and providing exact instructions on how to read the remaining lines.
pub fn clip_file(_path: &str, src: &str) -> String {
    if src.len() <= MAX_CHARS {
        return src.to_string();
    }
    let mut cut = MAX_CHARS;
    while cut > 0 && !src.is_char_boundary(cut) {
        cut -= 1;
    }
    let cut_pos = src[..cut].rfind('\n').unwrap_or(cut);
    let head = &src[..cut_pos];
    let lines_kept = head.lines().count();
    let total_lines = src.lines().count();
    format!(
        "{head}\n\n[… file truncated at line {lines_kept} of {total_lines} ({dropped} characters omitted). \
         Use read_file with offset={next_offset} and limit=1000 to read subsequent lines …]",
        dropped = src.len() - cut_pos,
        next_offset = lines_kept + 1,
    )
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

    #[test]
    fn clip_file_keeps_code_and_lines_intact() {
        let short = "fn main() {\n    println!(\"hello\");\n}\n";
        assert_eq!(clip_file("main.rs", short), short);

        let lines: Vec<String> = (1..=3000).map(|i| format!("let var_{i} = {i};")).collect();
        let long = lines.join("\n");
        let clipped = clip_file("large.rs", &long);
        assert!(clipped.contains("let var_1 = 1;"));
        assert!(clipped.contains("file truncated at line"));
        assert!(clipped.contains("read_file with offset="));
    }
}
