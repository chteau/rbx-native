//! The tools a client can call, named and shaped after Studio's own MCP
//! server's, and the dotted paths they name instances by.

mod assets;
mod datamodel;
mod docs;
mod edit;
mod input;
mod luau;
mod scripts;
mod skills;
mod studio;
mod tree;
mod values;

#[cfg(test)]
mod tests;

use rbx_dom::{Ref, WeakDom};
use serde_json::{json, Value};

use super::protocol::{Failure, INVALID_PARAMS};
use super::Session;
use datamodel::{datamodel_dom, edit_datamodel};

pub(crate) use assets::{inserted, Insertion};
pub(crate) use edit::{apply as apply_edit, ScriptEdit};
pub(crate) use studio::{LIST_STUDIOS, STUDIOS_KEY, STUDIO_STATE};

/// What every path starts with, as `Instance:GetFullName()` would print it
/// from a script: the services are the DataModel's children.
const ROOT: &str = "game";

/// A tool's own failure (a path that names nothing): the client is told in
/// the result rather than as a protocol error, so the model can read it and
/// try again.
type Outcome = Result<Value, String>;

/// `tools/list`, mirroring Studio's own (`StudioMCP`) names, titles,
/// annotations and argument schemas, so a client or prompt written for
/// Studio works unchanged. The descriptions are ours and say what this
/// editor does.
const STUDIO_TOOLS: &str = include_str!("tools/studio_tools.json");

/// Tools of our own, for what Studio's MCP cannot do (saving the place).
/// Each says so in its description.
const EXTRA_TOOLS: &str = include_str!("tools/extra_tools.json");

/// The names of [`EXTRA_TOOLS`].
#[cfg(test)]
pub(super) fn extra_names() -> Vec<String> {
    let extras: Vec<Value> = serde_json::from_str(EXTRA_TOOLS).expect("extra_tools.json parses");
    extras
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect()
}

/// `tools/list`'s answer, Studio's then ours, with the `skill` entry naming this editor's
/// skills. Studio requires `studio_id` on every call and says so;
/// here it may still be left out while only one editor is open.
pub(super) fn list() -> Value {
    let mut tools: Value = serde_json::from_str(STUDIO_TOOLS).expect("studio_tools.json parses");
    let extras: Vec<Value> = serde_json::from_str(EXTRA_TOOLS).expect("extra_tools.json parses");
    if let Some(list) = tools.as_array_mut() {
        list.extend(extras);
    }
    for tool in tools.as_array_mut().into_iter().flatten() {
        if tool["name"] == skills::NAME {
            skills::describe(tool);
        }
    }
    tools
}

/// `tools/call`'s answer.
pub(super) fn call(params: &Value, session: &mut Session) -> Result<Value, Failure> {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return Err((INVALID_PARAMS, "tools/call needs a tool name".to_owned()));
    };
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
    let outcome = match text_argument(&arguments, "studio_id") {
        // Only a client that connected to this editor by hand gets here with
        // another editor's id: the broker routes by it.
        Some(id) if name != LIST_STUDIOS && id != session.studio.id => Err(format!(
            "this editor is studio {}, not {id}; call {LIST_STUDIOS} for the others",
            session.studio.id
        )),
        _ => run(name, &arguments, session)?,
    };
    Ok(finished(outcome))
}

/// A tool's outcome as `tools/call` answers it. `pub(super)`: a deferred
/// tool's outcome is only known once its work has run.
pub(super) fn finished(outcome: Outcome) -> Value {
    match outcome {
        Ok(value) => result(&value, false),
        Err(message) => result(&Value::String(message), true),
    }
}

fn run(name: &str, arguments: &Value, session: &mut Session) -> Result<Outcome, Failure> {
    Ok(match name {
        "search_game_tree" => tree::search(arguments, session),
        "inspect_instance" => tree::inspect(arguments, session),
        "script_read" => scripts::read(arguments, session),
        "script_search" => scripts::search(arguments, session),
        "script_grep" => scripts::grep(arguments, session),
        "multi_edit" => edit::multi_edit(arguments, session),
        input::NAVIGATION => input::navigation(arguments, session),
        input::KEYBOARD => input::keyboard(arguments, session),
        input::MOUSE => input::mouse(arguments, session),
        STUDIO_STATE => Ok(studio::state(session.studio)),
        skills::NAME => skills::call(arguments),
        docs::NAME => {
            let arguments = arguments.clone();
            session.deferred = Some(Box::new(move || docs::call(&arguments)));
            Ok(Value::Null)
        }
        LIST_STUDIOS => Ok(studio::list(session.studio)),
        luau::EXECUTE_LUAU => luau::execute(arguments, session),
        luau::START_STOP_PLAY => luau::start_stop(arguments, session),
        luau::CONSOLE_OUTPUT => luau::console(session),
        luau::SCREEN_CAPTURE => luau::capture(arguments, session),
        luau::SAVE_PLACE => luau::save(arguments, session),
        _ => match assets::run(name, arguments, session) {
            Some(outcome) => outcome,
            None => return Err((INVALID_PARAMS, format!("unknown tool: {name}"))),
        },
    })
}

