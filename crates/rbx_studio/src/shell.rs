//! Window layout: a Studio-style `DockArea` (see `shell::dock`) whose default
//! arrangement is the tabbed 3D view over the Output log on the left, the
//! Explorer and Properties stacked on the right — but any of the four can be
//! dragged to another edge or stacked as tabs.

mod command;
mod dock;
mod edit;
mod history;
mod keys;
mod output;
mod panels;
mod quality;
mod rows;
mod save;
mod selection;

use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::dock::DockArea;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::AppMenuBar;
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::tree::TreeState;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme, IndexPath, Sizable};
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::QualityLevel;

use crate::class_icons::SpriteSheet;
use crate::command_bar::{self, CommandBar};
use crate::explorer::Explorer;
use crate::history::{History, DEFAULT_CAP};
use crate::properties::Properties;
use crate::save::Format;
use crate::settings::Settings;
use crate::workspace_view::{AssetWarnings, PoseSynced, WorkspaceView};
use crate::Place;
use quality::{quality_labels, quality_row};
use selection::Selection;

const EXPLORER_WIDTH: f32 = 320.0;
const PROPERTIES_HEIGHT: f32 = 200.0;
const OUTPUT_HEIGHT: f32 = 180.0;
const QUALITY_WIDTH: f32 = 104.0;

/// The graphics quality dropdown's list: plain labels, since the mode a label
/// stands for is read back out of the label itself.
type QualityOptions = SearchableVec<SharedString>;

pub(crate) struct Shell {
    /// The top menu bar (File/Edit/Model/View); see `crate::menu_bar`. Kept
    /// as a field only so `Render for Shell` has an entity to mount — the
    /// menu's own state (which submenu is open) lives entirely inside it.
    menu_bar: Entity<AppMenuBar>,
    title: SharedString,
    viewport: Entity<WorkspaceView>,
    explorer: Rc<Explorer>,
    tree: Entity<TreeState>,
    /// Whether the Explorer lists every root the file has rather than just
    /// Studio's default service set. Persisted (see `settings`); every write
    /// goes through [`Shell::save_settings`].
    show_all_services: bool,
    /// The dropdown's current pick, kept alongside the `Select` entity itself
    /// so a settings write never has to reach into GPUI state to read it back.
    quality_choice: QualityLevel,
    /// Whether the viewport's main camera is orthographic rather than
    /// perspective. Persisted (see `settings`); every write goes through
    /// [`Shell::save_settings`].
    orthographic: bool,
    search: Entity<InputState>,
    filter: Entity<InputState>,
    properties: Properties,
    /// One open `Input` per Properties row currently being edited, keyed by
    /// property name; see `shell::edit`.
    edits: edit::Edits,
    /// Mirrors the tree's selected row (see [`Shell::sync_selection`]).
    selection: Selection,
    properties_scroll: ScrollHandle,
    dock_area: Entity<DockArea>,
    quality: Entity<SelectState<QualityOptions>>,
    /// The canonical, mutable tree a Command Bar script runs against; see
    /// `Place::dom`.
    dom: WeakDom,
    /// Whole-DOM snapshots either side of `self.dom`; see `shell::history`.
    history: History,
    database: ReflectionDatabase,
    icons: Option<SpriteSheet>,
    command_bar: CommandBar,
    /// Every Command Bar run, success or failure; see `shell::output`.
    output: output::OutputLog,
    /// Which levels the Output panel currently shows; see `shell::output`.
    output_filter: output::OutputFilter,
    output_scroll: ScrollHandle,
    /// The file `self.dom` was opened from and its on-disk format; see
    /// `shell::save`. Ctrl+S always writes back here, in this format,
    /// regardless of what the tree currently looks like.
    path: PathBuf,
    format: Format,
    /// Kept only to stay subscribed: dropping these unregisters the listeners.
    _subscriptions: [Subscription; 6],
}

