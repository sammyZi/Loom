use ide_core::WorkspaceRoot;
use std::path::Path;

/// Which toolchain the open folder actually uses. The agent used to be told
/// "cargo check / cargo test" no matter what was open, so on a JS or Python
/// project it concluded the repo was Rust and invented Rust files.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stack {
    Cargo,
    Node,
    Python,
    Go,
    Unknown,
}

impl Stack {
    pub fn detect(ws: &WorkspaceRoot) -> Self {
        let r = ws.root();
        if r.join("Cargo.toml").exists() {
            Self::Cargo
        } else if r.join("package.json").exists() {
            Self::Node
        } else if r.join("go.mod").exists() {
            Self::Go
        } else if r.join("pyproject.toml").exists()
            || r.join("requirements.txt").exists()
            || r.join("setup.py").exists()
        {
            Self::Python
        } else {
            Self::Unknown
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Cargo => "Rust (Cargo)",
            Self::Node => "JavaScript/TypeScript (npm)",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::Unknown => "unknown",
        }
    }
}

/// Directories that would drown the listing and teach the agent nothing.
pub const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".expo",
    "Pods",
    "vendor",
    "__pycache__",
    ".venv",
    "venv",
    ".gradle",
    ".idea",
    ".ide-ai-tmp",
    "DerivedData",
];

fn skipped(name: &str) -> bool {
    SKIP_DIRS.contains(&name)
}

/// Relative paths under `root`, breadth-first, capped at `limit`.
pub fn list_files(root: &Path, sub: Option<&str>, limit: usize) -> Vec<String> {
    let start = match sub.map(str::trim).filter(|s| !s.is_empty() && *s != ".") {
        Some(s) => root.join(s),
        None => root.to_path_buf(),
    };
    let mut out = Vec::new();
    let mut queue = vec![start];

    while let Some(dir) = queue.pop() {
        if out.len() >= limit {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        names.sort_by_key(|e| e.file_name());
        for e in names {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') && name != ".env.example" {
                continue;
            }
            let path = e.path();
            let is_dir = path.is_dir();
            if is_dir && skipped(&name) {
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if is_dir {
                out.push(format!("{rel}/"));
                queue.push(path);
            } else {
                out.push(rel);
            }
            if out.len() >= limit {
                break;
            }
        }
    }
    out.sort();
    out
}

/// A short orientation block for the system prompt: what this folder actually is.
pub fn summary(ws: &WorkspaceRoot) -> String {
    let stack = Stack::detect(ws);
    let top = list_files(ws.root(), None, 60);
    let shown: Vec<_> = top.iter().take(40).cloned().collect();
    format!(
        "Open folder: {}\nDetected stack: {}\nTop-level contents:\n{}\n\
         Trust this listing over any assumption about the language. Use list_files to \
         discover more before claiming a file exists; never describe a file you have not read.",
        ws.root().display(),
        stack.label(),
        shown.join("\n")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ide-ai-proj-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn detects_node_over_nothing() {
        let d = tmp("node");
        std::fs::write(d.join("package.json"), "{}").unwrap();
        let ws = WorkspaceRoot::open(&d).unwrap();
        assert_eq!(Stack::detect(&ws), Stack::Node);
        assert!(Stack::detect(&ws).label().contains("npm"));
    }

    #[test]
    fn cargo_wins_when_both_present() {
        let d = tmp("both");
        std::fs::write(d.join("Cargo.toml"), "").unwrap();
        std::fs::write(d.join("package.json"), "{}").unwrap();
        let ws = WorkspaceRoot::open(&d).unwrap();
        assert_eq!(Stack::detect(&ws), Stack::Cargo);
    }

    #[test]
    fn unknown_when_no_manifest() {
        let d = tmp("bare");
        std::fs::write(d.join("notes.txt"), "hi").unwrap();
        let ws = WorkspaceRoot::open(&d).unwrap();
        assert_eq!(Stack::detect(&ws), Stack::Unknown);
    }

    #[test]
    fn listing_skips_heavy_dirs_and_finds_real_files() {
        let d = tmp("list");
        std::fs::create_dir_all(d.join("node_modules/react")).unwrap();
        std::fs::write(d.join("node_modules/react/index.js"), "x").unwrap();
        std::fs::create_dir_all(d.join("src/screens")).unwrap();
        std::fs::write(d.join("src/screens/Home.tsx"), "x").unwrap();
        std::fs::write(d.join("App.tsx"), "x").unwrap();

        let files = list_files(&d, None, 100);
        assert!(files.iter().any(|f| f == "App.tsx"), "{files:?}");
        assert!(files.iter().any(|f| f == "src/screens/Home.tsx"), "{files:?}");
        assert!(
            !files.iter().any(|f| f.contains("node_modules")),
            "node_modules must be skipped: {files:?}"
        );
    }

    #[test]
    fn listing_respects_limit() {
        let d = tmp("limit");
        for i in 0..50 {
            std::fs::write(d.join(format!("f{i}.txt")), "x").unwrap();
        }
        assert!(list_files(&d, None, 10).len() <= 10);
    }
}
