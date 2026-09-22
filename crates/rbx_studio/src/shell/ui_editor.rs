//! The UI Editor document: a Figma-style 2D canvas for one `ScreenGui` at a
//! time (the default sub-tab), beside the style sheet editor that used to be
//! this whole document (the Stylesheet sub-tab — `shell::style_panel`,
//! moved in unchanged).
//!
//! The canvas keeps no copy of what it edits:
//!
//! - **Selection** is `Shell::selection`, set through `Shell::select`/
//!   `extend_selection`/`reselect` exactly as an Explorer click sets it, so
//!   the Explorer and the Properties panel follow the canvas and the canvas
//!   follows them.
//! - **Edits** are property text written through `Shell::write_drag` —
//!   the Properties panel's own commit, one undo step per gesture on the
//!   one history every other edit uses — or, where they add or take away
//!   instances (a drawn element, a group, a stroke), `Shell::edit_gui_tree`,
//!   which logs the tree change and its first values as that one step.
//! - **The picture** is the 3D view's own GUI renderer drawing the chosen
//!   `ScreenGui` alone at the simulated resolution, on the same render
//!   thread (see `workspace_view::canvas`) — with no scene pass: the 3D view
//!   is not on screen while this is, so it stops drawing.
//!
//! While the canvas sub-tab is up, the Explorer lists only the place's UI
//! (see `Explorer::ui_items`); leaving it puts the full tree back.

mod arrange;
mod canvas;
mod draw;
mod gesture;
mod insert_bar;
mod inspector;
mod layout_overlay;
mod order;
mod responsive;
mod sidebar;
mod text_edit;
mod toolbar;
mod tree;

pub(super) use toolbar::size_field;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::tree::TreeItem;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::chrome::Document;
use super::layout::Panel;
use super::roving::Roving;
use super::Shell;
use crate::ui_canvas::{guides::Guide, Unit, View, PRESETS};
use crate::workspace_view::CanvasRequest;

/// Read once at startup by `Shell::new`; documented in `main`'s module doc
/// comment.
pub(crate) const UI_EDITOR_VARIABLE: &str = "RBX_STUDIO_UI_EDITOR";

/// The docks the canvas sets aside for the room: its own sidebar stands in
/// for Properties, Output is height the canvas wants more, and the
/// Viewport dock's settings are the 3D view's, which is not on screen.
const CANVAS_HIDES: [Panel; 3] = [Panel::Properties, Panel::Output, Panel::Viewport];

const SCREEN_CLASS: &str = "ScreenGui";
/// What a canvas can put up: every `LayerCollector` a place holds.
const ROOT_CLASSES: [&str; 3] = [SCREEN_CLASS, "BillboardGui", "SurfaceGui"];
const GUI_OBJECT_CLASS: &str = "GuiObject";

/// The document's two sub-tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Tab {
    #[default]
    Canvas,
    Stylesheet,
}

impl Tab {
    const ALL: [Tab; 2] = [Tab::Canvas, Tab::Stylesheet];

    fn key(self) -> &'static str {
        match self {
            Tab::Canvas => "ui-editor-canvas",
            Tab::Stylesheet => "ui-editor-stylesheet",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Tab::Canvas => "UI Editor",
            Tab::Stylesheet => "Stylesheet",
        }
    }
}

