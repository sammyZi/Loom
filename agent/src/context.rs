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
    let matches: Vec<_> = cursor.matches(&q, tree.root_node(), src.as_bytes()).collect();
    for m in matches {
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
        out.truncate(MAX_CHARS);
        out.push_str("\n[truncated]");
    }
    Some(out)
}
