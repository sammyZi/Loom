use anyhow::{bail, Context, Result};
use ide_core::{FileNode, WorkspaceRoot};
use std::fs;
use std::path::Path;
const MAX_NODES: usize = 8_000;
const MAX_FILE_BYTES: u64 = 2_000_000;

pub fn tree(ws: &WorkspaceRoot) -> Result<FileNode> {
    let root = ws.root();
    let name = root
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned());
    let mut budget = MAX_NODES;
    Ok(FileNode {
        name,
        path: String::new(),
        is_dir: true,
        children: Some(collect_children(ws, root, &mut budget)?),
    })
}

/// `budget` is shared across the whole recursion so the node cap is global.
/// Per-level counts used to reset in every branch, letting deep trees blow
/// far past MAX_NODES.
fn collect_children(ws: &WorkspaceRoot, dir: &Path, budget: &mut usize) -> Result<Vec<FileNode>> {
    let mut out = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .with_context(|| format!("read {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| {
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        (!is_dir, e.file_name())
    });
    for ent in entries {
        if *budget == 0 {
            break;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        let path = ent.path();
        let is_dir = ent.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir && WorkspaceRoot::is_skipped_dir(&name) {
            continue;
        }
        *budget -= 1;
        let rel = ws.rel_to(&path);
        let children = if is_dir {
            Some(collect_children(ws, &path, budget)?)
        } else {
            None
        };
        out.push(FileNode {
            name,
            path: rel,
            is_dir,
            children,
        });
    }
    Ok(out)
}

pub fn read_file(ws: &WorkspaceRoot, rel: &str) -> Result<String> {
    let path = ws.resolve(rel)?;
    if path.is_dir() {
        bail!("is a directory");
    }
    let meta = fs::metadata(&path).context("stat")?;
    if meta.len() > MAX_FILE_BYTES {
        bail!("file too large ({} bytes)", meta.len());
    }
    fs::read_to_string(&path).context("read file")
}

pub fn write_file(ws: &WorkspaceRoot, rel: &str, content: &str) -> Result<()> {
    let path = ws.resolve(rel)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("mkdir")?;
    }
    fs::write(&path, content).context("write file")
}
