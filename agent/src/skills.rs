//! Skills: reusable instructions kept out of the system prompt until wanted.
//!
//! A skill is a `SKILL.md` with YAML frontmatter. The agent sees only the name
//! and description of each — listed in the `skill` tool's description — and
//! pulls the body in on demand. That is the point: a dozen playbooks cost a
//! dozen lines of context instead of a dozen documents.

use ide_core::WorkspaceRoot;
use std::path::{Path, PathBuf};

/// Where a skill was found, nearest first. Project skills beat global ones.
const PROJECT_DIRS: &[&str] = &[".opencode/skills", ".claude/skills", ".agents/skills"];
const GLOBAL_DIRS: &[&str] = &[".config/opencode/skills", ".claude/skills", ".agents/skills"];

/// Largest body we will paste into the conversation. A runaway file would
/// otherwise blow the context window in one call.
const MAX_BODY: usize = 60_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

/// `^[a-z0-9]+(-[a-z0-9]+)*$` — lowercase, single hyphens, no leading, trailing
/// or doubled separators. Enforced so a skill name is always safe to print in a
/// tool description and to match a permission pattern against.
pub fn valid_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return false;
    }
    name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Split YAML frontmatter from the body. Deliberately a small hand parser: the
/// only fields that matter are flat `key: value` strings, and pulling in a YAML
/// crate for that would be a dependency for four lines of work.
pub fn parse_front_matter(text: &str) -> (Vec<(String, String)>, String) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let rest = match text.strip_prefix("---") {
        Some(r) => r.trim_start_matches(['\r', '\n']),
        None => return (Vec::new(), text.to_string()),
    };
    // The closing fence must be its own line, or a `---` rule inside the body
    // would truncate the skill.
    let mut fields = Vec::new();
    let mut body_start = None;
    let mut consumed = 0usize;
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        consumed += line.len();
        if trimmed.trim() == "---" {
            body_start = Some(consumed);
            break;
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            let val = v.trim().trim_matches(['"', '\'']).to_string();
            if !key.is_empty() && !key.starts_with('#') {
                fields.push((key, val));
            }
        }
    }
    match body_start {
        Some(at) => (fields, rest[at..].trim_start_matches(['\r', '\n']).to_string()),
        // Unterminated frontmatter: treat the whole file as body rather than
        // silently returning nothing.
        None => (Vec::new(), text.to_string()),
    }
}

fn field<'a>(fields: &'a [(String, String)], key: &str) -> Option<&'a str> {
    fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Read one `<dir>/<name>/SKILL.md`. Returns None for anything malformed — a
/// broken skill is skipped, never fatal.
fn load_one(dir: &Path) -> Option<Skill> {
    let name_from_dir = dir.file_name()?.to_str()?.to_string();
    let file = dir.join("SKILL.md");
    let text = std::fs::read_to_string(&file).ok()?;
    let (fields, _) = parse_front_matter(&text);
    let name = field(&fields, "name")?.to_string();
    let description = field(&fields, "description")?.trim().to_string();
    // The name must match the folder: that is what makes `skill({name})`
    // resolvable back to a single file without a lookup table.
    if name != name_from_dir || !valid_name(&name) {
        return None;
    }
    if description.is_empty() || description.len() > 1024 {
        return None;
    }
    Some(Skill { name, description, path: file })
}

fn scan(root: &Path, out: &mut Vec<Skill>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for e in entries.flatten() {
        if e.path().is_dir() {
            if let Some(s) = load_one(&e.path()) {
                // Nearest wins: a project skill already collected keeps its slot.
                if !out.iter().any(|k| k.name == s.name) {
                    out.push(s);
                }
            }
        }
    }
}