impl Shell {
    pub(crate) fn new(
        title: impl Into<SharedString>,
        place: Place,
        quality: QualityLevel,
        show_all_services: bool,
        orthographic: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Place {
            explorer,
            properties,
            selected,
            camera,
            viewer,
            dom,
            database,
            icons,
            path,
            format,
        } = place;
        let items = explorer.items(show_all_services);

        let selector = cx.new(|cx| {
            let row = IndexPath::new(quality_row(quality));
            SelectState::new(SearchableVec::new(quality_labels()), Some(row), window, cx)
        });
        let picked = cx.subscribe(&selector, |shell, _, event: &SelectEvent<_>, cx| {
            shell.pick_quality(event, cx);
        });

        let preselected = selected.and_then(|reference| explorer.item(reference));
        let tree = cx.new(|cx| {
            let mut tree = TreeState::new(cx).items(items);
            tree.set_selected_item(preselected.as_ref(), cx);
            tree
        });
        // The tree owns the click: it selects and notifies, and this is how
        // the selection travels back out of it (there is no selection event).
        let clicked = cx.observe(&tree, |shell, tree, cx| shell.sync_selection(&tree, cx));

        let filter = cx
            .new(|cx| InputState::new(window, cx).placeholder("Filter Properties (Ctrl+Shift+P)"));
        let filtered = cx.subscribe(&filter, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        });