/// Everything the document keeps between frames. None of it is the place:
/// which screen is up, how the canvas is zoomed, the gesture in flight.
pub(super) struct UiEditor {
    pub(super) tab: Tab,
    /// The `ScreenGui` on the canvas. Follows the selection into whichever
    /// screen it lands in, and stays put while the selection is elsewhere —
    /// a click on a `Workspace` part must not blank the canvas.
    screen: Option<Ref>,
    /// The simulated screen, in pixels.
    resolution: (u32, u32),
    view: View,
    /// Whether `view` still follows the panel — true until the user pans or
    /// zooms, and again whenever the screen or the resolution changes.
    fitted: bool,
    gesture: Option<gesture::Gesture>,
    /// Which half of a `UDim` canvas edits write — the insert bar's switch.
    unit: Unit,
    /// The class the next press on the canvas draws, when a tool is armed.
    tool: Option<&'static str>,
    /// The sidebar's design fields' own state.
    inspector: inspector::Inspector,
    /// The field open over a text element, while its words are edited.
    text_edit: Option<text_edit::TextEdit>,
    /// Space is held: a drag pans.
    panning: bool,
    /// The element under the pointer, and whether Alt is held over it: the
    /// hover outline and the distance readout.
    hovered: Option<Ref>,
    measuring: bool,
    /// The guides the gesture in flight snapped onto.
    guides: Vec<Guide>,
    /// Where the canvas element was laid out, in window pixels.
    bounds: Rc<Cell<Bounds<Pixels>>>,
    focus: FocusHandle,
    width: Entity<InputState>,
    height: Entity<InputState>,
    nav: Roving,
    sidebar_scroll: ScrollHandle,
    /// Whether the Explorer was last given the UI-only rows.
    filtered: bool,
    /// Docks the canvas would set aside that were asked back by name this
    /// visit — see `Shell::hidden_panels`.
    unhidden: Vec<Panel>,
    _subscriptions: [Subscription; 2],
}

impl UiEditor {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Shell>) -> Self {
        let (_, width, height) = PRESETS[0];
        let field = |value: u32, window: &mut Window, cx: &mut Context<Shell>| {
            cx.new(|cx| InputState::new(window, cx).default_value(value.to_string()))
        };
        let width_field = field(width, window, cx);
        let height_field = field(height, window, cx);
        let commit = |input: &Entity<InputState>, cx: &mut Context<Shell>| {
            cx.subscribe(input, |shell, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    shell.apply_typed_resolution(cx);
                }
            })
        };
        let subscriptions = [commit(&width_field, cx), commit(&height_field, cx)];
        UiEditor {
            tab: Tab::default(),
            screen: None,
            resolution: (width, height),
            view: View {
                zoom: 1.0,
                pan: [0.0, 0.0],
            },
            fitted: true,
            gesture: None,
            unit: Unit::default(),
            tool: None,
            inspector: inspector::Inspector::default(),
            text_edit: None,
            panning: false,
            hovered: None,
            measuring: false,
            guides: Vec::new(),
            bounds: Rc::default(),
            focus: cx.focus_handle(),
            width: width_field,
            height: height_field,
            nav: Roving::horizontal(),
            sidebar_scroll: ScrollHandle::new(),
            filtered: false,
            unhidden: Vec::new(),
            _subscriptions: subscriptions,
        }
    }
}

