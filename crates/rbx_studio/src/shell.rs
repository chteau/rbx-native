//! Window layout: a Studio-style `DockArea` (see `shell::dock`) whose default
//! arrangement is the tabbed 3D view over the Output log on the left, the
//! Explorer and Properties stacked on the right — but any of the four can be
//! dragged to another edge or stacked as tabs.

mod align;
mod chrome;
mod command;
mod drag;
mod edit;
mod folder_color;
mod group;
mod history;
mod keys;
mod menu;
mod output;
mod panels;
mod quality;
mod reparent;
mod ribbon;
mod roving;

pub(crate) use roving::install as install_key_bindings;
mod tree_keys;

mod rows;
mod save;
mod script_panel;
mod scripts;
mod scroll;
mod scrub;
mod selection;
mod style_panel;
mod toolbar;
mod tooltip;
mod workspace;

use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::menu::AppMenuBar;
use gpui_kit::component::select::{SearchableVec, Select, SelectEvent, SelectState};
use gpui_kit::component::tree::TreeState;
use gpui_kit::component::{v_flex, IndexPath, Sizable};
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick::Selected;
use rbx_viewer::QualityLevel;

use crate::align::Options as AlignOptions;
use crate::class_icons::IconPack;
use crate::command_bar::{self, CommandBar};
use crate::explorer::Explorer;
use crate::folder_colors::FolderColors;
use crate::history::{History, DEFAULT_CAP};
use crate::pacing::UnfocusedFps;
use crate::properties::Properties;
use crate::save::Format;
use crate::script_editor::ScriptEditor;
use crate::settings::Settings;
use crate::tokens;
use crate::transform::{Targets, Transform};
use crate::workspace_view::{AssetWarnings, Opened, PoseSynced, ViewportAction, WorkspaceView};
use crate::Place;
use chrome::{Document, Drag};
use menu::MenuId;
use quality::{quality_labels, quality_row};
use selection::{outlined, Selection};
use toolbar::snap::SnapFields;

