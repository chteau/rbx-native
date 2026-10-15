//! The `plugin` global and what it creates: toolbars and buttons, plugin
//! actions and menus, dock widgets, settings, and rbx-native's additions.

use mlua::{Lua, Result, UserData, UserDataMethods};

use super::api::{Answer, UiId};

/// The `plugin` object. Members:
/// - `Name`; `CreateToolbar(name)` → toolbar (one per name per plugin);
///   toolbar `CreateButton(id, tooltip, icon, text?)` → button with
///   `Click` signal, `SetActive(bool)`, settable `Enabled`, `Icon`,
///   `ClickableWhenViewportHidden`;
/// - `CreatePluginAction(id, text, tooltip, icon?, allowBinding?)` →
///   action with `Triggered`, `Enabled`; `CreatePluginMenu(id, title?,
///   icon?)` → menu with `AddAction`, `AddNewAction(id, text, icon?)`,
///   `AddSeparator`, `AddMenu`, `Clear`, `Title`, `Icon`, yielding
///   `ShowAsync()` (→ `PromptKind::Menu`, returns the chosen action or nil);
/// - `CreateDockWidgetPluginGui(id, info)` → a `DockWidgetPluginGui`
///   instance under `Env::plugin_gui_service` (`Title` = id until set,
///   `Enabled` from the info), registered as a `Widget` (same id twice
///   returns the same widget); global `DockWidgetPluginGuiInfo.new(
///   initDockState?, initEnabled?, overrideRestore?, floatX?, floatY?,
///   minW?, minH?)`;
/// - `GetSetting(key)`, `SetSetting(key, value)` (JSON-able values only,
///   persisted to `Shared::settings_dir/<plugin name>.json`);
/// - `Activate(exclusive)`, `Deactivate()`, `IsActivated()`,
///   `IsActivatedWithExclusiveMouse()`, `Deactivation` and `Unloading`
///   signals; `GetMouse()` → `PluginMouse` (`Hit` CFrame, `Target`,
///   `Origin`, `UnitRay`-free, from `Context::mouse_*`; `Button1Down`/
///   `Button1Up`/`Move` signals that never fire yet);
/// - `OpenScript(script, line?)` → `Request::OpenScript`;
///   `SelectRibbonTool`, `OpenWikiPage`, `StartDrag` → no-ops;
///   `GetStudioUserId()` 0; `Union`/`Separate`/`Negate` → error naming
///   the missing CSG support;
/// - rbx-native additions: yielding `PromptColor3(initial?)` → Color3 or
///   nil, yielding `PromptBrickColor(initial?)` → BrickColor or nil,
///   `CreateFloatingWidget(id, {Size: Vector2, Position: Vector2?,
///   LightDismiss: bool?})` → a widget instance of kind `Floating`,
///   `GetDebuggerManager()` → `debugger_api::manager`.
pub(crate) struct LuaPlugin;

impl UserData for LuaPlugin {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        todo!()
    }
}

/// Installs `plugin` and `DockWidgetPluginGuiInfo`. Call before sandboxing.
pub(crate) fn install(lua: &Lua) -> Result<()> {
    todo!()
}

/// Finds or creates this plugin's toolbar `toolbar` and a button `key` on
/// it (the same key twice returns the same button), as `CreateButton`
/// does; its `Click` signal is `SignalKey::new(Owner::Ui(id), "Click")`.
/// For `rbxnative.command`.
pub(crate) fn toolbar_button(lua: &Lua, toolbar: &str, key: &str, tooltip: &str, icon: &str, text: &str) -> Result<UiId> {
    todo!()
}

/// Resolves a `PromptColor3`/`PromptBrickColor`/`ShowAsync` prompt this VM
/// is waiting on. Returns false when the id is not one of its prompts.
pub(crate) fn answer(lua: &Lua, prompt: UiId, answer: &Answer) -> Result<bool> {
    todo!()
}
