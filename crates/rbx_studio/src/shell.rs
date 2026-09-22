//! Window layout: a Studio-style `DockArea` (see `shell::dock`) whose default
//! arrangement is the tabbed 3D view over the Output log on the left, the
//! Explorer and Properties stacked on the right — but any of the four can be
//! dragged to another edge or stacked as tabs.

mod align;
mod attributes_panel;
mod brick_color;
mod change_class;
mod chrome;
mod clipboard;
mod command;
mod dock_drag;
mod docks;
mod drag;
mod edit;
mod explorer_edit;
mod folder_color;
mod group;
mod guides;
mod history;
mod keys;
mod layout;
mod light_guides;
mod menu;
mod output;
mod panel_window;
mod panels;
mod quality;
mod reparent;
mod ribbon;
mod roving;

pub(crate) use chrome::panel_topbar;
pub(crate) use layout::{edge_from_key, edge_key, Edge, Panel, SavedEdge, SavedGroup, SavedLayout};
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
mod sun;
mod toolbar;
mod tooltip;
mod viewport_dock;
mod workspace;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::select::{SearchableVec, SelectEvent, SelectState};
use gpui_kit::component::tree::TreeState;
use gpui_kit::component::{v_flex, IndexPath};
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;
use rbx_viewer::pick::Selected;
use rbx_viewer::QualityLevel;

use crate::align::Options as AlignOptions;
use crate::class_icons::IconPack;
use crate::cli::Launch;
use crate::command_bar::CommandBar;
use crate::explorer::Explorer;
use crate::folder_colors::FolderColors;
use crate::history::{History, DEFAULT_CAP};
use crate::menu_bar::MenuBar;
use crate::pacing::UnfocusedFps;
use crate::properties::{self, Properties};
use crate::save::Format;
use crate::script_editor::ScriptEditor;
use crate::settings::{DraggerSettings, Settings};
use crate::tokens;
use crate::transform::{Targets, Transform};
use crate::workspace_view::{AssetWarnings, Opened, PoseSynced, ViewportAction, WorkspaceView};
use crate::Place;
use chrome::{Document, Drag};
use menu::MenuId;
use quality::{quality_labels, quality_row};
use selection::{outlined, Selection};
use toolbar::snap::SnapFields;

/// The graphics quality dropdown's list: plain labels, since the mode a label
/// stands for is read back out of the label itself.
type QualityOptions = SearchableVec<SharedString>;

