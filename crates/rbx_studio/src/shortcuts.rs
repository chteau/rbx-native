//! Every rebindable keyboard shortcut, in one table: real Studio's
//! File › Customize Shortcuts (the window is `shell::shortcuts_window`).
//!
//! Each key handler asks [`pressed`] instead of matching a key itself, so a
//! rebinding applies everywhere at once. Defaults live in [`ALL`]; the
//! user's changes are the only thing persisted, in
//! `<config>/shortcuts.json` as `{"overrides": {"<id>": ["ctrl-s", …]}}`
//! (an empty list unbinds). Keys are GPUI's keystroke spelling, where
//! `secondary` is Cmd on macOS and Ctrl elsewhere.

use std::collections::BTreeMap;
use std::path::Path;

use gpui_kit::{Action, App, Keystroke};

/// Where a command shows in the Customize Shortcuts window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Category {
    File,
    Edit,
    View,
    Explorer,
    Tools,
    Camera,
    Terrain,
    Script,
    Debug,
    Graph,
    UiEditor,
    Animation,
}

/// Where a command's key is listened for. Two commands only clash when
/// they share a scope, or one of them is [`Scope::Global`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Scope {
    Global,
    Explorer,
    Viewport,
    Script,
    Graph,
    UiEditor,
    Animation,
}

macro_rules! commands {
    ($($variant:ident, $id:literal, $name:literal, $category:ident, [$($key:literal),*];)*) => {
        /// One rebindable command.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub(crate) enum Command { $($variant,)* }

        /// Every command, in the order the window lists them.
        pub(crate) const ALL: &[Spec] = &[
            $(Spec {
                command: Command::$variant,
                id: $id,
                name: $name,
                category: Category::$category,
                defaults: &[$($key),*],
            },)*
        ];
    };
}