/// The `ScreenGui`, `BillboardGui` or `SurfaceGui` `referent` is, or sits
/// in — the nearest, since that is the one that draws it — or `None`
/// outside every one.
pub(super) fn root_of(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> Option<Ref> {
    let mut current = Some(referent);
    while let Some(reference) = current {
        let instance = dom.get(reference)?;
        if ROOT_CLASSES
            .iter()
            .any(|class| database.is_subclass_of(instance.class(), class))
        {
            return Some(reference);
        }
        current = dom.parent(reference);
    }
    None
}

/// Whether the canvas's root takes the resolution the toolbar picks: a
/// `ScreenGui` is a device's screen, where a `BillboardGui`/`SurfaceGui`'s
/// canvas size is its own (see `rbx_viewer::GuiCanvas::size`).
pub(super) fn takes_resolution(dom: &WeakDom, database: &ReflectionDatabase, root: Ref) -> bool {
    dom.get(root)
        .is_none_or(|instance| database.is_subclass_of(instance.class(), SCREEN_CLASS))
}

/// Whether `referent` is a `GuiObject` — something with a `Position` and a
/// `Size` for the canvas to move.
pub(super) fn is_gui_object(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> bool {
    dom.get(referent)
        .is_some_and(|instance| database.is_subclass_of(instance.class(), GUI_OBJECT_CLASS))
}

impl Shell {
    /// Switches Row A's document — from a click on its tab, a script
    /// opening, the View menu. Opening the UI Editor lands on its canvas.
    pub(super) fn set_document(&mut self, document: Document, cx: &mut Context<Self>) {
        if document == Document::UiEditor && self.document != Document::UiEditor {
            self.ui.tab = Tab::Canvas;
        }
        self.document = document;
        self.sync_explorer_filter(cx);
        cx.notify();
    }

    /// The Stylesheet sub-tab, from the View menu's Style Editor item.
    pub(super) fn show_stylesheet(&mut self, cx: &mut Context<Self>) {
        self.set_ui_tab(Tab::Stylesheet, cx);
    }

    fn set_ui_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.ui.tab = tab;
        self.sync_explorer_filter(cx);
        cx.notify();
    }

    /// Whether the canvas is what the centre column shows.
    pub(super) fn ui_canvas_active(&self) -> bool {
        self.document == Document::UiEditor && self.ui.tab == Tab::Canvas
    }

    /// The Explorer's rows as things stand: the UI alone while the canvas
    /// is up, the place otherwise. Every place that pushes rows into the
    /// tree asks this, so none of them can put the wrong set back.
    pub(super) fn explorer_items(&self) -> Vec<TreeItem> {
        match self.ui_canvas_active() {
            true => self.explorer.ui_items(),
            false => self.explorer.items(self.show_all_services),
        }
    }

    /// The docks left out of the layout while the canvas is up. Nothing in
    /// the layout itself changes — leaving the canvas shows exactly what
    /// was there, and a dock that was shut stays shut.
    pub(super) fn hidden_panels(&self) -> Vec<Panel> {
        match self.ui_canvas_active() {
            true => CANVAS_HIDES
                .into_iter()
                .filter(|panel| !self.ui.unhidden.contains(panel))
                .collect(),
            false => Vec::new(),
        }
    }

    /// A dock asked back by name while the canvas has it set aside.
    pub(super) fn ui_unhide(&mut self, panel: Panel) {
        if self.hidden_panels().contains(&panel) {
            self.ui.unhidden.push(panel);
        }
    }

    /// Swaps the Explorer's rows when the canvas has just come up or gone —
    /// keeping the selected row, as `Shell::set_show_all_services` does.
    fn sync_explorer_filter(&mut self, cx: &mut Context<Self>) {
        let filtered = self.ui_canvas_active();
        if filtered == self.ui.filtered {
            return;
        }
        self.ui.filtered = filtered;
        // A fresh visit sets the docks aside again.
        self.ui.unhidden.clear();
        let items = self.explorer_items();
        let selected = self
            .selected()
            .and_then(|reference| self.explorer.item(reference));
        self.tree.update(cx, |tree, cx| {
            tree.set_items(items, cx);
            tree.set_selected_item(selected.as_ref(), cx);
        });
    }

    /// Puts the screen the selection is in on the canvas — a
    /// `BillboardGui`/`SurfaceGui` as much as a `ScreenGui` — called on
    /// every selection change, from wherever it came.
    pub(super) fn ui_follow_selection(&mut self) {
        self.ui.inspector.clear();
        let screen = self
            .selected()
            .and_then(|reference| root_of(&self.dom, &self.database, reference));
        if screen.is_some() && screen != self.ui.screen {
            self.ui.screen = screen;
            self.ui.fitted = true;
        }
    }

    /// What the render thread is to draw for the canvas: the screen at the
    /// simulated resolution, while the canvas is on screen and the screen
    /// still exists.
    pub(super) fn canvas_request(&self) -> Option<CanvasRequest> {
        let screen = self
            .ui
            .screen
            .filter(|&screen| self.dom.get(screen).is_some())?;
        self.ui_canvas_active().then_some(CanvasRequest {
            screen,
            size: self.ui.resolution,
        })
    }

    /// The size the canvas's root is laid out at: the frame on hand when it
    /// is the one asked for — the only word on a `BillboardGui`/
    /// `SurfaceGui`'s own size — or the resolution picked.
    pub(super) fn canvas_size(&self, cx: &App) -> (u32, u32) {
        let request = self.canvas_request();
        self.viewport
            .read(cx)
            .canvas()
            .filter(|canvas| Some(canvas.request) == request)
            .map_or(self.ui.resolution, |canvas| canvas.size)
    }

    /// The document itself: the sub-tab strip over whichever sub-tab is up.
    pub(super) fn ui_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let strip = self.ui_tabs(cx);
        let body = match self.ui.tab {
            Tab::Canvas => {
                let canvas = self.ui_canvas(window, cx);
                let sidebar = self.ui_sidebar(window, cx);
                gpui_kit::component::h_flex()
                    .size_full()
                    .child(div().flex_1().h_full().overflow_hidden().child(canvas))
                    .child(sidebar)
                    .into_any_element()
            }
            Tab::Stylesheet => self.style_editor(window, cx).into_any_element(),
        };
        gpui_kit::component::v_flex()
            .size_full()
            .bg(crate::tokens::dock())
            .child(strip)
            .child(div().flex_1().overflow_hidden().child(body))
            .into_any_element()
    }

    /// Picks a preset, or a typed size: a new screen to lay the tree out
    /// against, which the canvas then fits to the panel, and which the 3D
    /// view emulates. No window, no other document, no property changes.
    pub(super) fn set_resolution(&mut self, size: (u32, u32), cx: &mut Context<Self>) {
        self.ui.resolution = (size.0.clamp(1, 8192), size.1.clamp(1, 8192));
        self.ui.fitted = true;
        // The screen a GUI is designed at is the one the 3D view shows it
        // at too, or the viewport would lay it out at whatever size its
        // panel happens to be (see `WorkspaceView::set_screen`).
        let screen = self.ui.resolution;
        self.viewport
            .update(cx, |view, cx| view.set_screen(Some(screen), cx));
        cx.notify();
    }

    /// The width and height fields, shared by the canvas's toolbar and the
    /// Viewport dock's Screen row — never on screen together, since the
    /// canvas sets the dock aside.
    pub(super) fn ui_size_fields(&self) -> (Entity<InputState>, Entity<InputState>) {
        (self.ui.width.clone(), self.ui.height.clone())
    }

    /// The Viewport dock's "Viewport size": the 3D view back to its own
    /// size, the canvas keeping the resolution it had.
    pub(super) fn clear_viewport_screen(&mut self, cx: &mut Context<Self>) {
        self.viewport
            .update(cx, |view, cx| view.set_screen(None, cx));
        cx.notify();
    }

    /// `RBX_STUDIO_UI_EDITOR=1|<width>x<height>`: documented in `main`'s
    /// module doc comment. A size that does not parse still opens the
    /// canvas, at the resolution it already had.
    pub(super) fn apply_debug_ui_editor(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(UI_EDITOR_VARIABLE) else {
            return;
        };
        self.set_document(Document::UiEditor, cx);
        let size = spec
            .split_once('x')
            .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)));
        if let Some(size) = size {
            self.set_resolution(size, cx);
        }
    }

    /// The two size fields, read when either is committed. Text that is not
    /// a whole number leaves the resolution alone.
    fn apply_typed_resolution(&mut self, cx: &mut Context<Self>) {
        let read = |input: &Entity<InputState>, cx: &App| {
            input.read(cx).value().trim().parse::<u32>().ok()
        };
        if let (Some(width), Some(height)) = (read(&self.ui.width, cx), read(&self.ui.height, cx)) {
            if (width, height) != self.ui.resolution {
                self.set_resolution((width, height), cx);
            }
        }
    }
}

#[cfg(test)]
mod tests;
