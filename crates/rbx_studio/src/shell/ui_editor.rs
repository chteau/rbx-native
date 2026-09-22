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
//! - **Edits** are `Position`/`Size`/`Rotation` text written through
//!   `Shell::write_drag` — the Properties panel's own commit, one undo step
//!   per gesture on the one history every other edit uses.
//! - **The picture** is the 3D view's own GUI renderer drawing the chosen
//!   `ScreenGui` alone at the simulated resolution, on the same render
//!   thread (see `workspace_view::canvas`) — with no scene pass: the 3D view
//!   is not on screen while this is, so it stops drawing.
//!
//! While the canvas sub-tab is up, the Explorer lists only the place's UI
//! (see `Explorer::ui_items`); leaving it puts the full tree back.

mod arrange;
mod canvas;
mod gesture;
mod toolbar;

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::tree::TreeItem;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::chrome::Document;
use super::roving::Roving;
use super::Shell;
use crate::ui_canvas::{guides::Guide, View, PRESETS};
use crate::workspace_view::CanvasRequest;

/// Read once at startup by `Shell::new`; documented in `main`'s module doc
/// comment.
pub(crate) const UI_EDITOR_VARIABLE: &str = "RBX_STUDIO_UI_EDITOR";

const SCREEN_CLASS: &str = "ScreenGui";
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
    /// Whether the Explorer was last given the UI-only rows.
    filtered: bool,
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
            hovered: None,
            measuring: false,
            guides: Vec::new(),
            bounds: Rc::default(),
            focus: cx.focus_handle(),
            width: width_field,
            height: height_field,
            nav: Roving::horizontal(),
            filtered: false,
            _subscriptions: subscriptions,
        }
    }
}

/// The `ScreenGui` `referent` is, or sits in — `None` outside every one,
/// which includes a `BillboardGui`/`SurfaceGui`: those are drawn in the
/// world, at a canvas size the part they sit on decides.
pub(super) fn screen_of(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
) -> Option<Ref> {
    let mut current = Some(referent);
    while let Some(reference) = current {
        let instance = dom.get(reference)?;
        if database.is_subclass_of(instance.class(), SCREEN_CLASS) {
            return Some(reference);
        }
        current = dom.parent(reference);
    }
    None
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

    /// Swaps the Explorer's rows when the canvas has just come up or gone —
    /// keeping the selected row, as `Shell::set_show_all_services` does.
    fn sync_explorer_filter(&mut self, cx: &mut Context<Self>) {
        let filtered = self.ui_canvas_active();
        if filtered == self.ui.filtered {
            return;
        }
        self.ui.filtered = filtered;
        let items = self.explorer_items();
        let selected = self
            .selected()
            .and_then(|reference| self.explorer.item(reference));
        self.tree.update(cx, |tree, cx| {
            tree.set_items(items, cx);
            tree.set_selected_item(selected.as_ref(), cx);
        });
    }

    /// Puts the screen the selection is in on the canvas — called on every
    /// selection change, from wherever it came.
    pub(super) fn ui_follow_selection(&mut self) {
        let screen = self
            .selected()
            .and_then(|reference| screen_of(&self.dom, &self.database, reference));
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

    /// The document itself: the sub-tab strip over whichever sub-tab is up.
    pub(super) fn ui_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let strip = self.ui_tabs(cx);
        let body = match self.ui.tab {
            Tab::Canvas => self.ui_canvas(window, cx),
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
    /// against, which the canvas then fits to the panel. Only the canvas
    /// changes — no window, no other document, no property.
    fn set_resolution(&mut self, size: (u32, u32), cx: &mut Context<Self>) {
        self.ui.resolution = (size.0.clamp(1, 8192), size.1.clamp(1, 8192));
        self.ui.fitted = true;
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
mod tests {
    use rbx_dom::WeakDom;
    use rbx_reflection::ReflectionDatabase;

    use super::{is_gui_object, screen_of};

    // What decides which screen is on the canvas: the one the selection is
    // in, found from any depth, and none for a canvas the world draws.
    #[test]
    fn the_canvas_screen_is_the_screen_gui_the_selection_sits_in() {
        let database = ReflectionDatabase::embedded();
        let mut dom = WeakDom::new();
        let starter = dom.new_instance("StarterGui", "StarterGui", None);
        let hud = dom.new_instance("ScreenGui", "Hud", Some(starter));
        let folder = dom.new_instance("Folder", "Bits", Some(hud));
        let label = dom.new_instance("TextLabel", "Title", Some(folder));
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let sign = dom.new_instance("Part", "Sign", Some(workspace));
        let surface = dom.new_instance("SurfaceGui", "Face", Some(sign));
        let text = dom.new_instance("TextLabel", "Text", Some(surface));

        assert_eq!(screen_of(&dom, &database, hud), Some(hud));
        assert_eq!(screen_of(&dom, &database, label), Some(hud));
        assert_eq!(screen_of(&dom, &database, text), None);
        assert_eq!(screen_of(&dom, &database, sign), None);

        assert!(is_gui_object(&dom, &database, label));
        assert!(!is_gui_object(&dom, &database, folder));
        assert!(
            !is_gui_object(&dom, &database, hud),
            "a screen has no Position"
        );
    }
}
