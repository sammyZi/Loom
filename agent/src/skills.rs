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
    let mut fields: Vec<(String, String)> = Vec::new();
    let mut body_start = None;
    let mut consumed = 0usize;
    // Set while inside a `key: >` / `key: |` block; real SKILL.md files wrap
    // long descriptions that way, and reading only the marker line left the
    // description empty, which silently discarded the whole skill.
    let mut folding: Option<(usize, bool)> = None; // (field index, keep newlines)
    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        consumed += line.len();
        if trimmed.trim() == "---" {
            body_start = Some(consumed);
            break;
        }
        if let Some((idx, literal)) = folding {
            let indented = trimmed.starts_with(' ') || trimmed.starts_with('\t');
            if indented || trimmed.trim().is_empty() {
                let piece = trimmed.trim();
                let acc = &mut fields[idx].1;
                if !piece.is_empty() {
                    if !acc.is_empty() {
                        acc.push(if literal { '\n' } else { ' ' });
                    }
                    acc.push_str(piece);
                }
                continue;
            }
            folding = None; // dedented: the block ended
        }
        if let Some((k, v)) = trimmed.split_once(':') {
            let key = k.trim().to_ascii_lowercase();
            if key.is_empty() || key.starts_with('#') {
                continue;
            }
            let val = v.trim();
            if val == ">" || val == "|" || val == ">-" || val == "|-" {
                fields.push((key, String::new()));
                folding = Some((fields.len() - 1, val.starts_with('|')));
                continue;
            }
            fields.push((key, val.trim_matches(['"', '\'']).to_string()));
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
    discover_in(ws, home_dir().as_deref())
}

