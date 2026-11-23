//! `skill`: guidance a client pulls before a kind of work, such as building
//! UI or writing a DataStore. Each skill is a Markdown file under
//! `mcp/skills/`, its `name` and `description` in a front matter block as
//! Studio's own skills carry them, so the text is edited without touching
//! code.

#[cfg(test)]
mod tests;

use serde_json::Value;

use super::{text_argument, Outcome};

pub(super) const NAME: &str = "skill";
const SKILL_NAME: &str = "skill_name";

const FILES: &[&str] = &[
    include_str!("../skills/accessibility.md"),
    include_str!("../skills/animation.md"),
    include_str!("../skills/build-color-and-materials.md"),
    include_str!("../skills/build-importing-assets.md"),
    include_str!("../skills/build-level-design.md"),
    include_str!("../skills/build-lighting-and-atmosphere.md"),
    include_str!("../skills/build-modeling.md"),
    include_str!("../skills/build-modular-kits.md"),
    include_str!("../skills/data-persistence.md"),
    include_str!("../skills/debugging.md"),
    include_str!("../skills/docs-search.md"),
    include_str!("../skills/growth-analytics.md"),
    include_str!("../skills/growth-monetization.md"),
    include_str!("../skills/growth-promotion.md"),
    include_str!("../skills/growth-retention.md"),
    include_str!("../skills/input-and-devices.md"),
    include_str!("../skills/live-ops.md"),
    include_str!("../skills/luau-scripting.md"),
    include_str!("../skills/monetization.md"),
    include_str!("../skills/performance.md"),
    include_str!("../skills/security.md"),
    include_str!("../skills/server-authority.md"),
    include_str!("../skills/testing.md"),
    include_str!("../skills/ui-style-cartoony.md"),
    include_str!("../skills/ui-style-genres.md"),
    include_str!("../skills/ui-style-icons.md"),
    include_str!("../skills/ui-style-motion.md"),
    include_str!("../skills/ui-style-sleek.md"),
    include_str!("../skills/ui-style-striking.md"),
    include_str!("../skills/ui-style-trendy.md"),
    include_str!("../skills/ui-style.md"),
    include_str!("../skills/ux-patterns.md"),
];

struct Skill {
    name: &'static str,
    description: &'static str,
    body: &'static str,
}

/// `file` split into its front matter's `name` and `description` and the
/// text after it; `None` for a file without both.
fn parse(file: &'static str) -> Option<Skill> {
    // Matched without the line end: a Windows checkout turns them into CRLF.
    let rest = file.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    let field = |key: &str| {
        rest[..end].lines().find_map(|line| {
            let value = line.strip_prefix(key)?.strip_prefix(':')?.trim();
            Some(value.trim_matches('"'))
        })
    };
    Some(Skill {
        name: field("name")?,
        description: field("description")?,
        body: rest[end + 4..].trim_start(),
    })
}

fn skills() -> impl Iterator<Item = Skill> {
    FILES.iter().filter_map(|file| parse(file))
}

/// Fills the `skill` entry with this editor's skills: the
/// `<available_skills>` block of the description and the options of
/// `skill_name`.
pub(super) fn describe(tool: &mut Value) {
    let description = tool["description"].as_str().unwrap_or_default();
    let (Some(start), Some(end)) = (
        description.find("<available_skills>"),
        description.find("</available_skills>"),
    ) else {
        return;
    };
    let entries: String = skills()
        .map(|skill| {
            format!(
                "  <skill>\n    <name>{}</name>\n    <source>rbx-native</source>\n    \
                 <description>{}</description>\n  </skill>\n",
                skill.name, skill.description
            )
        })
        .collect();
    let description = format!(
        "{}<available_skills>\n{entries}{}",
        &description[..start],
        &description[end..]
    );
    let names: Vec<&str> = skills().map(|skill| skill.name).collect();
    tool["description"] = Value::String(description);
    tool["inputSchema"]["properties"][SKILL_NAME]["description"] =
        Value::String(format!("Which skill to load. One of: {}", names.join(", ")));
}

pub(super) fn call(arguments: &Value) -> Outcome {
    // Studio's argument is `skill_name`; a bare `name` is taken as well.
    let wanted = text_argument(arguments, SKILL_NAME).or_else(|| text_argument(arguments, "name"));
    let Some(wanted) = wanted.map(str::trim).filter(|name| !name.is_empty()) else {
        return Ok(Value::String(catalogue()));
    };
    skills()
        .find(|skill| skill.name.eq_ignore_ascii_case(wanted))
        .map(|skill| Value::String(format!("# Skill: {}\n\n{}", skill.name, skill.body)))
        .ok_or_else(|| format!("Unknown skill: {wanted}\n\n{}", catalogue()))
}

fn catalogue() -> String {
    let rows: Vec<String> = skills()
        .map(|skill| format!("- {}: {}", skill.name, skill.description))
        .collect();
    format!("Available skills:\n{}", rows.join("\n"))
}
