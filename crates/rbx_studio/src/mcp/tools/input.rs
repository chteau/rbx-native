//! `character_navigation`, `user_keyboard_input` and `user_mouse_input`:
//! playing the running playtest as its player would, with Studio's
//! arguments, checks and answers (compared side by side with its
//! `StudioMCP`).
//!
//! Studio's own versions act on its in-process client, so each tool here
//! does what that one does, to the real Roblox client:
//! - mouse buttons, the wheel and keys are real OS input into the embedded
//!   client's window (XTest on Linux, `SendInput` on Windows): Studio's are
//!   engine-level input events that games read through `UserInputService`
//!   and GUI objects exactly like a person's;
//! - walking is the client probe's `Humanoid:MoveTo` along a computed path at
//!   a scaled `WalkSpeed`, which is what Studio's does (no key is held: the
//!   humanoid's `MoveDirection` stays zero);
//! - `textInput` puts text into the focused `TextBox` without key events,
//!   as Studio's does, through the probe;
//! - where a `GuiObject` is on screen comes from the probe.
//!
//! Each call checks its arguments and the play state at once, then runs off
//! the UI thread ([`Session::deferred`]): a walk takes seconds, and every
//! step waits on the client.

mod run;

#[cfg(test)]
mod tests;

use serde_json::Value;

use super::{text_argument, Outcome};
use crate::mcp::{PlayState, Session};

pub(crate) const NAVIGATION: &str = "character_navigation";
pub(crate) const KEYBOARD: &str = "user_keyboard_input";
pub(crate) const MOUSE: &str = "user_mouse_input";

/// The one data model these tools act on: the playtest's client.
const CLIENT_DATAMODEL: &str = "Client";

/// Studio's longest `wait_time_ms`.
const MAX_WAIT_MS: f64 = 10_000.0;

pub(super) fn navigation(arguments: &Value, session: &mut Session) -> Outcome {
    let play = playtest(arguments, session)?;
    let arguments = arguments.clone();
    session.deferred = Some(Box::new(move || {
        run::navigate(&mut run::Live::new(play), &arguments).map(Value::String)
    }));
    Ok(Value::Null)
}

pub(super) fn keyboard(arguments: &Value, session: &mut Session) -> Outcome {
    let steps = steps(arguments)?;
    let play = playtest(arguments, session)?;
    session.deferred = Some(Box::new(move || {
        run::keyboard(&mut run::Live::new(play), &steps).map(Value::String)
    }));
    Ok(Value::Null)
}

pub(super) fn mouse(arguments: &Value, session: &mut Session) -> Outcome {
    let steps = steps(arguments)?;
    let play = playtest(arguments, session)?;
    session.deferred = Some(Box::new(move || {
        let pointer = play.pointer.clone();
        let mut last = pointer.lock().unwrap_or_else(|e| e.into_inner());
        run::mouse(&mut run::Live::new(play), &steps, &mut last).map(Value::String)
    }));
    Ok(Value::Null)
}

/// `actions`, which Studio requires before anything else.
fn steps(arguments: &Value) -> Result<Vec<Value>, String> {
    match arguments.get("actions") {
        Some(Value::Array(steps)) => Ok(steps.clone()),
        Some(_) => Err("actions must be an array".to_owned()),
        None => Err("Missing required argument: actions".to_owned()),
    }
}

/// The running playtest, once `datamodel_type` names its client.
fn playtest(arguments: &Value, session: &Session) -> Result<crate::mcp::PlayInput, String> {
    match text_argument(arguments, "datamodel_type") {
        None => return Err("datamodel_type is required".to_owned()),
        Some(CLIENT_DATAMODEL) => {}
        Some(other) => {
            return Err(format!(
                "{other} datamodel is not reachable: these tools act on the {CLIENT_DATAMODEL} \
                 of a running playtest"
            ))
        }
    }
    match (&session.play, session.studio.play) {
        (Some(play), PlayState::Playing | PlayState::ShowingLive) => Ok(play.clone()),
        (_, state) => Err(format!(
            "{CLIENT_DATAMODEL} datamodel is not available in {} mode",
            state.label()
        )),
    }
}