/// Split out so tests never depend on the machine's home directory — installing
/// a global skill used to break them, which is a fault in the test, not the
/// skill. Pass None to scan the project only.
pub fn discover_in(ws: &WorkspaceRoot, home: Option<&Path>) -> Vec<Skill> {
    let mut out = Vec::new();
    for d in PROJECT_DIRS {
        scan(&ws.root().join(d), &mut out);
    }
    if let Some(home) = home {
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

/// How much skill text may ride along in the system prompt. Generous because
/// the system prompt is the *cached* prefix — measured at ~94% reuse — so this
/// is paid in full once per run and at roughly a tenth on every turn after.
const PRELOAD_BUDGET: usize = 16_000;

/// Skills loaded up front, plus the names that were loaded.
///
/// The `skill` tool alone meant a skill only applied when the model remembered
/// to ask for it, which it often did not. Loading them at the start of the run
/// makes them unconditional. Anything past the budget stays behind the tool.
pub fn preload(ws: &WorkspaceRoot) -> (String, Vec<String>) {
    preload_in(ws, home_dir().as_deref())
}

/// Split out for the same reason as `discover_in`: a test must not depend on
/// what happens to be installed in the developer's home directory.
pub fn preload_in(ws: &WorkspaceRoot, home: Option<&Path>) -> (String, Vec<String>) {
    let found = discover_in(ws, home);
    let mut text = String::new();
    let mut names = Vec::new();
    let mut spent = 0usize;
    for skill in &found {
        let Ok(body) = load_body(skill) else { continue };
        if spent + body.len() > PRELOAD_BUDGET {
            continue;
        }
        spent += body.len();
        names.push(skill.name.clone());
        text.push_str(&format!(
            "\n\n--- skill: {} ---\n{}\n--- end skill: {} ---",
            skill.name, body, skill.name
        ));
    }
    if text.is_empty() {
        return (String::new(), names);
    }
    (
        format!(
            "\n\nThe following skills are active for this task. Follow them in place of your \
             default approach; they are instructions, not reference material.{text}"
        ),
        names,
    )
}

/// The catalogue line the model reads before choosing. Kept out of the system
/// prompt and put in the tool description, so it costs nothing when the agent
/// has no skills.
pub fn catalogue(skills: &[Skill]) -> String {
    catalogue_excluding(skills, &[])
}

/// The catalogue, minus anything already loaded into the system prompt — the
/// model should not be invited to fetch what it has already been given.
pub fn catalogue_excluding(skills: &[Skill], loaded: &[String]) -> String {
    let rest: Vec<&Skill> = skills.iter().filter(|k| !loaded.contains(&k.name)).collect();
    if rest.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\nAvailable skills:\n");
    for k in rest {
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

    /// Real SKILL.md files wrap long descriptions in a YAML block scalar. The
    /// first parser read only the `>` marker, so the description came out empty
    /// and the skill was thrown away as malformed.
    #[test]
    fn folded_block_descriptions_are_read_whole() {
        let (f, body) = parse_front_matter(
            "---\nname: ponytail\ndescription: >\n  Forces the laziest solution\n  that actually works.\nmode: all\n---\nBody.\n",
        );
        assert_eq!(field(&f, "name"), Some("ponytail"));
        assert_eq!(
            field(&f, "description"),
            Some("Forces the laziest solution that actually works."),
            "folded scalars join with spaces"
        );
        // a sibling key after the block is still parsed, not swallowed
        assert_eq!(field(&f, "mode"), Some("all"));
        assert_eq!(body.trim(), "Body.");
    }

    #[test]
    fn literal_blocks_keep_their_line_breaks() {
        let (f, _) = parse_front_matter("---\nname: x\ndescription: |\n  one\n  two\n---\nb\n");
        assert_eq!(field(&f, "description"), Some("one\ntwo"));
    }

    #[test]
    fn quotes_are_stripped_from_values() {
        let (f, _) = parse_front_matter("---\nname: \"pdf\"\ndescription: 'Reads PDFs'\n---\nb\n");
        assert_eq!(field(&f, "name"), Some("pdf"));
        assert_eq!(field(&f, "description"), Some("Reads PDFs"));
    }

    /// Skills only applied when the model remembered to fetch them, which it
    /// often did not. Preloading makes them unconditional.
    #[test]
    fn preload_inlines_bodies_and_names_what_it_loaded() {
        let root = std::env::temp_dir().join("ide-ai-preload-test");
        let _ = std::fs::remove_dir_all(&root);
        let base = root.join(".opencode/skills");
        std::fs::create_dir_all(base.join("ponytail")).unwrap();
        std::fs::write(
            base.join("ponytail/SKILL.md"),
            "---\nname: ponytail\ndescription: Laziest solution that works\n---\nPrefer stdlib.\n",
        )
        .unwrap();

        let ws = WorkspaceRoot::open(&root).unwrap();
        let (text, names) = preload_in(&ws, None);
        assert_eq!(names, vec!["ponytail".to_string()]);
        assert!(text.contains("Prefer stdlib."), "body must be inlined: {text}");
        assert!(text.contains("--- skill: ponytail ---"), "{text}");
        // Framed as instructions, not as something to consider.
        assert!(text.contains("in place of your default approach"), "{text}");

        // A preloaded skill drops out of the tool catalogue.
        let found = discover_in(&ws, None);
        assert!(catalogue_excluding(&found, &names).is_empty());
        assert!(catalogue_excluding(&found, &[]).contains("ponytail"));

        std::fs::remove_dir_all(&root).ok();
    }

    /// A runaway shelf must not swallow the prompt: anything past the budget
    /// stays behind the tool rather than being dropped silently.
    #[test]
    fn preload_stops_at_the_budget() {
        let root = std::env::temp_dir().join("ide-ai-preload-budget-test");
        let _ = std::fs::remove_dir_all(&root);
        let base = root.join(".opencode/skills");
        let big = "y".repeat(9_000);
        for n in ["aaa", "bbb", "ccc"] {
            std::fs::create_dir_all(base.join(n)).unwrap();
            std::fs::write(
                base.join(n).join("SKILL.md"),
                format!("---\nname: {n}\ndescription: d\n---\n{big}\n"),
            )
            .unwrap();
        }
        let ws = WorkspaceRoot::open(&root).unwrap();
        let (text, names) = preload_in(&ws, None);
        assert_eq!(names.len(), 1, "9 KB each, 16 KB budget: only one fits");
        assert!(text.len() < PRELOAD_BUDGET + 500, "budget respected");
        // The ones that did not fit are still reachable through the tool.
        let found = discover_in(&ws, None);
        let rest = catalogue_excluding(&found, &names);
        assert!(rest.contains("bbb") && rest.contains("ccc"), "{rest}");

        std::fs::remove_dir_all(&root).ok();
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
        let found = discover_in(&ws, None);
        assert_eq!(found.len(), 1, "got {found:?}");
        assert_eq!(found[0].name, "pdf");
        assert_eq!(load_body(&found[0]).unwrap(), "How to read a PDF.");

        std::fs::remove_dir_all(&root).ok();
    }
}
