//! Instance events and the plugin-only members of instances, through
//! `crate::hooks`.

use mlua::{Lua, Result, Value};
use rbx_dom::{Change, Ref};

use super::api::{GuiEvent, HandleEvent};
use crate::ctx::Ctx;
use crate::hooks::Hooks;

/// The hooks every plugin VM installs.
///
/// `index` answers, by class (`Ctx::database().is_subclass_of`):
/// - every instance: `Changed`, `ChildAdded`, `ChildRemoved`,
///   `DescendantAdded`, `DescendantRemoving`, `AncestryChanged`,
///   `Destroying`, `AttributeChanged` signals; `GetPropertyChangedSignal(
///   name)` and `GetAttributeChangedSignal(name)` functions;
///   `GetAttribute`, `SetAttribute`, `GetAttributes` (over the
///   `Attributes` property via `rbx_dom::attributes`);
///   `WaitForChild(name, timeout?)` (the child, or nil after a warning —
///   nothing loads later in the editor);
/// - `GuiObject`: `MouseEnter`, `MouseLeave`, `MouseMoved`,
///   `MouseButton1Down`, `MouseButton1Up`, `MouseButton2Down`,
///   `MouseButton2Up`, `MouseWheelForward`, `MouseWheelBackward`,
///   `InputBegan`, `InputEnded`, `InputChanged`; `GuiButton`:
///   `MouseButton1Click`, `MouseButton2Click`, `Activated`; `TextBox`:
///   `Focused`, `FocusLost`, `CaptureFocus()`, `ReleaseFocus()`,
///   `IsFocused()`;
/// - `HandlesBase` (`Handles`, `ArcHandles`): `MouseButton1Down`,
///   `MouseButton1Up`, `MouseDrag`, `MouseEnter`, `MouseLeave`;
///   `HandleAdornment`: `MouseButton1Down`, `MouseButton1Up`,
///   `MouseEnter`, `MouseLeave`;
/// - `BindableEvent`: `Event`, `Fire(...)`; `BindableFunction`:
///   `Invoke(...)` (calls `OnInvoke` synchronously);
/// - `PluginGui` / `DockWidgetPluginGui` roots: `BindToClose(fn?)`,
///   `WindowFocused`, `WindowFocusReleased`, `GetRelativeMousePosition()`,
///   `HostWidgetWasRestored` (false).
///
/// `new_index` handles `OnInvoke` on a `BindableFunction` (stored in the
/// Lua registry), and for a widget root keeps `Ui` in step with `Title`
/// and `Enabled` (returning false so the property is written too).
/// `service` delegates to `services::service`.
pub(crate) struct PluginHooks;

impl Hooks for PluginHooks {
    fn index(&self, lua: &Lua, ctx: &Ctx, referent: Ref, class: &str, key: &str) -> Result<Option<Value>> {
        todo!()
    }

    fn new_index(&self, lua: &Lua, ctx: &Ctx, referent: Ref, class: &str, key: &str, value: &Value) -> Result<bool> {
        todo!()
    }

    fn service(&self, lua: &Lua, ctx: &Ctx, name: &str) -> Result<Option<Value>> {
        super::services::service(lua, name)
    }
}

/// Fires this VM's instance signals for a change log, reading the DOM as it
/// is now: `Property` → `Changed(name)` and `Changed:<name>` (and
/// `AttributeChanged`/`AttributeChanged:<n>` for `Attributes`, comparing
/// nothing — every attribute name present fires); `Parent` → the child's
/// `AncestryChanged(child, parent)` (and its descendants'), the old
/// parent's `ChildRemoved`, the new one's `ChildAdded`, and
/// `DescendantRemoving`/`DescendantAdded` up both ancestor chains;
/// `Added` with a parent → as a `Parent` change from nil; `Removed` →
/// `Destroying` on the instance. Only connected signals cost anything.
pub(crate) fn fire_changes(lua: &Lua, changes: &[Change]) -> Result<()> {
    todo!()
}

/// Fires the GUI signals for one widget input. `Click` → `MouseButton1Click`
/// (`button` 1) or `MouseButton2Click`, then `Activated` on a `GuiButton`;
/// `Down`/`Up` → `MouseButton{n}Down/Up(x, y)`; `Enter`/`Leave`/`Move` →
/// `MouseEnter`/`MouseLeave`/`MouseMoved(x, y)`; `TextChanged(text)` →
/// writes `Text` (the change log then fires `Changed`); `Focused` /
/// `FocusLost { enter }` → `Focused()` / `FocusLost(enter, nil)`;
/// `Scroll` → `MouseWheelForward`/`Backward`. Also remembers the last
/// pointer position per widget for `GetRelativeMousePosition`.
pub(crate) fn fire_gui(lua: &Lua, event: &GuiEvent) -> Result<()> {
    todo!()
}

/// Fires `Handles`/`ArcHandles`/`HandleAdornment` signals: `Down{face}` →
/// `MouseButton1Down(Enum.NormalId)`, `Drag{face, distance}` →
/// `MouseDrag(Enum.NormalId, distance)`, `ArcDrag{axis, angle,
/// delta_radius}` → `MouseDrag(Enum.Axis, angle, deltaRadius)`, and so on.
pub(crate) fn fire_handle(lua: &Lua, event: &HandleEvent) -> Result<()> {
    todo!()
}
