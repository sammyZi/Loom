use anyhow::{bail, Context, Result};
use core::{GitFileStatus, GitStatus, WorkspaceRoot};
use git2::{DiffOptions, Repository, StatusOptions};

pub fn status(ws: &WorkspaceRoot) -> Result<GitStatus> {
    let repo = open(ws)?;
    let head = repo.head().ok();
    let branch = head
        .as_ref()
        .and_then(|h| h.shorthand())
        .unwrap_or("HEAD")
        .to_string();

    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .exclude_submodules(true);

    let statuses = repo.statuses(Some(&mut opts))?;
    let mut files = Vec::new();
    for entry in statuses.iter() {
        let path = entry.path().unwrap_or("").replace('\\', "/");
        let s = entry.status();
        let label = if s.is_conflicted() {
            "conflict"
        } else if s.is_wt_new() || s.is_index_new() {
            "untracked"
        } else if s.is_wt_deleted() || s.is_index_deleted() {
            "deleted"
        } else if s.is_wt_renamed() || s.is_index_renamed() {
            "renamed"
        } else if s.is_wt_modified() || s.is_index_modified() {
            "modified"
        } else {
            continue;
        };
        files.push(GitFileStatus {
            path,
            status: label.into(),
        });
    }
    Ok(GitStatus { branch, files })
}

pub fn diff(ws: &WorkspaceRoot, rel: Option<&str>) -> Result<String> {
    let repo = open(ws)?;
    let mut opts = DiffOptions::new();
    if let Some(p) = rel {
        opts.pathspec(p);
    }
    let diff = repo.diff_index_to_workdir(None, Some(&mut opts))?;
    let mut buf = String::new();
    diff.print(git2::DiffFormat::Patch, |_d, _h, line| {
        let origin = line.origin();
        if matches!(origin, '+' | '-' | ' ' | '@' | '\\') {
            buf.push(origin);
        }
        buf.push_str(std::str::from_utf8(line.content()).unwrap_or(""));
        true
    })?;
    if buf.is_empty() {
        // also include staged vs HEAD
        if let Ok(head) = repo.head().and_then(|h| h.peel_to_tree()) {
            let diff = repo.diff_tree_to_index(Some(&head), None, None)?;
            diff.print(git2::DiffFormat::Patch, |_d, _h, line| {
                let origin = line.origin();
                if matches!(origin, '+' | '-' | ' ' | '@' | '\\') {
                    buf.push(origin);
                }
                buf.push_str(std::str::from_utf8(line.content()).unwrap_or(""));
                true
            })?;
        }
    }
    Ok(buf)
}

pub fn commit(ws: &WorkspaceRoot, message: &str) -> Result<String> {
    let repo = open(ws)?;
    if message.trim().is_empty() {
        bail!("empty commit message");
    }
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;
    let sig = repo
        .signature()
        .or_else(|_| git2::Signature::now("ide-ai", "ide-ai@local"))?;
    let parent = repo.head().ok().and_then(|h| h.peel_to_commit().ok());
    let parents: Vec<&git2::Commit> = parent.as_ref().into_iter().collect();
    let oid = repo.commit(Some("HEAD"), &sig, &sig, message.trim(), &tree, &parents)?;
    Ok(oid.to_string())
}

fn open(ws: &WorkspaceRoot) -> Result<Repository> {
    Repository::discover(ws.root()).context("not a git repository")
}
