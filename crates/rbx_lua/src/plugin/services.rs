//! The services a place file has no instance for: `Selection`,
//! `ChangeHistoryService`, `HttpService`, `RunService`, `StudioService`,
//! `CollectionService`, `CoreGui` and `PluginGuiService` (the plugin's own
//! folders), plus the `settings()` global.

use mlua::{Lua, Result, Value};

use super::api::{Answer, UiId};

/// `game:GetService(name)` for the services above (one userdata per VM per
/// service, cached in the Lua registry); `None` for anything else, which
/// then falls back to the place's own instances.
///
/// - `Selection`: `Get()`, `Set(list)`, `Add(list)`, `Remove(list)`,
///   `SelectionChanged` (`Owner::Service("Selection")`). Reads
///   `Shared::context.selection` (dropping refs no longer in the DOM);
///   writes it back and sets `Shared::selection_written`. Rust-side
///   `Set` from inside a `SelectionChanged` handler does not re-fire.
/// - `ChangeHistoryService`: `SetWaypoint(name)` → `Request::Waypoint`;
///   `TryBeginRecording(name)` → an id string; `FinishRecording(id, op)`
///   → `Request::Waypoint`; `Undo`/`Redo` → requests; `GetCanUndo`/
///   `GetCanRedo` → true; `ResetWaypoints`, `SetEnabled` → no-ops;
///   `OnUndo`/`OnRedo` signals.
/// - `HttpService`: `JSONEncode`/`JSONDecode` (serde_json; arrays ⇄ tables
///   with 1..n keys; non-finite numbers are an error), `GenerateGUID(wrap)`
///   (uuid v4, braces when `wrap ~= false`), `UrlEncode`, `HttpEnabled`
///   (true), and yielding `RequestAsync{Url, Method, Headers, Body}` →
///   `{Success, StatusCode, StatusMessage, Headers, Body}`, `GetAsync(url)`
///   and `PostAsync(url, body, contentType?)` → body (error on non-2xx).
///   Before the first request from a Roblox plugin to a host, the request
///   waits on `PromptKind::HttpPermission` (answer remembered in the
///   plugin's settings under `"__rbxnative_http"`); a native plugin needs
///   `Permission::Http(host)` or `Http("*")` instead and is refused
///   otherwise.
/// - `RunService`: `IsStudio`/`IsEdit` true, `IsRunning`/`IsRunMode`/
///   `IsClient`/`IsServer` false; `Heartbeat`, `RenderStepped`, `Stepped`
///   signals (fired by the host on `Tick` with the delta).
/// - `StudioService`: `ActiveScript` (nil), `GetUserId()` 0,
///   `StudioLocaleId` "en_us", `GridSize` 1, `RotateIncrement` 15.
/// - `CollectionService`: `GetTagged(tag)`, `HasTag`, `AddTag`,
///   `RemoveTag`, `GetTags` over the DOM's `Tags` property
///   (`rbx_dom::attributes::{tags, encode_tags}`).
/// - `CoreGui` / `PluginGuiService`: the plugin's own folders
///   (`Env::core_gui`, `Env::plugin_gui_service`) as instances.
pub(crate) fn service(lua: &Lua, name: &str) -> Result<Option<Value>> {
    todo!()
}

/// Installs `settings()` (and `UserSettings()`): `settings().Studio.Theme`
/// with `Name` "Dark", `GetColor(styleGuideColor, modifier?)` from a fixed
/// dark palette keyed by `Enum.StudioStyleGuideColor` names (MainBackground,
/// MainText, Button, ButtonText, InputFieldBackground, Border, …, falling
/// back to mid grey) and `ThemeChanged` (never fires).
pub(crate) fn install_settings(lua: &Lua) -> Result<()> {
    todo!()
}

/// Continues HTTP requests that were waiting on a permission prompt:
/// allowed → sent, refused → their threads resume with an error
/// ("Http requests to <host> are not allowed").
pub(crate) fn http_permission_answered(lua: &Lua, prompt: UiId, answer: &Answer) -> Result<()> {
    todo!()
}

/// Resumes threads whose HTTP request finished (`Http::poll`, filtered to
/// this VM's plugin by the host).
pub(crate) fn http_finished(lua: &Lua, finished: Vec<super::http::Finished>) -> Result<()> {
    todo!()
}
