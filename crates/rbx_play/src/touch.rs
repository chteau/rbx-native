//! Touch input for scripts that run in a Play session on a simulated touch
//! device.
//!
//! Sober hands every input to the game as a mouse, so on a simulated phone
//! a game's own touch code (`TouchStarted`, `TouchTap`, `TouchPinch`, an
//! `InputBegan` that checks for `Enum.UserInputType.Touch`) never runs. So
//! every `<expr>:GetService("UserInputService")` (and `:FindService`) in a
//! game script is wrapped in place, never across a new line, the way
//! `market` wraps purchases:
//!
//! ```lua
//! local UIS = require(game:GetService("ReplicatedStorage"):WaitForChild("__rbxnative_touch_input")).wrap(game:GetService("UserInputService"))
//! ```
//!
//! The probe's `__rbxnative_touch_input` module (`probe/touch_input.luau`)
//! is the other half. On a simulated touch device its proxy turns the
//! mouse into a finger (and the wheel into a two-finger pinch or rotate)
//! and reports a touch device; anywhere else, and on the server, `wrap`
//! returns the real service.

/// Name of the ModuleScript the rewritten scripts require.
pub const TOUCH_INPUT_NAME: &str = "__rbxnative_touch_input";
/// Inserted before a matching chain; the closing `)` follows the lookup.
pub const WRAP_OPEN: &str =
    "require(game:GetService(\"ReplicatedStorage\"):WaitForChild(\"__rbxnative_touch_input\")).wrap(";

/// Script names whose subtree is never rewritten: a game's fork of the
/// PlayerModule drives the stand-in thumbstick, as the stock one does, and
/// would draw its own touch controls if told the device is a touch device.
pub const SKIPPED_SUBTREES: &[&str] = &["PlayerModule", "ControlScript", "CameraScript"];

/// Wraps every `UserInputService` lookup in `source`. `Ok(None)` when there
/// is nothing to rewrite, `Err` with the parser's message when `source` does
/// not parse (the caller publishes it as is).
pub fn rewrite(source: &str) -> Result<Option<String>, String> {
    crate::service_wrap::wrap_lookups(source, "UserInputService", WRAP_OPEN)
}