pub(crate) struct Shell {
    /// The top menu bar (File/Edit/Model/View); see `crate::menu_bar`. Kept
    /// as a field so `Render for Shell` has an entity to mount, and so the
    /// window-level F10/Alt handlers below have something to reach into —
    /// which title is current and which menu is open lives entirely inside it.
    menu_bar: Entity<MenuBar>,
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
    /// Whether a selected light draws its guide — Studio's `Show Light
    /// Guides`. Persisted the same way `orthographic` is (see `settings`).
    light_guides: bool,
    /// The guide segments last sent to the viewport, so an edit that leaves
    /// them as they were sends nothing — see `shell::light_guides`.
    light_guides_sent: Vec<rbx_viewer::Segment>,
    /// Which of the class icon kit's two variants the Explorer draws.
    /// Persisted (see `settings`); every write goes through
    /// [`Shell::save_settings`].
    icon_pack: IconPack,
    /// Which installed icon pack (see `crate::packs`) is drawn over the kit,
    /// and every pack found at startup for the Explorer menu to list —
    /// listed once because the menu is rebuilt every frame.
    appearance: crate::packs::Appearance,
    installed_icon_packs: Vec<String>,
    /// The render loop's frame rate cap while the window is unfocused (see
    /// `pacing::FocusPacing`). Persisted (see `settings`); every write goes
    /// through [`Shell::save_settings`].
    unfocused_fps: UnfocusedFps,
    /// The dragger guides' switches (see `shell::guides`). Persisted.
    dragger: DraggerSettings,
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
    /// The Viewport dock's settings, one Tab stop for the lot — see
    /// `shell::viewport_dock`.
    viewport_nav: roving::Roving,
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
    /// The open `NumberSequence`/`ColorSequence` graph, if any: a window of
    /// its own (see `crate::sequence_window`, which owns everything about
    /// it), kept only so opening a second one replaces the first.
    sequence: Option<WindowHandle<gpui_kit::component::Root>>,
    /// The Explorer's type-ahead buffer — see `shell::tree_keys`.
    typeahead: tree_keys::Typeahead,
    /// The Explorer's own editing affordances — the `+` picker, the
    /// right-click menu and the in-place name box. See
    /// `shell::explorer_edit`.
    explorer_edit: explorer_edit::ExplorerEdit,
    /// Whether a new instance whose name a sibling already carries is
    /// numbered, and whether inserting or selecting expands the tree to
    /// reveal the instance. Real Studio's two insertion preferences, both
    /// persisted — see `shell::explorer_edit::picker`.
    increment_names: bool,
    expand_on_select: bool,
    search: Entity<InputState>,
    filter: Entity<InputState>,
    properties: Properties,
    /// One open `Input` per Properties row currently being edited, keyed by
    /// property name; see `shell::edit`.
    edits: edit::Edits,
    /// The Attributes/Tags section's own rename box and add-attribute/add-tag
    /// fields; see `shell::attributes_panel`.
    attribute_edits: attributes_panel::AttributeEdits,
    /// Mirrors the tree's selected row (see [`Shell::sync_selection`]).
    selection: Selection,
    /// This window's own copy/paste clipboard, replaced whole by every
    /// `Ctrl+C` — see `shell::clipboard`.
    clipboard: Vec<clipboard::Clipped>,
    /// The user's starter scripts, read once at startup — see
    /// `crate::script_templates`.
    script_templates: crate::script_templates::ScriptTemplates,
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
    /// The Viewport dock's own, for when it is docked somewhere too short
    /// for its settings — see `shell::viewport_dock`.
    viewport_scroll: ScrollHandle,
    /// Where each of the dock's settings was laid out last frame, so a
    /// keyboard move can scroll the one it lands on into view.
    viewport_rows: Rc<std::cell::RefCell<Vec<Bounds<Pixels>>>>,
    /// The Output tab's free-text search box. Session-only and unpersisted,
    /// like `output_filter` beside it — a log you are still reading is not a
    /// setting.
    output_search: Entity<InputState>,
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
    /// Whether the Align popover is open, and so whether its live preview
    /// is being drawn — see `shell::align::Shell::refresh_align_preview`.
    align_open: bool,
    /// The Sun tool's body and gesture, and the drag under way — see
    /// `crate::sun`. Whether the tool is active at all is `transform.tool`.
    sun: crate::sun::SunTool,
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
    /// Which panel is on which edge and how big each edge is — the data
    /// that used to be the order of three `.child()` calls (see
    /// `shell::layout`). Persisted, along with the drag in progress if
    /// there is one.
    layout: layout::Layout,
    output_collapsed: bool,
    drag: Option<Drag>,
    /// The dock currently being dragged by its tab, which is what puts the
    /// drop strips on screen (see `shell::dock_drag`). `None` the rest of
    /// the time, which is nearly always.
    dragging_panel: Option<layout::Panel>,
    /// One window per torn-out dock, kept in step with the layout by
    /// `shell::panel_window`.
    panel_windows: HashMap<layout::Panel, WindowHandle<gpui_kit::component::Root>>,
    /// Whether the main window held focus last frame, so a torn-out dock
    /// is raised with it once rather than fought over every frame.
    window_was_active: bool,
    /// Kept only to stay subscribed: dropping these unregisters the listeners.
    _subscriptions: [Subscription; 12],
}