/// The answer to a tool that reads nothing of an editor's (`skill`,
/// `http_get`), which the broker gives itself, so documentation is reachable
/// with no editor open and a slow fetch never waits on a UI thread.
pub(super) fn without_editor(name: &str, arguments: &Value) -> Option<Value> {
    let outcome = match name {
        skills::NAME => skills::call(arguments),
        docs::NAME => docs::call(arguments),
        _ => return None,
    };
    Some(finished(outcome))
}

/// A tool's result: its value as text, pretty JSON for anything but a
/// string. `pub(super)`: the broker answers `list_roblox_studios` with it.
pub(super) fn result(value: &Value, is_error: bool) -> Value {
    let text = match value {
        Value::String(text) => text.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_default(),
    };
    json!({ "content": [text_content(&text)], "isError": is_error })
}

/// An MCP text content block.
pub(crate) fn text_content(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

/// An MCP image content block holding a JPEG, as Studio's screen_capture
/// answers.
pub(crate) fn image_content(jpeg: &[u8]) -> Value {
    use base64::Engine;
    json!({
        "type": "image",
        "data": base64::engine::general_purpose::STANDARD.encode(jpeg),
        "mimeType": "image/jpeg",
    })
}

fn text_argument<'a>(arguments: &'a Value, name: &str) -> Option<&'a str> {
    arguments.get(name).and_then(Value::as_str)
}

fn required_text<'a>(arguments: &'a Value, name: &str) -> Result<&'a str, String> {
    text_argument(arguments, name).ok_or_else(|| format!("missing the `{name}` argument"))
}

fn flag_argument(arguments: &Value, name: &str) -> Option<bool> {
    arguments.get(name).and_then(Value::as_bool)
}

fn count_argument(arguments: &Value, name: &str) -> Option<usize> {
    arguments
        .get(name)
        .and_then(Value::as_u64)
        .map(|count| usize::try_from(count).unwrap_or(usize::MAX))
}

/// `Workspace.Part` for an instance, as Studio's tools print paths: from
/// the services down, without `game.` in front.
fn path_of(dom: &WeakDom, reference: Ref) -> String {
    let mut names = Vec::new();
    let mut at = Some(reference);
    while let Some(current) = at {
        let Some(instance) = dom.get(current) else {
            break;
        };
        names.push(instance.name());
        at = dom.parent(current);
    }
    names.reverse();
    names.join(".")
}

/// `path` without the optional `game.` in front, in either case.
fn relative(path: &str) -> &str {
    let path = path.trim();
    match path.get(..ROOT.len() + 1) {
        Some(prefix) if prefix.eq_ignore_ascii_case("game.") => &path[ROOT.len() + 1..],
        _ => path,
    }
}

/// The instance a dotted path names; `game.` in front is optional, so a
/// path copied from the Explorer works as well. `None` for `game` itself,
/// which is not an instance this DOM holds.
fn resolve(dom: &WeakDom, path: &str) -> Option<Ref> {
    let segments: Vec<&str> = relative(path).split('.').collect();
    descend(dom, dom.root_refs(), &segments)
}

/// Every instance a dotted path names, ignoring case, in Explorer order:
/// siblings that share a name are all of them named by it.
fn resolve_all(dom: &WeakDom, path: &str) -> Vec<Ref> {
    let segments: Vec<&str> = relative(path).split('.').collect();
    let mut found = Vec::new();
    collect(dom, dom.root_refs(), &segments, &mut found);
    found
}

/// [`descend`] for every match rather than the first.
fn collect(dom: &WeakDom, candidates: &[Ref], segments: &[&str], found: &mut Vec<Ref>) {
    for taken in 1..=segments.len() {
        let name = segments[..taken].join(".").to_lowercase();
        for &candidate in candidates {
            let Some(instance) = dom.get(candidate) else {
                continue;
            };
            if instance.name().to_lowercase() != name {
                continue;
            }
            if taken == segments.len() {
                if !found.contains(&candidate) {
                    found.push(candidate);
                }
            } else {
                collect(dom, instance.children(), &segments[taken..], found);
            }
        }
    }
}

/// A name may itself hold a dot (`Config.v2`), so each level tries every
/// run of segments as one name, shortest first.
fn descend(dom: &WeakDom, candidates: &[Ref], segments: &[&str]) -> Option<Ref> {
    for taken in 1..=segments.len() {
        let name = segments[..taken].join(".");
        for &candidate in candidates {
            let Some(instance) = dom.get(candidate) else {
                continue;
            };
            if instance.name() != name {
                continue;
            }
            if taken == segments.len() {
                return Some(candidate);
            }
            if let Some(found) = descend(dom, instance.children(), &segments[taken..]) {
                return Some(found);
            }
        }
    }
    None
}
