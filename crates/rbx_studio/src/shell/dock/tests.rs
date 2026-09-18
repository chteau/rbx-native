use std::sync::Arc;

use gpui_kit::component::dock::{
    panel_handle, register_panel, BasePanel, DockArea, DockLayout, Panel as ComponentPanel,
    PanelEvent, PanelHandle,
};
// A glob `use gpui_kit::*;` (the style the rest of this module uses) also
// re-exports GPUI's `#[gpui::test]` macro under the plain name `test`,
// which shadows the standard library's `#[test]` attribute used just below
// and sends it into infinite recursion — so this file's GPUI imports stay
// explicit instead.
use gpui_kit::{
    App, AppContext, Context, Empty, EventEmitter, FocusHandle, Focusable, IntoElement, Render,
    TestAppContext, Window,
};

use super::{locate, Section};

// `panel_name` documents that its value must never change once chosen (it
// will be the persisted layout's panel key once settings land), so this is
// worth locking down even though the rest of this module needs a live GPUI
// window to exercise.
#[test]
fn every_section_has_a_distinct_stable_name() {
    let names = [
        Section::Viewport.name(),
        Section::Explorer.name(),
        Section::Properties.name(),
        Section::Output.name(),
        Section::Scripts.name(),
        Section::StyleEditor.name(),
    ];
    assert_eq!(
        names,
        [
            "Viewport",
            "Explorer",
            "Properties",
            "Output",
            "Script Editor",
            "Style Editor"
        ]
    );
}

/// A minimal dock panel standing in for `SectionPanel`: real `SectionPanel`s
/// only exist wrapped around a live `Shell`, which needs a real place file
/// and a GPU-backed viewer to construct (see `Shell::new`) — neither of
/// which a plain `cargo test` has on hand. This carries only what
/// `register_panels`'s reconstruction path actually depends on: a
/// `panel_name` the registry looks builders up by.
struct Probe {
    name: &'static str,
    focus_handle: FocusHandle,
}

impl Probe {
    fn new(name: &'static str, cx: &mut Context<Self>) -> Self {
        Self {
            name,
            focus_handle: cx.focus_handle(),
        }
    }
}

impl EventEmitter<PanelEvent> for Probe {}

impl Focusable for Probe {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

impl BasePanel for Probe {
    fn panel_name(&self) -> &'static str {
        self.name
    }
}

impl ComponentPanel for Probe {}

/// Reproduces the actual bug mechanism: a panel registry builder — exactly
/// what `register_panels` hands `gpui_component::dock::register_panel` — has
/// to return its rebuilt panel wrapped in `panel_handle` for the tab bar's
/// `PanelHandle::of` downcast to recover it later (which is what feeds a
/// panel's `dropdown_menu` and `zoom_control`, i.e. the "Orthographic" entry
/// and the "Zoom In" affordance). A bare `Arc::new(entity)` compiles just as
/// well — `Entity<P>` already satisfies the same `PanelView` trait object
/// through `gpui_base`'s own blanket impl — which is exactly how this bug
/// shipped: `register_panels`'s closure used to write `Arc::new(panel)`
/// where `build()`'s already used `panel_handle(panel)` right next to it.
///
/// This drives a saved-layout round trip (`DockArea::dump` /
/// `DockArea::load`) the same way a relaunch that restores a persisted
/// layout does, so it exercises the reconstruction path specifically, not
/// the first-launch path `build()` already gets right.
#[gpui_kit::test]
fn saved_layout_reconstruction_needs_panel_handle_not_a_bare_entity(cx: &mut TestAppContext) {
    let (area, cx) = cx.add_window_view(|window, cx| DockArea::new("dock-tests", None, window, cx));

    cx.update(|window, cx| {
        register_panel(cx, "Wrapped", |_, _, cx| {
            panel_handle(cx.new(|cx| Probe::new("Wrapped", cx)))
        });
        register_panel(cx, "Bare", |_, _, cx| {
            Arc::new(cx.new(|cx| Probe::new("Bare", cx)))
        });

        // The first-ever-launch path: both panels start out correctly
        // wrapped, exactly like `build()` does — the registry builders above
        // are only exercised once `load` below rebuilds from saved state.
        let layout = DockLayout::tabs()
            .panel_view(panel_handle(cx.new(|cx| Probe::new("Wrapped", cx))), cx)
            .panel_view(panel_handle(cx.new(|cx| Probe::new("Bare", cx))), cx);
        area.update(cx, |area, cx| area.set_center(layout, window, cx));
    });
    cx.run_until_parked();

    let state = cx.read(|cx| area.read(cx).dump(cx));
    cx.update(|window, cx| area.update(cx, |area, cx| area.load(state, window, cx).unwrap()));
    cx.run_until_parked();

    let (wrapped_recovered, bare_recovered) = cx.read(|cx| {
        let wrapped = locate(&area, "Wrapped", cx)
            .and_then(|(id, ..)| area.read(cx).panel(id))
            .map(|panel| PanelHandle::of(panel).is_some());
        let bare = locate(&area, "Bare", cx)
            .and_then(|(id, ..)| area.read(cx).panel(id))
            .map(|panel| PanelHandle::of(panel).is_some());
        (wrapped, bare)
    });

    assert_eq!(
        wrapped_recovered,
        Some(true),
        "a registry builder that returns panel_handle(panel) must survive reconstruction \
         — this is what register_panels does after the fix"
    );
    assert_eq!(
        bare_recovered,
        Some(false),
        "a registry builder that returns a bare Arc::new(panel) must NOT survive \
         reconstruction — this was the actual bug: it silently disabled every panel's \
         dropdown menu and zoom control the first time a saved layout was restored"
    );
}