impl Shell {
    pub(crate) fn new(
        title: impl Into<SharedString>,
        place: Place,
        settings: Settings,
        launch: Launch,
        user: crate::packs::UserContent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let Settings {
            quality,
            show_all_services,
            orthographic,
            axis_indicator,
            selection_occluded,
            light_guides,
            icon_pack,
            unfocused_fps,
            font_scale,
            large_targets,
            reduce_motion,
            docks,
            output_collapsed,
            increment_names,
            expand_on_select,
            dragger,
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
            let mut view = WorkspaceView::new(
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
            );
            view.set_dragger(dragger);
            view
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
            light_guides,
            light_guides_sent: Vec::new(),
            icon_pack,
            appearance: user.appearance,
            installed_icon_packs: user.icon_packs,
            unfocused_fps,
            dragger,
            document_nav: roving::Roving::horizontal(),
            ribbon_tabs_nav: roving::Roving::horizontal(),
            ribbon_nav: roving::Roving::horizontal(),
            properties_nav: roving::Roving::vertical(),
            viewport_nav: roving::Roving::vertical(),
            tab_order: roving::TabOrder::default(),
            reduce_motion,
            scrub: None,
            sequence: None,
            tree_focus_handle,
            typeahead: tree_keys::Typeahead::default(),
            explorer_edit: explorer_edit::ExplorerEdit::default(),
            increment_names,
            expand_on_select,
            search: cx.new(|cx| InputState::new(window, cx).placeholder("Search")),
            filter,
            properties,
            edits: edit::Edits::default(),
            attribute_edits: attributes_panel::AttributeEdits::default(),
            selection: Selection::new(selected),
            clipboard: Vec::new(),
            script_templates: user.script_templates,
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
            viewport_scroll: ScrollHandle::new(),
            viewport_rows: Rc::default(),
            output_search: cx.new(|cx| InputState::new(window, cx).placeholder("Search")),
            path,
            format,
            folder_colors,
            transform,
            snap_fields,
            align: AlignOptions::default(),
            align_open: false,
            sun: crate::sun::SunTool::default(),
            ribbon_tab: ribbon::Tab::default(),
            document: Document::default(),
            open_menu: None,
            // A saved layout wins over the default, and is total over
            // whatever the file actually held (see `layout::Layout::restore`).
            layout: layout::Layout::restore(&docks),
            dragging_panel: None,
            panel_windows: HashMap::new(),
            window_was_active: true,
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
        shell.covered = initial_targets
            .iter()
            .map(|target| target.referent)
            .collect();
        shell
            .viewport
            .update(cx, |viewport, _| viewport.set_targets(initial_targets));
        shell.sync_snap_neighbours(cx);
        // Its light guides, for the same reason.
        shell.sync_light_guides(cx);

        // `RBX_STUDIO_TOOL` (see `shell::toolbar`). Before the Command Bar
        // block below rather than after it: a script's reload rebuilds the
        // renderer, so setting the tool first is what makes a screenshot
        // taken afterwards prove the rebuild *kept* it (see
        // `rbx_viewer::view::View`) instead of only proving it was set last.
        shell.apply_debug_tool(cx);

        // `--select` / `RBX_STUDIO_SELECT`: `main::load` already resolved a
        // single target into the initial `Place.selected` before the window
        // opened (too early for a comma list — it looks up one literal
        // target and finds nothing for one containing a comma), so this is
        // what actually applies a multi-instance selection — the only way to
        // put the outline/gizmo over more than one part, since nothing else
        // can send the viewport a `Shift`/`Ctrl`/`Cmd`-click on the editor's
        // behalf.
        //
        // What it could not resolve is held rather than reported here: a
        // target the `--run` block below is about to create is legitimately
        // missing at this point, so only that block's own second attempt
        // (or this one, when there is no script) says anything.
        let mut unresolved = Vec::new();
        if let Some(spec) = launch.select.as_deref() {
            unresolved = shell.apply_debug_select(spec, &launch, cx);
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

        // `--run` / `RBX_STUDIO_RUN`: runs exactly the pipeline Enter would,
        // once, before the first frame — the way a scripted launch changes a
        // place without synthetic input (see `AGENTS.md`'s safety rules).
        if let Some(source) = launch.run.as_deref() {
            shell.run_command(source, cx);
            launch.say(format!("--run: {}", shell.command_bar.feedback().label()));
            // Re-applied: too early above to name anything the script just
            // created. Trying it again here is what lets a screenshot show
            // that without a click nothing else can send.
            if let Some(spec) = launch.select.as_deref() {
                unresolved = shell.apply_debug_select(spec, &launch, cx);
            }
        }

        // Always on stderr, verbose or not: a `--select` that quietly does
        // nothing is exactly the failure a script driving this editor cannot
        // see for itself.
        for target in &unresolved {
            eprintln!("rbxstudio: --select: no instance matches {target:?}");
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

        // `RBX_STUDIO_MENU` (see `menu_bar::MenuBar::apply_debug_entry`):
        // the only way to put the keyboard in the menu bar without a
        // keystroke, and so the only way to screenshot it there.
        let menu_bar = shell.menu_bar.clone();
        menu_bar.update(cx, |bar, cx| bar.apply_debug_entry(window, cx));

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
        // Not a `reflect_changes`, so the Properties cache is told here —
        // only when a camera is among the selection, or a flight would
        // rebuild a large selection's rows five times a second for nothing.
        let camera_shown = self.selected_all().iter().any(|&reference| {
            self.dom
                .get(reference)
                .is_some_and(|instance| instance.class() == "Camera")
        });
        if camera_shown {
            self.properties.dom_changed(&[]);
        }
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
        self.attribute_edits.clear();
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
        self.layout = layout::Layout::default();
        self.output_collapsed = false;
        self.save_settings();
        cx.notify();
    }

    /// Docks one panel on one edge — from a drop, or from the dock menu's
    /// "Move to" entry.
    ///
    /// Both go through here rather than each transforming the layout
    /// themselves, so the two can never disagree about what a move means.
    /// The menu is not decoration: the accessibility guidance this project
    /// follows treats drag-only rearrangement as a failure, so the drag is
    /// the fast path and the menu is the one that has to exist.
    pub(super) fn land_panel(
        &mut self,
        panel: layout::Panel,
        landing: layout::Landing,
        cx: &mut Context<Self>,
    ) {
        self.dragging_panel = None;
        self.layout.apply(panel, landing);
        self.save_settings();
        cx.notify();
    }

    /// Shuts one panel, from its tab's own cross or its dock menu.
    pub(super) fn close_panel(&mut self, panel: layout::Panel, cx: &mut Context<Self>) {
        self.layout.close(panel);
        self.save_settings();
        cx.notify();
    }

    /// Opens (or brings forward) or shuts one, from the View menu or the
    /// ribbon's Home tab — the only two ways back, which is why both exist.
    pub(crate) fn set_panel_open(
        &mut self,
        panel: layout::Panel,
        open: bool,
        cx: &mut Context<Self>,
    ) {
        if open {
            self.layout.open(panel);
        } else {
            self.layout.close(panel);
        }
        self.save_settings();
        cx.notify();
    }

    /// Whether a panel is on screen — what a View menu toggle and a ribbon
    /// tile read, so one hidden behind another tab is brought forward by
    /// them rather than shut.
    pub(crate) fn is_panel_showing(&self, panel: layout::Panel) -> bool {
        self.layout.is_showing(panel)
    }

    /// Shows one of a dock's tabs, from a click on it.
    pub(super) fn activate_panel(&mut self, panel: layout::Panel, cx: &mut Context<Self>) {
        self.layout.activate(panel);
        self.save_settings();
        cx.notify();
    }

    /// Tears one panel out into a window of its own — from a tab dragged
    /// past the window's edge, or from the dock menu's "Float".
    pub(super) fn float_panel(&mut self, panel: layout::Panel, cx: &mut Context<Self>) {
        self.layout.float(panel);
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
        self.layout.reset_sizes();
        self.save_settings();
        cx.notify();
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

    /// The installed icon pack in use, if any, and every installed pack's
    /// name — for the Explorer menu (see `shell::workspace`).
    pub(super) fn installed_icon_packs(&self) -> (&[String], Option<&str>) {
        (
            &self.installed_icon_packs,
            self.appearance.icon_pack.as_deref(),
        )
    }

    /// Switches the user's icon pack over the kit (`None` for the kit alone)
    /// and re-resolves every Explorer row's icon. A pack that no longer
    /// loads — deleted since startup, say — is reported in the Output dock
    /// and changes nothing, rather than silently reverting to the kit.
    fn set_user_icon_pack(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        let overlay = match &name {
            Some(name) => match crate::packs::IconOverlay::load(name) {
                Some(overlay) => Some(overlay),
                None => {
                    self.output
                        .push_warning(&format!("icon pack {name:?} could not be loaded"));
                    cx.notify();
                    return;
                }
            },
            None => None,
        };
        crate::class_icons::set_user_pack(overlay);
        self.appearance.icon_pack = name;
        if let Err(err) = self.appearance.save_icon_pack() {
            self.output
                .push_warning(&format!("could not remember the icon pack: {err}"));
        }
        self.explorer = Rc::new(self.explorer.set_icon_pack(
            self.icon_pack,
            &self.folder_colors,
            &self.path,
        ));
        cx.notify();
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
            light_guides: self.light_guides,
            icon_pack: self.icon_pack,
            unfocused_fps: self.unfocused_fps,
            font_scale: tokens::font_scale(),
            large_targets: tokens::large_targets(),
            reduce_motion: self.reduce_motion,
            docks: self.layout.saved(),
            output_collapsed: self.output_collapsed,
            increment_names: self.increment_names,
            expand_on_select: self.expand_on_select,
            dragger: self.dragger,
        };
        let _ = settings.save();

        // Save the current dock layout state
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
        // Before the tree is built, so the box this focuses is in the very
        // frame that hands it the caret — see `Shell::focus_explorer_edit`.
        self.focus_explorer_edit(window, cx);
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
            // A bare Alt tap is one of the two ways into the menu bar, so
            // the window has to see Alt going down and coming back up. Both
            // of the listeners below only exist to tell a tap apart from Alt
            // being used as the live modifier it also is here — see
            // `menu_bar::alt_tap`.
            .on_modifiers_changed(cx.listener(
                |shell, event: &ModifiersChangedEvent, window, cx| {
                    let menu_bar = shell.menu_bar.clone();
                    menu_bar.update(cx, |bar, cx| {
                        bar.modifiers_changed(event.modifiers, window, cx)
                    });
                },
            ))
            .capture_any_mouse_down(cx.listener(|shell, _: &MouseDownEvent, _, cx| {
                shell.menu_bar.update(cx, |bar, _| bar.interrupt_alt_tap());
            }))
            .on_mouse_move(cx.listener(|shell, event: &MouseMoveEvent, window, cx| {
                shell.note_pointer(event.position);
                shell.drag_resize(event.position, cx);
                shell.drag_scrub(event.position.x, event.modifiers, window, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|shell, event: &MouseUpEvent, window, cx| {
                    shell.end_resize(cx);
                    shell.scrub = None;
                    shell.end_panel_drag(event.position, window.viewport_size(), cx);
                }),
            )
            // The one that matters for tearing a dock out: a pointer
            // released *outside* the window is by definition not over any
            // element of ours, so no drop is ever reported for it and this
            // is the only place the gesture can be finished.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|shell, event: &MouseUpEvent, window, cx| {
                    shell.end_resize(cx);
                    shell.scrub = None;
                    shell.end_panel_drag(event.position, window.viewport_size(), cx);
                }),
            )
            .child(self.topbar(cx))
            .child(crate::menu_bar::bar(&self.menu_bar))
            .child(self.document_tabs(cx))
            .child(self.ribbon_tabs(cx))
            .child(self.ribbon(cx))
            .child(self.workspace(window, cx))
            .child(
                self.command_bar
                    .render(self.tab_order.next(), self.output_collapsed, cx),
            )
            // Painted at the window's root rather than inside the Explorer:
            // a popup nested in the tree's own scrolled, virtualised list
            // is clipped by it.
            .children(self.explorer_popups(cx))
    }
}

// ---------------------------------------------------------- sequence graph

impl Shell {
    /// Opens the `NumberSequence`/`ColorSequence` graph for the row named
    /// `row`, in a window of its own (`crate::sequence_window`). Whatever
    /// graph was already open closes first: two windows editing the same
    /// property would be two answers to the same question.
    pub(super) fn open_sequence_editor(
        &mut self,
        row: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((color, text)) = self.sequence_row(row) else {
            return;
        };
        let Some(value) = properties::edit::sequence_value(color, &text) else {
            return;
        };
        let Some(editor) = crate::sequence_editor::Editor::open(&value) else {
            return;
        };
        let title = self.sequence_title(row);
        let previous = self.sequence.take();
        let shell = cx.entity();
        let row = row.to_owned();

        // Deferred, because this runs inside the click handler's own `Shell`
        // update: opening a window renders it, and the first render reads
        // the very entity that borrow is holding — which is a panic rather
        // than something the compiler would have caught. `App::defer` puts
        // both the close and the open after that update ends.
        cx.defer(move |cx| {
            if let Some(previous) = previous {
                // An error here only means the window is already gone, which
                // is exactly the state this is trying to reach.
                let _ = previous.update(cx, |_, window, _| window.remove_window());
            }
            let opened = crate::sequence_window::SequenceWindow::open(
                shell.clone(),
                row,
                title,
                text,
                editor,
                cx,
            );
            shell.update(cx, |shell, _| shell.sequence = opened);
        });
    }

    /// The row's `EditKind`, found the same way the Properties panel itself
    /// finds a row rather than kept in a second place that could go stale.
    /// `None` once the row is gone, which is how the graph window knows to
    /// close itself.
    pub(crate) fn sequence_row(&self, row: &str) -> Option<(bool, String)> {
        let reference = self.selected()?;
        let kind = if let Some(attribute) = properties::attributes::attribute_of_row(row) {
            let value = properties::attributes::attributes(&self.dom, reference)
                .get(attribute)?
                .clone();
            properties::attributes::edit_kind(&value)?
        } else {
            let folder_color = self.folder_color(reference);
            self.properties
                .rows(&self.dom, self.selected_all(), folder_color)
                .into_iter()
                .find(|candidate| candidate.name == row)
                .and_then(|candidate| candidate.edit)?
        };
        match kind {
            properties::EditKind::Sequence { color, text } => Some((color, text)),
            _ => None,
        }
    }

    /// `ParticleEmitter.Size` — the instance's class and the property, the
    /// way Studio titles the same window. An attribute row drops its
    /// `Attribute:` prefix and reads as the attribute's name.
    pub(crate) fn sequence_title(&self, row: &str) -> String {
        let name = properties::attributes::attribute_of_row(row).unwrap_or(row);
        let class = self
            .selected()
            .and_then(|reference| self.dom.get(reference))
            .map(|instance| instance.class().to_owned());
        match class {
            Some(class) => format!("{class}.{name}"),
            None => name.to_owned(),
        }
    }
}

/// One number as this editor's fields read it back: three decimals with
/// trailing zeros trimmed, the same reading `shell::scrub` gives a dragged
/// value, so a number that arrived by drag and one that was typed look
/// alike. `pub(crate)` for `crate::sequence_window`, whose footer is a row
/// of exactly those fields in a window of its own.
pub(crate) fn format_scrubbed(value: f32) -> String {
    scrub::format(value, crate::properties::FieldKind::Decimal)
}