commands! {
    Save, "save", "Save to File", File, ["secondary-s"];
    StudioSettings, "studio_settings", "Studio Settings", File, ["alt-s"];
    Undo, "undo", "Undo", Edit, ["secondary-z"];
    Redo, "redo", "Redo", Edit, ["secondary-shift-z", "secondary-y"];
    Cut, "cut", "Cut", Edit, ["secondary-x"];
    Copy, "copy", "Copy", Edit, ["secondary-c"];
    Paste, "paste", "Paste", Edit, ["secondary-v"];
    PasteInto, "paste_into", "Paste Into", Edit, ["secondary-shift-v"];
    Duplicate, "duplicate", "Duplicate", Edit, ["secondary-d"];
    Group, "group", "Group", Edit, ["secondary-g"];
    Ungroup, "ungroup", "Ungroup", Edit, ["secondary-shift-g"];
    UiScaleIn, "ui_scale_in", "Zoom In (UI scale)", View, ["secondary-="];
    UiScaleOut, "ui_scale_out", "Zoom Out (UI scale)", View, ["secondary--"];
    UiScaleReset, "ui_scale_reset", "Reset UI Scale", View, ["secondary-0"];
    CommandPalette, "command_palette", "Command Palette", View, ["secondary-alt-p"];
    QuickOpen, "quick_open", "Quick Open", View, ["secondary-p"];
    MenuBar, "menu_bar", "Focus the Menu Bar", View, ["f10"];
    Delete, "delete", "Delete", Explorer, ["delete", "backspace"];
    Rename, "rename", "Rename", Explorer, ["f2"];
    InsertPart, "insert_part", "Insert Part", Explorer, ["secondary-shift-p"];
    InsertFolder, "insert_folder", "Insert Folder", Explorer, ["secondary-shift-f"];
    InsertObject, "insert_object", "Insert Object…", Explorer, ["secondary-i"];
    RowMenu, "row_menu", "Row Menu", Explorer, ["shift-f10", "menu"];
    ToolSelect, "tool_select", "Select Tool", Tools, ["1"];
    ToolMove, "tool_move", "Move Tool", Tools, ["2"];
    ToolScale, "tool_scale", "Scale Tool", Tools, ["3"];
    ToolRotate, "tool_rotate", "Rotate Tool", Tools, ["4"];
    ToolTransform, "tool_transform", "Transform Tool", Tools, ["5"];
    ToggleLocal, "toggle_local", "Toggle Local/World Space", Tools, ["secondary-l"];
    ToggleJoint, "toggle_joint", "Toggle Joint Posing", Tools, ["r"];
    FocusMoveIncrement, "focus_move_increment", "Edit Move Increment", Tools, ["shift-2"];
    FocusRotateIncrement, "focus_rotate_increment", "Edit Rotate Increment", Tools, ["alt-r"];
    TurnTilt, "turn_tilt", "Tilt Held Part", Tools, ["t"];
    TurnRotate, "turn_rotate", "Rotate Held Part", Tools, ["r"];
    SummonHandles, "summon_handles", "Summon Handles", Tools, ["tab"];
    CameraForward, "camera_forward", "Camera Forward", Camera, ["w", "up"];
    CameraBack, "camera_back", "Camera Back", Camera, ["s", "down"];
    CameraLeft, "camera_left", "Camera Left", Camera, ["a", "left"];
    CameraRight, "camera_right", "Camera Right", Camera, ["d", "right"];
    CameraUp, "camera_up", "Camera Up", Camera, ["e"];
    CameraDown, "camera_down", "Camera Down", Camera, ["q"];
    TerrainBrush, "terrain_brush", "Hold for Brush", Terrain, ["b"];
    TerrainDelete, "terrain_delete", "Delete Region", Terrain, ["delete", "backspace"];
    FindFunction, "find_function", "Go to Function", Script, ["alt-f"];
    FindAll, "find_all", "Find in All Scripts", Script, ["secondary-shift-f", "secondary-shift-h"];
    SelectNextMatch, "select_next_match", "Select Next Occurrence", Script, ["secondary-d"];
    SelectAllMatches, "select_all_matches", "Select All Occurrences", Script, ["shift-alt-l"];
    DebugContinue, "debug_continue", "Start / Continue Debugging", Debug, ["f5"];
    DebugStop, "debug_stop", "Stop Debugging", Debug, ["shift-f5"];
    StepOver, "step_over", "Step Over", Debug, ["f10"];
    StepInto, "step_into", "Step Into", Debug, ["f11"];
    StepOut, "step_out", "Step Out", Debug, ["shift-f11"];
    ToggleBreakpoint, "toggle_breakpoint", "Toggle Breakpoint", Debug, ["f9"];
    GraphAddNode, "graph_add_node", "Add Node", Graph, ["shift-a"];
    GraphSelectAll, "graph_select_all", "Select All Nodes", Graph, ["secondary-a"];
    GraphDuplicate, "graph_duplicate", "Duplicate Nodes", Graph, ["secondary-d"];
    GraphGroup, "graph_group", "Group Nodes", Graph, ["secondary-g"];
    GraphFrame, "graph_frame", "Frame Selection", Graph, ["f"];
    GraphDelete, "graph_delete", "Delete Nodes", Graph, ["delete", "backspace"];
    GraphPan, "graph_pan", "Hold to Pan", Graph, ["space"];
    GraphMenu, "graph_menu", "Graph Menu", Graph, ["shift-f10"];
    UiToolFrame, "ui_tool_frame", "Frame Tool", UiEditor, ["f"];
    UiToolTextLabel, "ui_tool_text_label", "TextLabel Tool", UiEditor, ["t"];
    UiToolTextButton, "ui_tool_text_button", "TextButton Tool", UiEditor, ["b"];
    UiToolTextBox, "ui_tool_text_box", "TextBox Tool", UiEditor, ["x"];
    UiToolImageLabel, "ui_tool_image_label", "ImageLabel Tool", UiEditor, ["l"];
    UiToolImageButton, "ui_tool_image_button", "ImageButton Tool", UiEditor, ["g"];
    UiDisarm, "ui_disarm", "Select (Disarm Tool)", UiEditor, ["v"];
    UiPan, "ui_pan", "Hold to Pan", UiEditor, ["space"];
    UiBringToFront, "ui_bring_to_front", "Bring to Front", UiEditor, ["shift-]"];
    UiBringForward, "ui_bring_forward", "Bring Forward", UiEditor, ["]"];
    UiSendToBack, "ui_send_to_back", "Send to Back", UiEditor, ["shift-["];
    UiSendBackward, "ui_send_backward", "Send Backward", UiEditor, ["["];
    UiZoomReset, "ui_zoom_reset", "Zoom to 100%", UiEditor, ["0"];
    UiZoomFit, "ui_zoom_fit", "Zoom to Fit", UiEditor, ["1"];
    UiDelete, "ui_delete", "Delete", UiEditor, ["delete", "backspace"];
    AnimPlay, "anim_play", "Play / Pause", Animation, ["space"];
    AnimInsertKey, "anim_insert_key", "Insert Keyframe", Animation, ["i"];
    AnimDeleteKeys, "anim_delete_keys", "Delete Keyframes", Animation, ["x", "delete", "backspace"];
    AnimPrevFrame, "anim_prev_frame", "Previous Frame", Animation, ["left"];
    AnimNextFrame, "anim_next_frame", "Next Frame", Animation, ["right"];
    AnimPrevKey, "anim_prev_key", "Previous Keyframe", Animation, ["up"];
    AnimNextKey, "anim_next_key", "Next Keyframe", Animation, ["down"];
    AnimStart, "anim_start", "Go to Start", Animation, ["home"];
    AnimEnd, "anim_end", "Go to End", Animation, ["end"];
    AnimSelectAll, "anim_select_all", "Select All Keyframes", Animation, ["a"];
    AnimSelectNone, "anim_select_none", "Select No Keyframes", Animation, ["alt-a"];
    AnimFit, "anim_fit", "Fit Timeline", Animation, ["f"];
    AnimCopy, "anim_copy", "Copy Keyframes", Animation, ["secondary-c"];
    AnimPaste, "anim_paste", "Paste Keyframes", Animation, ["secondary-v"];
}

