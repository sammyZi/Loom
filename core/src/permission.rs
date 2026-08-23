//! Per-tool permissions, opencode's model.
//!
//! Loom used to swap whole hardcoded tool registries per mode, which made
//! control binary: "Approve" mode deleted `run_command` outright and the agent
//! could only say it was unable to run anything. Here every call is matched
//! against ordered rules that answer allow / ask / deny, so `git status` can be
//! free while `git push` is refused and everything else asks.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    /// Run without interrupting the user.
    Allow,
    /// Raise an approval request and wait for the answer.
    Ask,
    /// Do not offer the tool to the model at all.
    Deny,
}

impl Default for Permission {
    fn default() -> Self {
        Self::Allow
    }
}

/// One rule: a tool pattern, an argument pattern, and the answer.
#[derive(Clone, Debug)]
struct Rule {
    tool: String,
    arg: String,
    perm: Permission,
}

/// What the config accepts for a tool: either one decision for every call, or a
/// map of argument patterns to decisions.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PermissionEntry {
    Flat(Permission),
    /// e.g. `{ "*": "ask", "git status": "allow", "git push": "deny" }`
    ByArgs(BTreeMap<String, Permission>),
}

/// Ordered rules; the **last** match wins, so a specific pattern written after
/// a broad one overrides it — the same precedence opencode documents.
#[derive(Clone, Debug, Default)]
pub struct PermissionSet {
    rules: Vec<Rule>,
}

impl PermissionSet {
    /// Nothing configured: everything is allowed. Presets narrow it from there.
    pub fn allow_all() -> Self {
        Self::default()
    }

    /// Build from config order. A BTreeMap would sort the keys and quietly
    /// change which rule is last, so callers pass a Vec and keep their order.
    pub fn from_entries(entries: Vec<(String, PermissionEntry)>) -> Self {
        let mut rules = Vec::new();
        for (tool, entry) in entries {
            match entry {
                PermissionEntry::Flat(perm) => rules.push(Rule {
                    tool,
                    arg: "*".into(),
                    perm,
                }),
                PermissionEntry::ByArgs(map) => {
                    // Within one tool, order the broad patterns first so an
                    // exact command still wins however the map was written.
                    let mut pairs: Vec<(String, Permission)> = map.into_iter().collect();
                    pairs.sort_by_key(|(pat, _)| specificity(pat));
                    for (arg, perm) in pairs {
                        rules.push(Rule {
                            tool: tool.clone(),
                            arg,
                            perm,
                        });
                    }
                }
            }
        }
        Self { rules }
    }

    /// Convenience for the built-in agents: one decision per tool.
    pub fn from_pairs(pairs: &[(&str, Permission)]) -> Self {
        Self::from_entries(
            pairs
                .iter()
                .map(|(t, p)| ((*t).to_string(), PermissionEntry::Flat(*p)))
                .collect(),
        )
    }

    /// Decide one call. `detail` is the command line for shell tools and the
    /// path for file tools; pass "" when the tool takes no meaningful subject.
    pub fn decide(&self, tool: &str, detail: &str) -> Permission {
        self.rules
            .iter()
            .filter(|r| glob_match(&r.tool, tool) && glob_match(&r.arg, detail))
            .next_back()
            .map(|r| r.perm)
            .unwrap_or(Permission::Allow)
    }

    /// True when the tool is worth offering to the model at all. A denied tool
    /// is left out of the schema entirely, so it is never called and never has
    /// to be explained away.
    pub fn offers(&self, tool: &str) -> bool {
        // Denied only if *every* rule touching this tool denies it: a tool with
        // `{"*": "deny", "git status": "allow"}` still has a usable call.
        let touching: Vec<&Rule> = self.rules.iter().filter(|r| glob_match(&r.tool, tool)).collect();
        if touching.is_empty() {
            return true;
        }
        touching.iter().any(|r| r.perm != Permission::Deny)
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }
}

/// Longer, wildcard-free patterns are more specific and sort later.
fn specificity(pattern: &str) -> (usize, usize) {
    let stars = pattern.matches('*').count();
    // fewer stars first? no: broad (more stars) first, so exact wins last.
    (usize::MAX - stars, pattern.len())
}