        let command_bar = CommandBar::new(window, cx);
        let entered = cx.subscribe(command_bar.input(), |shell, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::PressEnter { .. }) {
                shell.run_typed_command(cx);
            }
        });

        // The dock area's panels render by calling back into `Shell` (see
        // `shell::dock`), so it needs this entity's own handle before the
        // struct exists — valid as soon as `cx.new` starts building it.
        let dock_area = dock::build(cx.entity(), window, cx);

        let viewport = cx.new(|cx| {
            WorkspaceView::new(viewer, camera, quality, orthographic, selected, window, cx)
        });
        let camera_synced = cx.subscribe(&viewport, |shell, _, event: &PoseSynced, cx| {
            shell.sync_camera_pose(event.0, cx);
        });
        let asset_warnings = cx.subscribe(&viewport, |shell, _, event: &AssetWarnings, cx| {
            for warning in &event.0 {
                shell.output.push_warning(warning);
            }
            cx.notify();
        });

        // Built last of Shell::new's entities: its `Action` handlers close
        // over `cx.entity()`, so `Shell` must already be constructible (valid
        // as soon as `cx.new` starts building it, same as `dock_area` above).
        let menu_bar = crate::menu_bar::build(cx.entity(), cx);

        let mut shell = Shell {
            menu_bar,
            title: title.into(),
            viewport,
            explorer: Rc::new(explorer),
            tree,
            show_all_services,
            quality_choice: quality,
            orthographic,
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search")),
            filter,
            properties,
            edits: edit::Edits::default(),
            selection: Selection::new(selected),
            properties_scroll: ScrollHandle::new(),
            dock_area,
            quality: selector,
            dom,
            history: History::new(DEFAULT_CAP),
            database,
            icons,
            command_bar,
            output: output::OutputLog::default(),
            output_filter: output::OutputFilter::default(),
            output_scroll: ScrollHandle::new(),
            path,
            format,
            _subscriptions: [
                picked,
                clicked,
                filtered,
                entered,
                camera_synced,
                asset_warnings,
            ],
        };

        // A debugging aid for a screenshot that proves the bar works without
        // sending it synthetic input (see `AGENTS.md`'s safety rules): runs
        // exactly the pipeline Enter would, once, before the first frame.
        if let Ok(source) = std::env::var(command_bar::RUN_VARIABLE) {
            shell.run_command(&source, cx);
            // `RBX_STUDIO_SELECT` was already tried once, on the pristine DOM,
            // in `main::load` — too early to name anything the script just
            // created. Trying it again here is what lets a screenshot show
            // that without a click nothing else can send.
            if let Ok(name) = std::env::var(crate::SELECT_VARIABLE) {
                shell.select_by_name(&name, cx);
            }
        }

        // A debugging aid for the Properties panel itself, documented in
        // `shell::edit`: applied last, so it edits whatever the two blocks
        // above selected.
        if let Ok(spec) = std::env::var(edit::EDIT_VARIABLE) {
            shell.apply_debug_edit(&spec, window, cx);
        }

        // Explorer delete/insert debug aids (see `shell::keys`): applied
        // last, so an insert can parent under whatever is already selected.
        shell.apply_debug_explorer_action(cx);

        // `RBX_STUDIO_UNDO` (see `shell::history`): applied after every debug
        // mutation above, through the exact undo path a keypress would use —
        // a screenshot aid proving a just-applied mutation was reverted.
        shell.apply_debug_undo(cx);

        // `RBX_STUDIO_SAVE_AS` (see `shell::save`): applied last of all, so a
        // script can prove Ctrl+S round-trips whatever every block above just
        // mutated.
        shell.apply_debug_save(cx);

        shell
    }

    /// The selected instance, for whatever else wants to highlight it.
    pub(crate) fn selected(&self) -> Option<Ref> {
        self.selection.get()
    }

    /// Mirrors the render thread's latest free-camera pose into
    /// `Workspace.CurrentCamera.CFrame` (see `crate::camera::write_pose`) so
    /// Ctrl+S picks it up and a selected Camera's CFrame row stays live —
    /// subscribed to `WorkspaceView`'s throttled (≤5/s) `PoseSynced` event,
    /// never through [`Shell::push_history`]: flying the camera is not an
    /// edit, and a mouse-look frame landing on the undo stack every time this
    /// fires would blow its whole cap on nothing else. Nothing here may cost
    /// more than a re-render: this runs on the UI thread five times a second
    /// for as long as the camera moves, and every millisecond it takes is one
    /// the viewport's own frames wait behind (the Properties panel reads
    /// `self.dom` directly for that reason — no copy of the tree is kept). An
    /// open row editor is unaffected by the `cx.notify()` (see
    /// `shell::edit::edit_row`, which reuses its widget regardless of what a
    /// render recomputes), so a keystroke in Properties never gets clobbered
    /// by a pose update landing mid-edit.
    fn sync_camera_pose(&mut self, pose: rbx_viewer::Pose, cx: &mut Context<Self>) {
        crate::camera::write_pose(&mut self.dom, pose);
        cx.notify();
    }

    /// Reads the tree's selected row back into DOM terms. Called whenever the
    /// tree redraws, so it must stay quiet when nothing actually changed.
    fn sync_selection(&mut self, tree: &Entity<TreeState>, cx: &mut Context<Self>) {
        let selected = Selection::of_item(tree.read(cx).selected_item());
        if self.selection.set(selected) {
            // A different instance means a different row set; any open
            // editor belonged to the old one.
            self.edits.clear();
            self.viewport
                .update(cx, |viewport, _| viewport.set_selection(selected));
            cx.notify();
        }
    }

    /// Applies a pick from the dropdown. The labels are Roblox's own enum
    /// spelling, so the mode comes back out of them through the very parser the
    /// command line uses.
    fn pick_quality(&mut self, event: &SelectEvent<QualityOptions>, cx: &mut Context<Self>) {
        let SelectEvent::Confirm(picked) = event;
        let Some(mode) = picked
            .as_ref()
            .and_then(|label| label.parse::<QualityLevel>().ok())
        else {
            return;
        };

        self.viewport
            .update(cx, |viewport, cx| viewport.set_quality(mode, cx));
        self.quality_choice = mode;
        self.save_settings();
    }

    /// Whether the Explorer lists every root, for the dock's Explorer menu
    /// item (see `shell::dock`) to render its checked state.
    pub(super) fn show_all_services(&self) -> bool {
        self.show_all_services
    }

    /// Swaps the tree's rows for the other visibility set. The `TreeState`
    /// keeps its own copy, so the toggle has to push the new list into it
    /// rather than only flipping the flag this render reads.
    fn set_show_all_services(&mut self, show_all: bool, cx: &mut Context<Self>) {
        if show_all == self.show_all_services {
            return;
        }

        self.show_all_services = show_all;
        let items = self.explorer.items(show_all);
        // Replacing the rows drops the tree's selection; putting it back in the
        // same update keeps the observer from ever seeing the gap. A selected
        // root the default set hides is genuinely gone, and stays deselected.
        let selected = self
            .selected()
            .and_then(|reference| self.explorer.item(reference));
        self.tree.update(cx, |tree, cx| {
            tree.set_items(items, cx);
            tree.set_selected_item(selected.as_ref(), cx);
        });
        cx.notify();
        self.save_settings();
    }

    /// Whether the viewport's main camera is orthographic, for the dock's
    /// Viewport menu item (see `shell::dock`) to render its checked state.
    pub(super) fn orthographic(&self) -> bool {
        self.orthographic
    }

    /// Flips the viewport's main camera between perspective and orthographic
    /// projection — see `WorkspaceView::set_orthographic`.
    fn set_orthographic(&mut self, orthographic: bool, cx: &mut Context<Self>) {
        if orthographic == self.orthographic {
            return;
        }

        self.orthographic = orthographic;
        self.viewport.update(cx, |viewport, cx| {
            viewport.set_orthographic(orthographic, cx)
        });
        self.save_settings();
    }

    /// Writes the current quality pick, Explorer visibility and projection
    /// mode to disk. A settings file is tiny, so this runs synchronously on
    /// every change rather than debouncing; a write failure (e.g. no
    /// writable config directory) is not fatal and is silently dropped —
    /// losing a preference write is better than interrupting the editor
    /// over it.
    fn save_settings(&self) {
        let settings = Settings {
            quality: self.quality_choice,
            show_all_services: self.show_all_services,
            orthographic: self.orthographic,
        };
        let _ = settings.save();
    }

    /// The place file's name, shown as the dock's own Viewport tab title
    /// (see `shell::dock`) now that the viewport no longer draws a fake one.
    pub(super) fn title(&self) -> SharedString {
        self.title.clone()
    }

    /// The graphics-quality dropdown, relocated from the viewport's old fake
    /// tab bar into the dock's real Viewport title bar via `Panel::title_suffix`
    /// (see `shell::dock`) — the natural surviving home for a per-view control
    /// once that hand-rolled strip is gone.
    pub(super) fn quality_control(&self) -> impl IntoElement {
        div().px_1().w(px(QUALITY_WIDTH)).child(
            Select::new(&self.quality)
                .xsmall()
                .menu_width(px(QUALITY_WIDTH))
                .accessibility_label("Graphics quality"),
        )
    }

    fn viewport(&self, cx: &App) -> impl IntoElement {
        div()
            .size_full()
            .border_t_1()
            .border_color(cx.theme().border)
            .child(self.viewport.clone())
    }

    fn explorer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .child(
                h_flex().items_center().gap_1().p_1().my_1().child(
                    div()
                        .flex_1()
                        .child(Input::new(&self.search).small().py_1()),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(self.instance_tree(cx)),
            )
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            // Ctrl+S here rather than on one panel (contrast `instance_tree`'s
            // own `on_key_down`): a key event bubbles up from whatever holds
            // focus, and every panel — Explorer, Properties, viewport,
            // Command Bar — sits below this container, so a save works no
            // matter which one is focused.
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, _, cx| {
                shell.handle_shell_key(&event.keystroke, cx);
            }))
            .child(crate::menu_bar::bar(&self.menu_bar, cx))
            .child(
                div()
                    .flex_1()
                    .overflow_hidden()
                    .child(self.dock_area.clone()),
            )
            .child(self.command_bar.render(cx))
    }
}