/// One row of [`ALL`].
#[derive(Debug)]
pub(crate) struct Spec {
    pub(crate) command: Command,
    /// The stable name `shortcuts.json` uses.
    pub(crate) id: &'static str,
    pub(crate) name: &'static str,
    pub(crate) category: Category,
    /// GPUI keystroke spellings; more than one means any of them works.
    pub(crate) defaults: &'static [&'static str],
}

impl Category {
    /// The heading the window shows.
    pub(crate) fn label(self) -> &'static str {
        todo!()
    }

    pub(crate) fn scope(self) -> Scope {
        todo!()
    }
}

impl Command {
    pub(crate) fn spec(self) -> &'static Spec {
        todo!()
    }

    pub(crate) fn from_id(id: &str) -> Option<Command> {
        todo!()
    }
}

/// The user's changes over the defaults. Pure: the global copy is behind
/// [`pressed`] and friends.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Store {
    /// Command → its keys, replacing the defaults; empty = unbound.
    overrides: BTreeMap<Command, Vec<Keystroke>>,
}

impl Store {
    /// Parses `shortcuts.json`; unknown ids and unparsable keys are dropped,
    /// anything malformed is an empty store.
    pub(crate) fn from_json(text: &str) -> Store {
        todo!()
    }

    pub(crate) fn to_json(&self) -> String {
        todo!()
    }

    /// A missing or unreadable file is an empty store.
    pub(crate) fn load(path: &Path) -> Store {
        todo!()
    }

    /// Temp file then rename, creating the directory.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        todo!()
    }

    /// The keys that run `command` now: the override, else the defaults.
    pub(crate) fn bindings(&self, command: Command) -> Vec<Keystroke> {
        todo!()
    }

    pub(crate) fn is_default(&self, command: Command) -> bool {
        todo!()
    }

    /// Replaces `command`'s keys. Setting exactly the defaults clears the
    /// override.
    pub(crate) fn set(&mut self, command: Command, keys: Vec<Keystroke>) {
        todo!()
    }

    pub(crate) fn reset(&mut self, command: Command) {
        todo!()
    }

    pub(crate) fn reset_all(&mut self) {
        todo!()
    }

    /// Whether `keystroke` is one of `command`'s keys (see [`same_key`]).
    pub(crate) fn pressed(&self, command: Command, keystroke: &Keystroke) -> bool {
        todo!()
    }

    /// Other commands `keystroke` already runs in a scope that clashes with
    /// `command`'s (see [`Scope`]).
    pub(crate) fn conflicts(&self, command: Command, keystroke: &Keystroke) -> Vec<Command> {
        todo!()
    }
}

/// Parses one GPUI spelling (`secondary-shift-z`); `None` if invalid.
pub(crate) fn parse(spelling: &str) -> Option<Keystroke> {
    todo!()
}

/// Whether two keystrokes are the same key: key names compared ignoring
/// ASCII case; control/alt/shift/platform/function all equal. A shifted US
/// symbol (`}` `{` `+` `@` `_` …) counts as its base key with shift held,
/// on either side.
pub(crate) fn same_key(a: &Keystroke, b: &Keystroke) -> bool {
    todo!()
}

/// "Ctrl+Shift+G" (on macOS "⌘⇧G"), key names title-cased ("F10",
/// "Delete", "Space", "Up").
pub(crate) fn display(keystroke: &Keystroke) -> String {
    todo!()
}

/// Whether a key press may become a binding: not a bare modifier, not
/// escape/enter/tab with no modifier.
pub(crate) fn bindable(keystroke: &Keystroke) -> bool {
    todo!()
}

// The app's one copy, loaded from `<config>/shortcuts.json` on first use.

/// Whether `keystroke` runs `command` right now. Called by every handler.
pub(crate) fn pressed(command: Command, keystroke: &Keystroke) -> bool {
    todo!()
}

pub(crate) fn bindings(command: Command) -> Vec<Keystroke> {
    todo!()
}

pub(crate) fn is_default(command: Command) -> bool {
    todo!()
}

/// The first binding, displayed; `None` if unbound.
pub(crate) fn hint(command: Command) -> Option<String> {
    todo!()
}

/// A snapshot of the global store, for the window.
pub(crate) fn store() -> Store {
    todo!()
}

/// Replaces the global store, saves it, and re-applies [`apply_bindings`].
pub(crate) fn replace(store: Store, cx: &mut App) {
    todo!()
}

/// The command a menu action stands for, for menu and palette hints.
pub(crate) fn command_for_action(action: &dyn Action) -> Option<Command> {
    todo!()
}

/// Puts the keymap-backed commands (Studio Settings, Command Palette, Quick
/// Open, Summon Handles) in GPUI's keymap with their current keys,
/// replacing whatever bindings those actions had. Called once at startup
/// and after every change.
pub(crate) fn apply_bindings(cx: &mut App) {
    todo!()
}

#[cfg(test)]
mod tests;