/// The quality dropdown's width. Scaled like every other size, or its
/// longest label ("Automatic") is clipped the moment the UI scale grows.
fn quality_width() -> Pixels {
    tokens::scaled_width(132.)
}

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
    /// Whether the viewport's top-right orientation indicator draws at all.
    /// Persisted the same way `orthographic` is, for the same reason: it's
    /// meant to be a durable preference, not a per-session debug switch.
    axis_indicator: bool,
    /// Whether a part in front of the selection hides its outline box.
    /// Persisted the same way `orthographic` is (see `settings`).
    selection_occluded: bool,
    /// Which of the class icon kit's two variants the Explorer draws.
    /// Persisted (see `settings`); every write goes through
    /// [`Shell::save_settings`].
    icon_pack: IconPack,
    /// Whether the viewport's corner label shows its frame-rate readout —
    /// the Stats toggle, next to Orthographic in the same overflow menu (see
    /// `shell::dock`). Session-only, unlike the two settings above: real
    /// Studio's own `Window > Performance > Stats` doesn't persist across
    /// restarts either, so this one lazily doesn't bother with `settings`.
    stats_shown: bool,
    /// The render loop's frame rate cap while the window is unfocused (see
    /// `pacing::FocusPacing`). Persisted (see `settings`); every write goes
    /// through [`Shell::save_settings`].
    unfocused_fps: UnfocusedFps,
    /// The three composite widgets that are one Tab stop each: the
    /// document tab strip, the ribbon's category tabs, and the ribbon's own
    /// controls. See `shell::roving`.
    document_nav: roving::Roving,
    ribbon_tabs_nav: roving::Roving,
    ribbon_nav: roving::Roving,
    /// The Properties panel's own group. Without it every section header
    /// and every checkbox is its own Tab stop — measured at 32 presses to
    /// get from the panel back to the ribbon, which defeats the entire
    /// point of Tab moving between regions.
    properties_nav: roving::Roving,
    /// The window's Tab order, handed out afresh every render — see
    /// `shell::roving::TabOrder`.
    tab_order: roving::TabOrder,
    /// The Explorer tree's keyboard door: the wrapper that holds the
    /// Explorer's place in the Tab order and hands focus on to the tree
    /// itself, whose own handle the toolkit keeps private.
    tree_focus_handle: FocusHandle,
    /// An explicit reduce-motion choice, or `None` to follow the desktop.
    reduce_motion: Option<bool>,
    /// A numeric field being dragged — see `shell::scrub`.
    scrub: Option<scrub::Scrub>,
    /// The Explorer's type-ahead buffer — see `shell::tree_keys`.
    typeahead: tree_keys::Typeahead,
    search: Entity<InputState>,
    filter: Entity<InputState>,
    properties: Properties,
    /// One open `Input` per Properties row currently being edited, keyed by
    /// property name; see `shell::edit`.
    edits: edit::Edits,
    /// Mirrors the tree's selected row (see [`Shell::sync_selection`]).
    selection: Selection,
    /// The `BasePart` the cursor was last resolved to be over, if any — see
    /// `shell::drag::hover_in_viewport`. Kept here, alongside `selection`
    /// above, purely to dedupe: the viewport reports cursor motion on every
    /// pixel, and only an actual change is worth a command down to the
    /// render thread.
    hovered: Vec<Selected>,
    /// Every part the selection covers — the selected parts and every part
    /// beneath a selected `Model`: exactly the referents a drag writes, kept
    /// so `shell::command::refresh_for` can tell a drag's own writes from
    /// an edit to something else without walking the tree per mouse move.
    /// Re-read whenever the draggers' targets are (see
    /// `Shell::sync_viewport_selection`, `Shell::reflect_changes`).
    covered: HashSet<Ref>,
    /// Every script open in the Script Editor panel; see `shell::scripts`.
    scripts: ScriptEditor,
    properties_scroll: ScrollHandle,
    /// The Style Editor panel's open fields and last error; see
    /// `shell::style_panel`.
    style_edits: style_panel::StyleEdits,
    style_scroll: ScrollHandle,
    quality: Entity<SelectState<QualityOptions>>,
    /// The canonical, mutable tree a Command Bar script runs against; see
    /// `Place::dom`.
    dom: WeakDom,
    /// Whole-DOM snapshots either side of `self.dom`; see `shell::history`.
    history: History,
    database: ReflectionDatabase,
    command_bar: CommandBar,
    /// Every Command Bar run, success or failure; see `shell::output`.
    output: output::OutputLog,
    /// Which levels the Output panel currently shows; see `shell::output`.
    output_filter: output::OutputFilter,
    /// Whether Output rows print their `HH:MM:SS.SSS` timestamp; toggled from
    /// the panel's overflow menu (see `shell::dock`'s `dropdown_menu`). Not
    /// persisted — resets to off each launch, same as `output_filter` above.
    output_show_timestamps: bool,
    output_scroll: ScrollHandle,
    /// The file `self.dom` was opened from and its on-disk format; see
    /// `shell::save`. Ctrl+S always writes back here, in this format,
    /// regardless of what the tree currently looks like.
    path: PathBuf,
    format: Format,
    /// This place's `Folder` colour tags; see `shell::folder_color`.
    folder_colors: FolderColors,
    /// Which transform tool the toolbar has active, and whether its draggers
    /// follow the part's own axes — see `crate::transform`. Owned here because
    /// the toolbar renders from it; pushed down to the viewport, which
    /// hit-tests against it, whenever it changes.
    transform: Transform,
    /// The two snap increment fields' live text — see `shell::toolbar::snap`.
    snap_fields: SnapFields,
    /// The Align tool's current toggles (axes, Min/Center/Max, World/Local,
    /// Selection Bounds/Active Object) — see `crate::align`/`shell::align`.
    align: AlignOptions,
    /// Which of the ribbon's own category tabs is showing — see
    /// `shell::ribbon`. Session-only: real Studio's own ribbon always opens
    /// back on Home too, and there's nothing here worth writing to
    /// `settings` over.
    ribbon_tab: ribbon::Tab,
    /// Which editor the centre column shows (Row A). Switching it swaps
    /// *only* that column's contents — see `shell::workspace`.
    document: Document,
    /// Which dropdown is open, if any. Held here rather than inside each
    /// popover so that opening one closes the last, and so a menu item can
    /// close the menu it was clicked in (see `shell::menu`).
    open_menu: Option<MenuId>,
    /// The three column/dock sizes the user can drag, and the drag in
    /// progress if there is one. Session-only, like the rest of the
    /// layout: the dock layout used to be persisted, and restoring a saved
    /// one is a feature to redo deliberately rather than inherit.
    properties_width: f32,
    explorer_width: f32,
    output_height: f32,
    output_collapsed: bool,
    drag: Option<Drag>,
    /// Kept only to stay subscribed: dropping these unregisters the listeners.
    _subscriptions: [Subscription; 12],
}

