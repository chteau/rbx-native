//! `multi_edit`: several replacements in one script's `Source`, checked
//! against the text in full before anything is written, so a call either
//! lands whole or leaves the script as it was.
//!
//! The tool only plans the write ([`ScriptEdit`]); the shell applies it with
//! [`apply`] between an undo snapshot and its change log, as it does a
//! script tab's own edits.

#[cfg(test)]
mod tests;

use rbx_dom::{Ref, WeakDom};
use serde_json::Value;

use super::{edit_datamodel, path_of, relative, required_text, resolve, text_argument, Outcome};
use crate::mcp::{PlayState, Session};
use crate::script_editor::source;

/// What a new script can be created as.
pub(super) const SCRIPT_CLASSES: [&str; 3] = ["Script", "LocalScript", "ModuleScript"];

/// A `Source` write a `multi_edit` call asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ScriptEdit {
    pub(crate) target: Target,
    pub(crate) source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Existing(Ref),
    New {
        parent: Ref,
        name: String,
        class: &'static str,
    },
}

impl ScriptEdit {
    /// Whether applying it adds an instance, which the Explorer has to show.
    pub(crate) fn creates(&self) -> bool {
        matches!(self.target, Target::New { .. })
    }
}

/// Writes `edit` into `dom`, creating its script first when it is new. The
/// script written to, or `None` when what it names is gone or `Source`
/// already held the text.
pub(crate) fn apply(dom: &mut WeakDom, edit: &ScriptEdit) -> Option<Ref> {
    match &edit.target {
        Target::Existing(reference) => {
            source::write(dom, *reference, &edit.source).then_some(*reference)
        }
        Target::New {
            parent,
            name,
            class,
        } => {
            dom.get(*parent)?;
            let reference = dom.new_instance(class, name, Some(*parent));
            source::write(dom, reference, &edit.source);
            Some(reference)
        }
    }
}

struct Replacement {
    old: String,
    new: String,
    all: bool,
}

pub(super) fn multi_edit(arguments: &Value, session: &mut Session) -> Outcome {
    edit_datamodel(arguments)?;
    if session.studio.play == PlayState::ShowingLive {
        return Err(
            "the editor is showing a running playtest; switch back to the edited place first"
                .to_owned(),
        );
    }
    let path = required_text(arguments, "file_path")?;
    let replacements = replacements(arguments)?;
    let dom = session.dom;

    let (target, before, name, class) = match resolve(dom, path) {
        Some(reference) => {
            let Some(instance) = dom.get(reference) else {
                return Err(format!("Script not found at path: {path}"));
            };
            if !source::is_script_class(session.properties.db(), instance.class()) {
                return Err(format!("{} is not a script", path_of(dom, reference)));
            }
            let before = source::read(dom, reference).unwrap_or_default();
            let (name, class) = (instance.name().to_owned(), instance.class().to_owned());
            (Target::Existing(reference), before, name, class)
        }
        None => {
            let (parent, name) = new_place(dom, path)?;
            let class = new_class(arguments, path)?;
            let target = Target::New {
                parent,
                name: name.clone(),
                class,
            };
            (target, String::new(), name, class.to_owned())
        }
    };

    let after = replace(&before, &replacements)?;
    let created = matches!(target, Target::New { .. });
    // Edits that end where they started leave no undo step behind.
    if created || after != before {
        session.planned = Some(ScriptEdit {
            target,
            source: after,
        });
    }
    // Studio's own sentence, which counts edits as "edits" even for one.
    let verb = if created { "Created" } else { "Modified" };
    Ok(Value::String(format!(
        "{verb} {name} ({class}) with {} edits applied",
        replacements.len()
    )))
}

fn replacements(arguments: &Value) -> Result<Vec<Replacement>, String> {
    let Some(edits) = arguments
        .get("edits")
        .and_then(Value::as_array)
        .filter(|edits| !edits.is_empty())
    else {
        return Err("missing the `edits` argument: a list of {old_string, new_string}".to_owned());
    };
    edits
        .iter()
        .enumerate()
        .map(|(index, edit)| {
            let field = |name: &str| {
                text_argument(edit, name)
                    .map(str::to_owned)
                    .ok_or_else(|| format!("Edit {} - missing `{name}`", index + 1))
            };
            let replacement = Replacement {
                old: field("old_string")?,
                new: field("new_string")?,
                all: edit
                    .get("replace_all")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            };
            // Studio refuses this too: an edit that changes nothing is a
            // mistake in the call, not a request.
            if replacement.old == replacement.new {
                return Err(format!(
                    "Edit {} has identical old_string and new_string",
                    index + 1
                ));
            }
            Ok(replacement)
        })
        .collect()
}

/// Applies every replacement in turn to `text`, each against what the ones
/// before it left, as an editor applying them one by one would.
fn replace(text: &str, replacements: &[Replacement]) -> Result<String, String> {
    let mut text = text.to_owned();
    for (index, replacement) in replacements.iter().enumerate() {
        let number = index + 1;
        if replacement.old.is_empty() {
            // Anywhere at all would match an empty string; only an empty
            // script leaves no doubt about where the text goes.
            if !text.is_empty() {
                return Err(format!(
                    "Edit {number} - an empty old_string only fills an empty script"
                ));
            }
            text.clone_from(&replacement.new);
            continue;
        }
        match text.matches(replacement.old.as_str()).count() {
            0 => {
                return Err(format!(
                    "Edit {number} - old_string '{}' not found in current content",
                    replacement.old
                ))
            }
            1 => text = text.replacen(&replacement.old, &replacement.new, 1),
            _ if replacement.all => text = text.replace(&replacement.old, &replacement.new),
            count => {
                return Err(format!(
                    "Edit {number} - old_string '{}' appears {count} times; give more of the \
                     surrounding text or set replace_all",
                    replacement.old
                ))
            }
        }
    }
    Ok(text)
}

/// Where a script at `path`, which names nothing yet, would go: the last
/// segment is its name, everything before it its parent. Only the last level
/// may be new, so a typo higher up is reported rather than turned into a
/// script whose name holds a dot.
fn new_place(dom: &WeakDom, path: &str) -> Result<(Ref, String), String> {
    let parent = relative(path)
        .rsplit_once('.')
        .filter(|(_, name)| !name.is_empty())
        .and_then(|(parent, name)| Some((resolve(dom, parent)?, name.to_owned())));
    parent.ok_or_else(|| {
        format!("nothing in the place is at {path}, nor at the parent a new script would need")
    })
}

/// The class a new script is created as. Studio asks for it rather than
/// guess, since a `Script` where a `ModuleScript` was meant runs nothing the
/// caller expects.
fn new_class(arguments: &Value, path: &str) -> Result<&'static str, String> {
    let classes = SCRIPT_CLASSES.join(", ");
    let Some(asked) = text_argument(arguments, "className").filter(|asked| !asked.is_empty())
    else {
        return Err(format!(
            "`className` is required when creating a new script at path: {path}"
        ));
    };
    SCRIPT_CLASSES
        .iter()
        .copied()
        .find(|class| *class == asked)
        .ok_or_else(|| format!("className must be one of {classes}, not {asked}"))
}
