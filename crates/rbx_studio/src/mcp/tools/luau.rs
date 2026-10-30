//! `execute_luau`, `start_stop_play`, `get_console_output` and
//! `screen_capture`: tools only the shell can carry out. Each reads its
//! arguments here and leaves a [`Job`] for the shell, which answers the call.

#[cfg(test)]
mod tests;

use serde_json::Value;

use super::datamodel::{datamodel_argument, require_live};
use super::{required_text, Outcome};
use crate::mcp::{Job, Session};

pub(super) const EXECUTE_LUAU: &str = "execute_luau";
pub(super) const START_STOP_PLAY: &str = "start_stop_play";
pub(super) const CONSOLE_OUTPUT: &str = "get_console_output";
pub(super) const SCREEN_CAPTURE: &str = "screen_capture";
/// Not one of Studio's tools: its MCP has no way to save the place.
pub(super) const SAVE_PLACE: &str = "save_place";

const CAMERA_POSITION: &str = "camera_position";
const LOOK_AT_POSITION: &str = "look_at_position";

/// The shell answers every job; what a tool returns here is never sent.
fn leave(session: &mut Session, job: Job) -> Outcome {
    session.job = Some(job);
    Ok(Value::Null)
}

pub(super) fn execute(arguments: &Value, session: &mut Session) -> Outcome {
    let code = required_text(arguments, "code")?;
    let datamodel =
        datamodel_argument(arguments)?.ok_or_else(|| "datamodel_type is required".to_owned())?;
    require_live(datamodel, session.studio)?;
    let job = Job::Luau {
        code: code.to_owned(),
        datamodel,
    };
    leave(session, job)
}

pub(super) fn start_stop(arguments: &Value, session: &mut Session) -> Outcome {
    let start = arguments
        .get("is_start")
        .and_then(Value::as_bool)
        .ok_or_else(|| "missing the `is_start` argument".to_owned())?;
    leave(session, Job::Play { start })
}

pub(super) fn console(session: &mut Session) -> Outcome {
    leave(session, Job::Console)
}

pub(super) fn save(arguments: &Value, session: &mut Session) -> Outcome {
    let path = match arguments.get("path") {
        None | Some(Value::Null) => None,
        Some(Value::String(path)) if !path.trim().is_empty() => Some(path.trim().into()),
        Some(_) => return Err("path must be a file path".to_owned()),
    };
    leave(session, Job::Save { path })
}

pub(super) fn capture(arguments: &Value, session: &mut Session) -> Outcome {
    let eye = arguments.get(CAMERA_POSITION).filter(|v| !v.is_null());
    let target = arguments.get(LOOK_AT_POSITION).filter(|v| !v.is_null());
    let camera = match (eye, target) {
        (None, None) => None,
        (Some(eye), Some(target)) => Some((vector(eye)?, vector(target)?)),
        _ => {
            return Err(format!(
                "{CAMERA_POSITION} and {LOOK_AT_POSITION} must be provided together"
            ))
        }
    };
    leave(session, Job::Capture { camera })
}

fn vector(value: &Value) -> Result<[f32; 3], String> {
    let items = value
        .as_array()
        .filter(|items| items.len() == 3)
        .ok_or_else(|| {
            format!("{CAMERA_POSITION} and {LOOK_AT_POSITION} must be 3-element arrays")
        })?;
    let mut out = [0.0; 3];
    for (slot, item) in out.iter_mut().zip(items) {
        // Studs fit an f32; the viewport works in f32.
        *slot = item
            .as_f64()
            .ok_or_else(|| format!("{CAMERA_POSITION} and {LOOK_AT_POSITION} must be numbers"))?
            as f32;
    }
    Ok(out)
}