impl Shell {
    pub(crate) fn new(
        title: impl Into<SharedString>,
        place: Place,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Settings {
            quality,
            show_all_services,
            orthographic,
            axis_indicator,
            selection_occluded,
            icon_pack,
            unfocused_fps,
            font_scale,
            large_targets,
            reduce_motion,
            properties_width,
            explorer_width,
            output_height,
            output_collapsed,
        } = settings;
        // Before anything renders: every size token is read through these,
        // so a scale or target floor applied after the first frame would
        // flash.
        tokens::set_font_scale(font_scale);
        tokens::set_large_targets(large_targets);
        if let Some(reduced) = reduce_motion {
            tokens::set_reduced_motion(reduced);
        }
        let Place {
            explorer,
            properties,
            selected,
            camera,
            viewer,
            dom,
            database,
            path,
            format,
            folder_colors,
        } = place;
        let items = explorer.items(show_all_services);

        let selector = cx.new(|cx| {
            let row = IndexPath::new(quality_row(quality));
            SelectState::new(SearchableVec::new(quality_labels()), Some(row), window, cx)
        });
        let picked = cx.subscribe(&selector, |shell, _, event: &SelectEvent<_>, cx| {
            shell.pick_quality(event, cx);
        });

        // Tab lands on the Explorer's wrapper; this hands focus straight on
        // to the tree inside it, which is what activates the toolkit's
        // `Tree` key context and makes the arrow contract reachable at all.
        // The wrapper never keeps focus for more than an instant.
        let tree_focus_handle = cx.focus_handle();
        let tree_focused = cx.on_focus_in(&tree_focus_handle, window, |shell, window, cx| {
            let tree = shell.tree.clone();
            tree.update(cx, |tree, cx| {
                tree.focus(window, cx);
                // The APG's "on focus" rule: a tree with nothing selected
                // puts the cursor on its first node. Without this, arriving
                // by Tab lands on a tree with no visible position at all —
                // the keys work, but there is nothing to see them working
                // on.
                if tree.selected_index().is_none() {
                    tree.set_selected_index(Some(0), cx);
                }
            });
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

        let initial_outline = outlined(&dom, &database, &Vec::from_iter(selected));
        let viewport = cx.new(|cx| {
            let opened = Opened {
                viewer,
                dom: dom.clone(),
            };
            WorkspaceView::new(
                opened,
                camera,
                quality,
                orthographic,
                axis_indicator,
                selection_occluded,
                unfocused_fps,
                initial_outline,
                window,
                cx,
            )
        });
        let camera_synced = cx.subscribe(&viewport, |shell, _, event: &PoseSynced, cx| {
            shell.sync_camera_pose(event.0, cx);
        });
        // `subscribe_in` rather than `subscribe`: one viewport action (the
        // snap increment shortcut) moves the caret, and focus needs a window.
        let viewport_actions = cx.subscribe_in(
            &viewport,
            window,
            |shell, _, event: &ViewportAction, window, cx| {
                shell.handle_viewport_action(event, window, cx);
            },
        );
        let asset_warnings = cx.subscribe(&viewport, |shell, _, event: &AssetWarnings, cx| {
            for warning in &event.0 {
                shell.output.push_warning(warning);
            }
            cx.notify();
        });

        // Built last of Shell::new's entities: its `Action` handlers close
        // over `cx.entity()`, so `Shell` must already be constructible —
        // valid as soon as `cx.new` starts building it.
        let menu_bar = crate::menu_bar::build(cx.entity(), cx);

        let transform = Transform::default();
        let (snap_fields, [translate_typed, rotate_typed, translate_stepped, rotate_stepped]) =
            SnapFields::new(transform, window, cx);

        let initial_targets = Targets::read(&dom, &database, &Vec::from_iter(selected));
        let mut shell = Shell {
            menu_bar,
            title: title.into(),
            viewport,
            explorer: Rc::new(explorer),
            tree,
            show_all_services,
            quality_choice: quality,
            orthographic,
            axis_indicator,
            selection_occluded,
            icon_pack,
            stats_shown: false,
            unfocused_fps,
            document_nav: roving::Roving::horizontal(),
            ribbon_tabs_nav: roving::Roving::horizontal(),
            ribbon_nav: roving::Roving::horizontal(),
            properties_nav: roving::Roving::vertical(),
            tab_order: roving::TabOrder::default(),
            reduce_motion,
            scrub: None,
            tree_focus_handle,
            typeahead: tree_keys::Typeahead::default(),
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search")),
            filter,
            properties,
            edits: edit::Edits::default(),
            selection: Selection::new(selected),
            hovered: Vec::new(),
            covered: HashSet::new(),
            scripts: ScriptEditor::default(),
            properties_scroll: ScrollHandle::new(),
            style_edits: style_panel::StyleEdits::default(),
            style_scroll: ScrollHandle::new(),
            quality: selector,
            dom,
            history: History::new(DEFAULT_CAP),
            database,
            command_bar,
            output: output::OutputLog::default(),
            output_filter: output::OutputFilter::default(),
            output_show_timestamps: false,
            output_scroll: ScrollHandle::new(),
            path,
            format,
            folder_colors,
            transform,
            snap_fields,
            align: AlignOptions::default(),
            ribbon_tab: ribbon::Tab::default(),
            document: Document::default(),
            open_menu: None,
            // A saved layout wins over the default; a zero means nothing
            // was saved (see `Settings`).
            properties_width: workspace::saved_or_default(
                properties_width,
                workspace::properties_width(),
            ),
            explorer_width: workspace::saved_or_default(
                explorer_width,
                workspace::explorer_width(),
            ),
            output_height: workspace::saved_or_default(output_height, workspace::OUTPUT_HEIGHT),
            output_collapsed,
            drag: None,
            _subscriptions: [
                tree_focused,
                picked,
                clicked,
                filtered,
                entered,
                camera_synced,
                viewport_actions,
                asset_warnings,
                translate_typed,
                rotate_typed,
                translate_stepped,
                rotate_stepped,
            ],
        };

        // Where the draggers go for whatever the place opened with selected.
        // `sync_selection` only runs on a selection *change*, so without this
        // an instance selected before the first frame would be outlined and
        // gizmoed but not draggable until it was selected again.
        shell
            .viewport
            .update(cx, |viewport, _| viewport.set_targets(initial_targets));
        shell.sync_snap_neighbours(cx);

        // `RBX_STUDIO_TOOL` (see `shell::toolbar`). Before the Command Bar
        // block below rather than after it: a script's reload rebuilds the
        // renderer, so setting the tool first is what makes a screenshot
        // taken afterwards prove the rebuild *kept* it (see
        // `rbx_viewer::view::View`) instead of only proving it was set last.
        shell.apply_debug_tool(cx);

        // `RBX_STUDIO_SELECT=<name>[,<name>...]`: `main::load` already
        // resolved a single name into the initial `Place.selected` before
        // the window opened (too early for a comma list — it looks up one
        // literal name and finds nothing for a name containing a comma), so
        // this is what actually applies a multi-instance selection — a
        // debugging aid for a screenshot of the outline/gizmo over more than
        // one part, since nothing else can send the viewport a
        // `Shift`/`Ctrl`/`Cmd`-click on the editor's behalf.
        if let Ok(spec) = std::env::var(crate::SELECT_VARIABLE) {
            shell.apply_debug_select(&spec, cx);
        }

        // `RBX_STUDIO_ALIGN` (see `shell::align`): applied right after
        // selection, so it aligns whatever the file itself or
        // `RBX_STUDIO_SELECT` just selected — a screenshot aid for the Align
        // tool, since nothing else can click its popover on the editor's
        // behalf.
        shell.apply_debug_align(cx);

        // `RBX_STUDIO_DRAG` (see `shell::drag::debug`): applied right after
        // selection, so it moves whatever the file itself or
        // `RBX_STUDIO_SELECT` just selected — a screenshot aid for a group
        // drag.
        shell.apply_debug_drag(cx);

        // `RBX_STUDIO_RESIZE` (same module): the same, for a Scale drag of
        // the selected part.
        shell.apply_debug_resize(cx);

        // A debugging aid for a screenshot that proves the bar works without
        // sending it synthetic input (see `AGENTS.md`'s safety rules): runs
        // exactly the pipeline Enter would, once, before the first frame.
        if let Ok(source) = std::env::var(command_bar::RUN_VARIABLE) {
            shell.run_command(&source, cx);
            // Re-applied: too early above to name anything the script just
            // created. Trying it again here is what lets a screenshot show
            // that without a click nothing else can send.
            if let Ok(spec) = std::env::var(crate::SELECT_VARIABLE) {
                shell.apply_debug_select(&spec, cx);
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

        // `RBX_STUDIO_GROUP` / `RBX_STUDIO_UNGROUP` (see `shell::group`):
        // applied right after, so a group can wrap whatever the blocks above
        // just selected or inserted.
        shell.apply_debug_group(cx);

        // `RBX_STUDIO_OPEN_SCRIPT` (see `shell::scripts`): after the Command
        // Bar block above, so a script that block just created can be opened.
        shell.apply_debug_open_script(window, cx);

        // `RBX_STUDIO_UNDO` (see `shell::history`): applied after every debug
        // mutation above, through the exact undo path a keypress would use —
        // a screenshot aid proving a just-applied mutation was reverted.
        shell.apply_debug_undo(cx);

        // `RBX_STUDIO_STYLE_EDITOR` (see `shell::style_panel`): after the
        // selection blocks above, so the edit it may carry lands on whatever
        // `StyleRule` they selected.
        shell.apply_debug_style_editor(cx);

        // `RBX_STUDIO_SAVE_AS` (see `shell::save`): applied last of all, so a
        // script can prove Ctrl+S round-trips whatever every block above just
        // mutated.
        shell.apply_debug_save(cx);

        shell
    }

    /// The selection's anchor — see `shell::selection::Selection`'s own doc
    /// comment — for whatever still only understands one instance at a time
    /// (the Properties panel, a typed Command Bar `select`).
    pub(crate) fn selected(&self) -> Option<Ref> {
        self.selection.get()
    }

    /// Every selected instance, anchor first — what the Explorer highlights
    /// and the viewport outlines.
    pub(super) fn selected_all(&self) -> &[Ref] {
        self.selection.all()
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
    /// tree redraws, so it must stay quiet when nothing actually changed —
    /// including a redraw that leaves the anchor exactly where it was (a
    /// Command Bar script's own rebuild re-asserting the same row, say),
    /// which must not collapse a `Shift`/`Ctrl`/`Cmd`-click multi-selection
    /// down to just that one instance: nothing about the tree changing back
    /// to what it already showed means the user picked something else.
    fn sync_selection(&mut self, tree: &Entity<TreeState>, cx: &mut Context<Self>) {
        let selected = Selection::of_item(tree.read(cx).selected_item());
        if selected == self.selection.get() {
            return;
        }
        if self.selection.set(selected) {
            self.selection_changed(cx);
        }
    }

    /// Everything that has to stay in step with `self.selection` after it
    /// changes, whichever of `sync_selection`/`Shell::select`/
    /// `Shell::deselect`/`Shell::extend_selection` changed it: any open
    /// Properties editor belonged to the old selection, and the viewport's
    /// outline and draggers have to move to the new one.
    fn selection_changed(&mut self, cx: &mut Context<Self>) {
        self.edits.clear();
        self.sync_viewport_selection(cx);
        // A click on the very part the cursor was already hovering would
        // otherwise leave its hover box drawn right under the new selection
        // outline until the cursor happens to move again — `hover_in_viewport`
        // suppresses a hover on an already-selected referent, but only
        // resolves on the next mouse move, so the same suppression has to
        // apply here too, immediately.
        let stale_hover = self
            .hovered
            .iter()
            .any(|entry| entry.parts().iter().all(|part| self.covered.contains(part)));
        if stale_hover {
            self.viewport
                .update(cx, |viewport, _| viewport.set_hover(Vec::new()));
            self.hovered.clear();
        }
        // Whatever just stopped being selected becomes one of the neighbours
        // a drag can settle against, and whatever just started stops being
        // one.
        self.sync_snap_neighbours(cx);
        cx.notify();
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

    /// Suppresses or restores motion, and remembers the choice.
    ///
    /// An explicit answer replaces the desktop's, which is the point: the
    /// OS setting is a sensible default, not a verdict, and somebody who
    /// wants this editor calm on a machine that animates everything else
    /// needs somewhere to say so.
    pub(crate) fn toggle_reduce_motion(&mut self, cx: &mut Context<Self>) {
        let reduced = !tokens::reduced_motion();
        self.reduce_motion = Some(reduced);
        tokens::set_reduced_motion(reduced);
        cx.set_reduce_motion(reduced);
        self.save_settings();
        cx.notify();
    }

    /// Raises every pointer target from WCAG 2.5.8's 24px floor to 2.5.5's
    /// 44px one, or lowers it back.
    pub(crate) fn toggle_large_targets(&mut self, cx: &mut Context<Self>) {
        tokens::set_large_targets(!tokens::large_targets());
        self.save_settings();
        cx.notify();
    }

    /// Puts the docks back where they started.
    ///
    /// The companion every persisted layout needs: a dock dragged to a few
    /// pixels wide is saved that way, and without this the only way back is
    /// to find and delete the settings file.
    pub(crate) fn reset_layout(&mut self, cx: &mut Context<Self>) {
        self.properties_width = workspace::properties_width();
        self.explorer_width = workspace::explorer_width();
        self.output_height = workspace::OUTPUT_HEIGHT;
        self.output_collapsed = false;
        self.save_settings();
        cx.notify();
    }

    /// Applies a new UI scale and persists it.
    ///
    /// Every size token is read through `tokens::font_scale`, so this one
    /// call re-lays-out the whole window — which is the point: WCAG 1.4.4
    /// asks for text at 200% *without loss of content or functionality*,
    /// and text that grew while its row didn't would lose exactly that.
    pub(super) fn set_font_scale(&mut self, scale: f32, cx: &mut Context<Self>) {
        if !tokens::set_font_scale(scale) {
            return;
        }
        // The docks are sized in state, not in tokens, so they have to be
        // re-derived or a 2x scale leaves a 300px dock holding 600px rows.
        self.properties_width = workspace::properties_width();
        self.explorer_width = workspace::explorer_width();
        self.save_settings();
        cx.notify();
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

    /// Whether the viewport's orientation indicator draws, for the dock's
    /// Viewport menu item to render its checked state — see
    /// `set_axis_indicator`.
    pub(super) fn axis_indicator(&self) -> bool {
        self.axis_indicator
    }

    /// Shows or hides the top-right orientation indicator — see
    /// `WorkspaceView::set_axis_indicator`.
    fn set_axis_indicator(&mut self, shown: bool, cx: &mut Context<Self>) {
        if shown == self.axis_indicator {
            return;
        }

        self.axis_indicator = shown;
        self.viewport
            .update(cx, |viewport, cx| viewport.set_axis_indicator(shown, cx));
        self.save_settings();
    }

    /// Whether a part in front of the selection hides its outline box, for
    /// the Viewport overflow menu item to render its checked state — see
    /// `set_selection_occluded`.
    pub(super) fn selection_occluded(&self) -> bool {
        self.selection_occluded
    }

    /// Switches the selection outline between drawing through everything
    /// (off, the default) and being depth-tested against the scene — see
    /// `WorkspaceView::set_selection_occluded`.
    fn set_selection_occluded(&mut self, occluded: bool, cx: &mut Context<Self>) {
        if occluded == self.selection_occluded {
            return;
        }

        self.selection_occluded = occluded;
        self.viewport.update(cx, |viewport, cx| {
            viewport.set_selection_occluded(occluded, cx)
        });
        self.save_settings();
    }

    /// Which icon pack the Explorer draws, for the dock's Explorer menu item
    /// (see `shell::dock`) to render its checked state.
    pub(super) fn icon_pack(&self) -> IconPack {
        self.icon_pack
    }

    /// Re-resolves every Explorer row's icon for `pack` in place — the tree's
    /// rows and their expansion state are untouched, only which sprite each
    /// one points at changes (see `Explorer::set_icon_pack`).
    fn set_icon_pack(&mut self, pack: IconPack, cx: &mut Context<Self>) {
        if pack == self.icon_pack {
            return;
        }

        self.icon_pack = pack;
        self.explorer = Rc::new(
            self.explorer
                .set_icon_pack(pack, &self.folder_colors, &self.path),
        );
        cx.notify();
        self.save_settings();
    }

    /// Whether the viewport's corner label shows its frame-rate readout, for
    /// the dock's Viewport menu item (see `shell::dock`) to render its
    /// checked state.
    pub(super) fn stats_shown(&self) -> bool {
        self.stats_shown
    }

    /// Flips the viewport corner label's Stats readout on or off — see
    /// `WorkspaceView::set_stats_shown`.
    fn set_stats_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        if shown == self.stats_shown {
            return;
        }

        self.stats_shown = shown;
        self.viewport
            .update(cx, |viewport, cx| viewport.set_stats_shown(shown, cx));
    }

    /// The frame rate preset the render loop caps itself to while the window
    /// is unfocused, for the dock's Viewport menu item (see `shell::dock`)
    /// to render its checked state.
    pub(super) fn unfocused_fps(&self) -> UnfocusedFps {
        self.unfocused_fps
    }

    /// Switches the unfocused frame rate preset — see
    /// `WorkspaceView::set_unfocused_fps`.
    fn set_unfocused_fps(&mut self, unfocused_fps: UnfocusedFps, cx: &mut Context<Self>) {
        if unfocused_fps == self.unfocused_fps {
            return;
        }

        self.unfocused_fps = unfocused_fps;
        self.viewport
            .update(cx, |viewport, _| viewport.set_unfocused_fps(unfocused_fps));
        self.save_settings();
    }

    /// Writes the current quality pick, Explorer visibility, projection mode,
    /// orientation indicator toggle, icon pack, and unfocused frame rate
    /// preset to disk. Also saves the current dock layout. A settings file
    /// is tiny, so this runs synchronously on every change rather than
    /// debouncing; a write failure (e.g. no writable config directory) is
    /// not fatal and is silently dropped — losing a preference write is
    /// better than interrupting the editor over it.
    fn save_settings(&self) {
        let settings = Settings {
            quality: self.quality_choice,
            show_all_services: self.show_all_services,
            orthographic: self.orthographic,
            axis_indicator: self.axis_indicator,
            selection_occluded: self.selection_occluded,
            icon_pack: self.icon_pack,
            unfocused_fps: self.unfocused_fps,
            font_scale: tokens::font_scale(),
            large_targets: tokens::large_targets(),
            reduce_motion: self.reduce_motion,
            properties_width: self.properties_width,
            explorer_width: self.explorer_width,
            output_height: self.output_height,
            output_collapsed: self.output_collapsed,
        };
        let _ = settings.save();

        // Save the current dock layout state
    }

    /// The place file's name, shown as the dock's own Viewport tab title
    /// (see `shell::dock`) now that the viewport no longer draws a fake one.
    /// The graphics-quality dropdown, relocated from the viewport's old fake
    /// tab bar into the dock's real Viewport title bar via `Panel::title_suffix`
    /// (see `shell::dock`) — the natural surviving home for a per-view control
    /// once that hand-rolled strip is gone.
    /// The graphics-quality dropdown, in the same box every other field in
    /// the editor wears (`rows::field_box`) rather than in the toolkit's
    /// own chrome — it is a select like any other, and looked like a
    /// visitor from a different application floating over the viewport.
    pub(super) fn quality_control(&self) -> impl IntoElement {
        rows::select_box().w(quality_width()).child(
            Select::new(&self.quality)
                .appearance(false)
                .with_size(tokens::field_size())
                .h_full()
                .py_0()
                .pt(tokens::select_inset())
                .menu_width(quality_width())
                .accessibility_label("Graphics quality"),
        )
    }

    /// The 3D view itself. No border: Row D's panels are told apart from it
    /// by their own elevation and surface, not by a line (see
    /// `UX_GUIDELINES.md` §3).
    pub(super) fn viewport(&self) -> impl IntoElement {
        div().size_full().child(self.viewport.clone())
    }
}

impl Shell {
    /// Tab and Shift+Tab, walking this window's own order — see
    /// `roving::TabOrder::step` for why GPUI's cannot be used.
    fn step_focus(&self, backwards: bool, window: &mut Window, cx: &mut App) {
        self.tab_order.step(backwards, window, cx);
    }
}

impl Render for Shell {
    /// The shell, top to bottom: the title bar, the menu strip, document
    /// tabs (Row A), the
    /// ribbon's category tabs (Row B), the ribbon itself (Row C), the
    /// three-column workspace (Row D), and the Command Bar.
    ///
    /// The resize drags are handled here rather than on the handles
    /// themselves: a pointer moving faster than the frame rate leaves the
    /// 4px handle between two frames, and a listener that only fires while
    /// the pointer is still over the handle would drop the drag. This
    /// container spans the window, so it can't be outrun.
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Deliberately no seeded focus. Seeding it *programmatically* is
        // what left a cold start with focus somewhere and no ring anywhere:
        // `focus_visible` only paints for keyboard-driven focus, so a
        // seeded one is invisible by construction and the first Tab becomes
        // a guess. With every stop numbered in reading order, the first Tab
        // is already predictable on its own — which is all the APG's
        // entry-point rule actually asks for.
        self.tab_order.restart();
        v_flex()
            .size_full()
            .bg(tokens::black())
            .font_family(tokens::FONT_FAMILY_UI)
            .text_color(tokens::text_strong())
            // Ctrl+S here rather than on one panel (contrast `instance_tree`'s
            // own `on_key_down`): a key event bubbles up from whatever holds
            // focus, and every panel — Explorer, Properties, viewport,
            // Command Bar — sits below this container, so a save works no
            // matter which one is focused.
            // Tab and Shift+Tab, in the *capture* phase, before anything
            // else can eat them.
            //
            // The toolkit binds them on its own `Root` — but a focused text
            // input consumes Tab first, so focus starting in the Command
            // Bar (which is where it starts) could never leave it with the
            // keyboard. That is a keyboard trap (WCAG 2.1.2), not a
            // cosmetic problem: every region below is unreachable without a
            // mouse until this runs. None of this app's inputs is
            // multi-line, so Tab has nothing else it could usefully mean.
            .key_context(roving::CONTEXT)
            .on_action(cx.listener(|shell, _: &roving::FocusNext, window, cx| {
                shell.step_focus(false, window, cx);
                cx.notify();
            }))
            .on_action(cx.listener(|shell, _: &roving::FocusPrev, window, cx| {
                shell.step_focus(true, window, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                shell.handle_shell_key(&event.keystroke, window, cx);
            }))
            .on_mouse_move(cx.listener(|shell, event: &MouseMoveEvent, window, cx| {
                shell.drag_resize(event.position, cx);
                shell.drag_scrub(event.position.x, event.modifiers, window, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|shell, _: &MouseUpEvent, _, cx| {
                    shell.end_resize(cx);
                    shell.scrub = None;
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|shell, _: &MouseUpEvent, _, cx| {
                    shell.end_resize(cx);
                    shell.scrub = None;
                }),
            )
            .child(self.topbar(cx))
            .child(crate::menu_bar::bar(&self.menu_bar))
            .child(self.document_tabs(cx))
            .child(self.ribbon_tabs(cx))
            .child(self.ribbon(cx))
            .child(self.workspace(window, cx))
            .child(self.command_bar.render(self.tab_order.next(), cx))
    }
}