/// Every skill visible from this workspace, project before global, sorted by
/// name so the tool description is stable between runs.
pub fn discover(ws: &WorkspaceRoot) -> Vec<Skill> {
    let mut out = Vec::new();
    for d in PROJECT_DIRS {
        scan(&ws.root().join(d), &mut out);
    }
    if let Some(home) = home_dir() {
        for d in GLOBAL_DIRS {
            scan(&home.join(d), &mut out);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// The body, clipped. Frontmatter is stripped: the agent already has the name
/// and description, and repeating them wastes the context this feature saves.
pub fn load_body(skill: &Skill) -> anyhow::Result<String> {
    let text = std::fs::read_to_string(&skill.path)?;
    let (_, body) = parse_front_matter(&text);
    let body = body.trim();
    if body.is_empty() {
        anyhow::bail!("skill `{}` has no content below its frontmatter", skill.name);
    }
    if body.len() > MAX_BODY {
        let mut cut = MAX_BODY;
        while cut > 0 && !body.is_char_boundary(cut) {
            cut -= 1;
        }
        return Ok(format!("{}\n\n… skill truncated at {MAX_BODY} characters", &body[..cut]));
    }
    Ok(body.to_string())
}

/// The catalogue line the model reads before choosing. Kept out of the system
/// prompt and put in the tool description, so it costs nothing when the agent
/// has no skills.
pub fn catalogue(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\nAvailable skills:\n");
    for k in skills {
        s.push_str(&format!("- {}: {}\n", k.name, k.description));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_the_documented_pattern() {
        for ok in ["pdf", "code-review", "a1", "one-two-three"] {
            assert!(valid_name(ok), "{ok} should be valid");
        }
        for bad in ["", "-lead", "trail-", "double--hyphen", "Upper", "has space", "under_score"] {
            assert!(!valid_name(bad), "{bad} should be rejected");
        }
        assert!(!valid_name(&"a".repeat(65)), "64 characters is the cap");
    }

    #[test]
    fn front_matter_splits_fields_from_body() {
        let (f, body) = parse_front_matter("---\nname: pdf\ndescription: Reads PDFs\n---\nBody here\n");
        assert_eq!(field(&f, "name"), Some("pdf"));
        assert_eq!(field(&f, "description"), Some("Reads PDFs"));
        assert_eq!(body.trim(), "Body here");
    }

    /// A markdown horizontal rule in the body must not be mistaken for the
    /// closing fence, or the skill would be silently cut in half.
    #[test]
    fn a_rule_inside_the_body_does_not_end_the_front_matter() {
        let (f, body) = parse_front_matter(
            "---\nname: x\ndescription: d\n---\nintro\n\n---\n\nmore text\n",
        );
        assert_eq!(field(&f, "name"), Some("x"));
        assert!(body.contains("intro"), "{body}");
        assert!(body.contains("more text"), "body was truncated at the rule: {body}");
    }

    #[test]
    fn a_file_without_front_matter_is_all_body() {
        let (f, body) = parse_front_matter("just text\n");
        assert!(f.is_empty());
        assert_eq!(body.trim(), "just text");
    }

    /// Unterminated frontmatter used to swallow the file whole and return an
    /// empty body; keeping the text is the safer failure.
    #[test]
    fn unterminated_front_matter_keeps_the_text() {
        let (f, body) = parse_front_matter("---\nname: x\nno closing fence\n");
        assert!(f.is_empty());
        assert!(body.contains("no closing fence"));
    }

    #[test]
    fn quotes_are_stripped_from_values() {
        let (f, _) = parse_front_matter("---\nname: \"pdf\"\ndescription: 'Reads PDFs'\n---\nb\n");
        assert_eq!(field(&f, "name"), Some("pdf"));
        assert_eq!(field(&f, "description"), Some("Reads PDFs"));
    }

    #[test]
    fn catalogue_is_empty_when_there_are_no_skills() {
        assert_eq!(catalogue(&[]), "");
        let one = vec![Skill {
            name: "pdf".into(),
            description: "Reads PDFs".into(),
            path: PathBuf::from("x"),
        }];
        let c = catalogue(&one);
        assert!(c.contains("- pdf: Reads PDFs"), "{c}");
    }

    #[test]
    fn discovery_reads_a_real_skill_and_skips_a_mismatched_one() {
        let root = std::env::temp_dir().join("ide-ai-skills-test");
        let _ = std::fs::remove_dir_all(&root);
        let base = root.join(".opencode/skills");
        std::fs::create_dir_all(base.join("pdf")).unwrap();
        std::fs::write(
            base.join("pdf/SKILL.md"),
            "---\nname: pdf\ndescription: Reads PDFs\n---\nHow to read a PDF.\n",
        )
        .unwrap();
        // name disagrees with its folder, so it must be ignored
        std::fs::create_dir_all(base.join("wrong")).unwrap();
        std::fs::write(
            base.join("wrong/SKILL.md"),
            "---\nname: other\ndescription: d\n---\nbody\n",
        )
        .unwrap();
        // no description, also ignored
        std::fs::create_dir_all(base.join("bare")).unwrap();
        std::fs::write(base.join("bare/SKILL.md"), "---\nname: bare\n---\nbody\n").unwrap();

        let ws = WorkspaceRoot::open(&root).unwrap();
        let found = discover(&ws);
        assert_eq!(found.len(), 1, "got {found:?}");
        assert_eq!(found[0].name, "pdf");
        assert_eq!(load_body(&found[0]).unwrap(), "How to read a PDF.");

        std::fs::remove_dir_all(&root).ok();
    }
}
