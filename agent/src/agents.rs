//! Agent definitions, opencode's model.
//!
//! An agent is a name, a system prompt, and a permission set — not a hardcoded
//! tool registry. Two kinds: *primary* agents the user talks to directly and
//! cycles through, and *subagents* a primary agent delegates to (or the user
//! summons with `@name`).
//!
//! Loom's old `Mode` enum is now just a preset that selects one of these.

use ide_core::{Permission, PermissionSet};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AgentMode {
    /// Talked to directly; appears in the composer's agent picker.
    Primary,
    /// Delegated to; appears in `@` autocomplete, not the picker.
    Subagent,
    /// Both.
    All,
}

#[derive(Clone)]
pub struct AgentDef {
    pub id: &'static str,
    pub label: &'static str,
    /// Shown in the picker and given to the model when it chooses a subagent.
    pub description: &'static str,
    pub mode: AgentMode,
    /// Appended to the shared orientation block.
    pub prompt: &'static str,
    /// Cap on agentic iterations before the model must answer in prose.
    pub steps: u32,
    /// Lower is more focused. None leaves the provider default alone.
    pub temperature: Option<f32>,
}

impl AgentDef {
    /// What this agent may do. Built from scratch each call so a caller can
    /// layer user config on top without mutating the static definition.
    pub fn permissions(&self) -> PermissionSet {
        match self.id {
            // Everything on. The default; what "just do it" means.
            "build" => PermissionSet::allow_all(),

            // Analysis only. Reads anything, changes nothing, and asks before
            // shelling out — opencode's Plan agent exactly.
            "plan" => PermissionSet::from_pairs(&[
                ("edit_file", Permission::Deny),
                ("write_file", Permission::Deny),
                ("run_command", Permission::Ask),
            ]),

            // Read-only codebase search. No shell at all: this one exists to be
            // cheap and safe to run in parallel.
            "explore" => PermissionSet::from_pairs(&[
                ("edit_file", Permission::Deny),
                ("write_file", Permission::Deny),
                ("run_command", Permission::Deny),
                ("web_search", Permission::Deny),
                ("web_fetch", Permission::Deny),
            ]),

            // Read-only *outward*: docs and dependency research, no repo edits.
            "scout" => PermissionSet::from_pairs(&[
                ("edit_file", Permission::Deny),
                ("write_file", Permission::Deny),
                ("run_command", Permission::Deny),
            ]),

            // Full tools, for delegated multi-step work.
            "general" => PermissionSet::allow_all(),

            _ => PermissionSet::allow_all(),
        }
    }
}

pub static AGENTS: &[AgentDef] = &[
    AgentDef {
        id: "build",
        label: "Build",
        description: "Full access. Writes code, runs commands, starts servers.",
        mode: AgentMode::Primary,
        prompt: "You are the build agent: you carry the task through to a working result. \
Make the change, run what proves it, and report what happened. Do not hand work back to \
the user that you could have done yourself.",
        steps: 40,
        temperature: None,
    },
    AgentDef {
        id: "plan",
        label: "Plan",
        description: "Analyses and proposes. Never edits; asks before running anything.",
        mode: AgentMode::Primary,
        prompt: "You are the plan agent. Read what you need, then give a short numbered plan \
of the exact changes and commands. You cannot edit files. Say what you would do, not what \
you are unable to do — the user switches to Build to have it carried out.",
        steps: 20,
        temperature: Some(0.2),
    },
    AgentDef {
        id: "explore",
        label: "Explore",
        description: "Read-only codebase search. Finds where things live.",
        mode: AgentMode::Subagent,
        prompt: "You are the explore agent. Locate code and report exact paths with line \
numbers and a one-line note on each. Search by content with search_files rather than \
reading files one at a time. You cannot edit, write, or run anything. Answer only what \
was asked — no plan, no opinion, no summary of the repo.",
        steps: 15,
        temperature: Some(0.1),
    },
    AgentDef {
        id: "scout",
        label: "Scout",
        description: "Read-only research: external docs, dependencies, APIs.",
        mode: AgentMode::Subagent,
        prompt: "You are the scout agent. Answer from official documentation you actually \
fetched, quoting the URL. Prefer primary sources over blog posts and never invent a link. \
You cannot edit files or run commands. If the docs do not answer it, say so plainly.",
        steps: 15,
        temperature: Some(0.1),
    },
    AgentDef {
        id: "general",
        label: "General",
        description: "Full tools, for a self-contained delegated task.",
        mode: AgentMode::Subagent,
        prompt: "You are the general subagent, handed one self-contained task. Complete it \
and return a short result. The parent agent sees only your final message, so it must stand \
on its own — no references to steps it cannot see.",
        steps: 30,
        temperature: None,
    },
];