/// Glob with `*` meaning "any run of characters", anchored at both ends.
/// Case-insensitive: `Git Status` and `git status` are the same command.
pub fn glob_match(pattern: &str, subject: &str) -> bool {
    let p: Vec<char> = pattern.to_ascii_lowercase().chars().collect();
    let s: Vec<char> = subject.to_ascii_lowercase().chars().collect();
    // Classic two-pointer wildcard match: linear, no backtracking blowup on
    // patterns like `*a*a*a*`.
    let (mut pi, mut si) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);
    while si < s.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = si;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(pairs: &[(&str, &str, Permission)]) -> PermissionSet {
        PermissionSet::from_entries(
            pairs
                .iter()
                .map(|(tool, arg, perm)| {
                    let mut m = BTreeMap::new();
                    m.insert((*arg).to_string(), *perm);
                    ((*tool).to_string(), PermissionEntry::ByArgs(m))
                })
                .collect(),
        )
    }

    #[test]
    fn nothing_configured_allows_everything() {
        let p = PermissionSet::allow_all();
        assert_eq!(p.decide("bash", "rm -rf /"), Permission::Allow);
        assert!(p.offers("bash"));
    }

    #[test]
    fn a_flat_rule_covers_every_call_of_that_tool() {
        let p = PermissionSet::from_pairs(&[("edit", Permission::Deny)]);
        assert_eq!(p.decide("edit", "src/main.rs"), Permission::Deny);
        assert_eq!(p.decide("edit", ""), Permission::Deny);
        // and leaves other tools alone
        assert_eq!(p.decide("read", "src/main.rs"), Permission::Allow);
    }

    /// The headline case from opencode's docs: ask by default, but let a
    /// read-only command through and refuse a dangerous one outright.
    #[test]
    fn specific_command_patterns_override_the_catch_all() {
        let mut m = BTreeMap::new();
        m.insert("*".to_string(), Permission::Ask);
        m.insert("git status".to_string(), Permission::Allow);
        m.insert("git push".to_string(), Permission::Deny);
        let p = PermissionSet::from_entries(vec![("bash".into(), PermissionEntry::ByArgs(m))]);

        assert_eq!(p.decide("bash", "git status"), Permission::Allow);
        assert_eq!(p.decide("bash", "git push"), Permission::Deny);
        assert_eq!(p.decide("bash", "npm install"), Permission::Ask);
        // a tool that still has an allowed call is offered to the model
        assert!(p.offers("bash"));
    }

    #[test]
    fn wildcards_match_prefixes_and_infixes() {
        assert!(glob_match("git *", "git push --force"));
        assert!(glob_match("*install*", "npm install react"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("git *", "npm install"));
        // anchored: a pattern without stars must match the whole subject
        assert!(!glob_match("git", "git status"));
        assert!(glob_match("git", "GIT"));
    }

    #[test]
    fn a_fully_denied_tool_is_not_offered() {
        let p = set(&[("bash", "*", Permission::Deny)]);
        assert!(!p.offers("bash"));
        assert!(p.offers("read"));
    }

    /// Plan agent: reads freely, never edits, asks before shelling out.
    #[test]
    fn plan_preset_behaves_like_opencodes_plan_agent() {
        let p = PermissionSet::from_pairs(&[
            ("edit", Permission::Deny),
            ("write", Permission::Deny),
            ("bash", Permission::Ask),
        ]);
        assert_eq!(p.decide("read", "a.rs"), Permission::Allow);
        assert_eq!(p.decide("edit", "a.rs"), Permission::Deny);
        assert_eq!(p.decide("bash", "ls"), Permission::Ask);
        assert!(!p.offers("edit"));
        assert!(p.offers("bash"), "ask still offers the tool, it just prompts");
    }

    #[test]
    fn tool_patterns_can_wildcard_too() {
        // how an MCP server's whole namespace gets gated in one line
        let p = set(&[("mymcp_*", "*", Permission::Deny)]);
        assert!(!p.offers("mymcp_query"));
        assert!(p.offers("read"));
    }
}
