use anyhow::{Context, Result};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct BrowseEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

pub fn roots() -> Vec<BrowseEntry> {
    let mut out = Vec::new();
    #[cfg(windows)]
    {
        for c in b'A'..=b'Z' {
            let p = format!("{}:\\", c as char);
            if Path::new(&p).is_dir() {
                out.push(BrowseEntry {
                    name: p.clone(),
                    path: p,
                    is_dir: true,
                });
            }
        }
    }
    #[cfg(not(windows))]
    {
        out.push(BrowseEntry {
            name: "/".into(),
            path: "/".into(),
            is_dir: true,
        });
    }
    if out.is_empty() {
        if let Ok(home) = std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")) {
            out.push(BrowseEntry {
                name: home.clone(),
                path: home,
                is_dir: true,
            });
        }
    }
    out
}

pub fn list_dir(path: Option<&str>) -> Result<(String, Option<String>, Vec<BrowseEntry>)> {
    let current = match path {
        None | Some("") => {
            return Ok((String::new(), None, roots()));
        }
        Some(p) => dunce::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p)),
    };
    if !current.is_dir() {
        anyhow::bail!("not a directory");
    }
    let shown = current.to_string_lossy().into_owned();
    let parent = current
        .parent()
        .map(|p| p.to_string_lossy().into_owned());
    let mut entries = Vec::new();
    let mut read = fs::read_dir(&current).with_context(|| format!("read {}", current.display()))?;
    for ent in read.by_ref() {
        let Ok(ent) = ent else { continue };
        let Ok(ft) = ent.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }
        let name = ent.file_name().to_string_lossy().into_owned();
        if name == "." || name == ".." {
            continue;
        }
        entries.push(BrowseEntry {
            path: ent.path().to_string_lossy().into_owned(),
            name,
            is_dir: true,
        });
        if entries.len() > 800 {
            break;
        }
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok((shown, parent, entries))
}