pub fn agent_def(id: &str) -> Option<&'static AgentDef> {
    let id = id.trim().to_ascii_lowercase();
    AGENTS.iter().find(|a| a.id == id)
}

/// Agents the user can pick in the composer.
pub fn primary_agents() -> impl Iterator<Item = &'static AgentDef> {
    AGENTS
        .iter()
        .filter(|a| matches!(a.mode, AgentMode::Primary | AgentMode::All))
}

/// Agents a primary agent may delegate to.
pub fn subagents() -> impl Iterator<Item = &'static AgentDef> {
    AGENTS
        .iter()
        .filter(|a| matches!(a.mode, AgentMode::Subagent | AgentMode::All))
}

/// JSON for the composer's agent picker.
pub fn agents_json() -> serde_json::Value {
    serde_json::json!({
        "agents": AGENTS
            .iter()
            .map(|a| serde_json::json!({
                "id": a.id,
                "label": a.label,
                "description": a.description,
                "mode": match a.mode {
                    AgentMode::Primary => "primary",
                    AgentMode::Subagent => "subagent",
                    AgentMode::All => "all",
                },
            }))
            .collect::<Vec<_>>(),
        "default": "build",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_agent_is_reachable_by_id() {
        for a in AGENTS {
            assert!(agent_def(a.id).is_some(), "{} not found", a.id);
        }
        assert!(agent_def("BUILD").is_some(), "lookup is case-insensitive");
        assert!(agent_def("nope").is_none());
    }

    #[test]
    fn the_picker_offers_primaries_and_delegation_offers_subagents() {
        let primary: Vec<&str> = primary_agents().map(|a| a.id).collect();
        assert_eq!(primary, vec!["build", "plan"]);
        let subs: Vec<&str> = subagents().map(|a| a.id).collect();
        assert_eq!(subs, vec!["explore", "scout", "general"]);
    }

    /// The whole point of the rewrite: read-only agents must not be able to
    /// reach the shell or the editor, and Build must not be restricted.
    #[test]
    fn read_only_agents_cannot_edit_or_shell_out() {
        for id in ["explore", "scout"] {
            let p = agent_def(id).unwrap().permissions();
            assert!(!p.offers("edit_file"), "{id} should not edit");
            assert!(!p.offers("write_file"), "{id} should not write");
            assert!(!p.offers("run_command"), "{id} should not run commands");
            assert!(p.offers("read_file"), "{id} must still read");
            assert!(p.offers("search_files"), "{id} must still search");
        }
    }

    #[test]
    fn plan_asks_rather_than_refusing() {
        let p = agent_def("plan").unwrap().permissions();
        // The old Approve mode deleted the tool; asking keeps it usable.
        assert!(p.offers("run_command"));
        assert_eq!(p.decide("run_command", "npm test"), Permission::Ask);
        assert!(!p.offers("edit_file"));
    }

    #[test]
    fn build_and_general_are_unrestricted() {
        for id in ["build", "general"] {
            let p = agent_def(id).unwrap().permissions();
            assert!(p.is_empty(), "{id} should carry no restrictions");
            assert_eq!(p.decide("run_command", "anything"), Permission::Allow);
        }
    }

    #[test]
    fn explore_stays_local_but_scout_reaches_the_internet() {
        let explore = agent_def("explore").unwrap().permissions();
        let scout = agent_def("scout").unwrap().permissions();
        assert!(!explore.offers("web_fetch"), "explore is codebase-only");
        assert!(scout.offers("web_fetch"), "scout exists to read docs");
    }
}
