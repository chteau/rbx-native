//! Every rebindable keyboard shortcut, in one table: real Studio's
//! File › Customize Shortcuts (the window is `shell::shortcuts_window`).
//!
//! Each key handler asks [`pressed`] instead of matching a key itself, so a
//! rebinding applies everywhere at once. Defaults live in [`ALL`]; the
//! user's changes are the only thing persisted, in
//! `<config>/shortcuts.json` as `{"overrides": {"<id>": ["ctrl-s", …]}}`
//! (an empty list unbinds). Keys are GPUI's keystroke spelling, where
//! `secondary` is Cmd on macOS and Ctrl elsewhere.

use std::any::TypeId;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpui_kit::{Action, App, KeyBinding, Keystroke};

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
        #[allow(clippy::enum_variant_names)]
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
    NewPlace, "new_place", "New", File, ["secondary-n"];
    OpenPlace, "open_place", "Open…", File, ["secondary-o"];
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
        match self {
            Category::File => "File",
            Category::Edit => "Edit",
            Category::View => "View",
            Category::Explorer => "Explorer",
            Category::Tools => "Tools",
            Category::Camera => "Camera",
            Category::Terrain => "Terrain",
            Category::Script => "Script Editor",
            Category::Debug => "Debugger",
            Category::Graph => "Script Graph",
            Category::UiEditor => "UI Editor",
            Category::Animation => "Animation Editor",
        }
    }

    pub(crate) fn scope(self) -> Scope {
        match self {
            Category::File | Category::Edit | Category::View => Scope::Global,
            Category::Explorer => Scope::Explorer,
            Category::Tools | Category::Camera | Category::Terrain => Scope::Viewport,
            Category::Script | Category::Debug => Scope::Script,
            Category::Graph => Scope::Graph,
            Category::UiEditor => Scope::UiEditor,
            Category::Animation => Scope::Animation,
        }
    }
}

impl Command {
    pub(crate) fn spec(self) -> &'static Spec {
        ALL.iter()
            .find(|spec| spec.command == self)
            .expect("every command is listed in ALL")
    }

    pub(crate) fn from_id(id: &str) -> Option<Command> {
        ALL.iter()
            .find(|spec| spec.id == id)
            .map(|spec| spec.command)
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
        let mut store = Store::default();
        let Ok(serde_json::Value::Object(root)) = serde_json::from_str(text) else {
            return store;
        };
        let Some(serde_json::Value::Object(overrides)) = root.get("overrides") else {
            return store;
        };
        for (id, keys) in overrides {
            let (Some(command), Some(keys)) = (Command::from_id(id), keys.as_array()) else {
                continue;
            };
            let keys = keys
                .iter()
                .filter_map(|key| key.as_str().and_then(parse))
                .collect();
            store.set(command, keys);
        }
        store
    }

    pub(crate) fn to_json(&self) -> String {
        let mut overrides = serde_json::Map::new();
        for (command, keys) in &self.overrides {
            let keys = keys
                .iter()
                .map(|key| serde_json::Value::String(key.unparse()))
                .collect();
            overrides.insert(
                command.spec().id.to_string(),
                serde_json::Value::Array(keys),
            );
        }
        let mut root = serde_json::Map::new();
        root.insert("overrides".into(), serde_json::Value::Object(overrides));
        serde_json::to_string_pretty(&serde_json::Value::Object(root)).unwrap_or_default()
    }

    /// A missing or unreadable file is an empty store.
    pub(crate) fn load(path: &Path) -> Store {
        std::fs::read_to_string(path)
            .map(|text| Store::from_json(&text))
            .unwrap_or_default()
    }

    /// Temp file then rename, creating the directory.
    pub(crate) fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let temp = path.with_extension("json.tmp");
        std::fs::write(&temp, self.to_json())?;
        std::fs::rename(&temp, path)
    }

    /// The keys that run `command` now: the override, else the defaults.
    pub(crate) fn bindings(&self, command: Command) -> Vec<Keystroke> {
        match self.overrides.get(&command) {
            Some(keys) => keys.clone(),
            None => default_keys(command),
        }
    }

    pub(crate) fn is_default(&self, command: Command) -> bool {
        !self.overrides.contains_key(&command)
    }

    /// Replaces `command`'s keys. Setting exactly the defaults clears the
    /// override.
    pub(crate) fn set(&mut self, command: Command, keys: Vec<Keystroke>) {
        if keys == default_keys(command) {
            self.overrides.remove(&command);
        } else {
            self.overrides.insert(command, keys);
        }
    }

    pub(crate) fn reset(&mut self, command: Command) {
        self.overrides.remove(&command);
    }

    pub(crate) fn reset_all(&mut self) {
        self.overrides.clear();
    }

    /// Whether `keystroke` is one of `command`'s keys (see [`same_key`]).
    pub(crate) fn pressed(&self, command: Command, keystroke: &Keystroke) -> bool {
        self.bindings(command)
            .iter()
            .any(|key| same_key(key, keystroke))
    }

    /// Other commands `keystroke` already runs in a scope that clashes with
    /// `command`'s (see [`Scope`]).
    pub(crate) fn conflicts(&self, command: Command, keystroke: &Keystroke) -> Vec<Command> {
        let scope = command.spec().category.scope();
        ALL.iter()
            .filter(|spec| spec.command != command)
            .filter(|spec| {
                let other = spec.category.scope();
                other == scope || other == Scope::Global || scope == Scope::Global
            })
            .filter(|spec| self.pressed(spec.command, keystroke))
            .map(|spec| spec.command)
            .collect()
    }
}

fn default_keys(command: Command) -> Vec<Keystroke> {
    command
        .spec()
        .defaults
        .iter()
        .filter_map(|spelling| parse(spelling))
        .collect()
}

/// Parses one GPUI spelling (`secondary-shift-z`); `None` if invalid.
pub(crate) fn parse(spelling: &str) -> Option<Keystroke> {
    Keystroke::parse(spelling).ok()
}

/// The unshifted key a US-layout shifted symbol is typed with.
fn unshifted(key: &str) -> Option<&'static str> {
    Some(match key {
        "!" => "1",
        "@" => "2",
        "#" => "3",
        "$" => "4",
        "%" => "5",
        "^" => "6",
        "&" => "7",
        "*" => "8",
        "(" => "9",
        ")" => "0",
        "_" => "-",
        "+" => "=",
        "{" => "[",
        "}" => "]",
        "|" => "\\",
        ":" => ";",
        "\"" => "'",
        "<" => ",",
        ">" => ".",
        "?" => "/",
        "~" => "`",
        _ => return None,
    })
}

/// The key name lower-cased, and whether shift is held, with a shifted
/// symbol folded onto its base key.
fn normalized(keystroke: &Keystroke) -> (String, bool) {
    match unshifted(&keystroke.key) {
        Some(base) => (base.to_string(), true),
        None => (
            keystroke.key.to_ascii_lowercase(),
            keystroke.modifiers.shift,
        ),
    }
}

/// Whether two keystrokes are the same key: key names compared ignoring
/// ASCII case; control/alt/shift/platform/function all equal. A shifted US
/// symbol (`}` `{` `+` `@` `_` …) counts as its base key with shift held,
/// on either side.
pub(crate) fn same_key(a: &Keystroke, b: &Keystroke) -> bool {
    let (key_a, shift_a) = normalized(a);
    let (key_b, shift_b) = normalized(b);
    key_a == key_b
        && shift_a == shift_b
        && a.modifiers.control == b.modifiers.control
        && a.modifiers.alt == b.modifiers.alt
        && a.modifiers.platform == b.modifiers.platform
        && a.modifiers.function == b.modifiers.function
}

/// "Ctrl+Shift+G" (on macOS "⌘⇧G"), key names title-cased ("F10",
/// "Delete", "Space", "Up").
pub(crate) fn display(keystroke: &Keystroke) -> String {
    let modifiers = &keystroke.modifiers;
    let key = key_title(&keystroke.key);
    if cfg!(target_os = "macos") {
        let mut text = String::new();
        for (held, glyph) in [
            (modifiers.function, "Fn"),
            (modifiers.control, "⌃"),
            (modifiers.alt, "⌥"),
            (modifiers.shift, "⇧"),
            (modifiers.platform, "⌘"),
        ] {
            if held {
                text.push_str(glyph);
            }
        }
        text.push_str(&key);
        return text;
    }
    let mut parts: Vec<&str> = Vec::new();
    for (held, name) in [
        (modifiers.control, "Ctrl"),
        (modifiers.alt, "Alt"),
        (modifiers.shift, "Shift"),
        (modifiers.platform, "Super"),
        (modifiers.function, "Fn"),
    ] {
        if held {
            parts.push(name);
        }
    }
    parts.push(&key);
    parts.join("+")
}

fn key_title(key: &str) -> String {
    match key {
        "pageup" => return "Page Up".into(),
        "pagedown" => return "Page Down".into(),
        _ => {}
    }
    let mut chars = key.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Whether a key press may become a binding: not a bare modifier, not
/// escape/enter/tab with no modifier.
pub(crate) fn bindable(keystroke: &Keystroke) -> bool {
    if matches!(
        keystroke.key.as_str(),
        "shift" | "control" | "ctrl" | "alt" | "platform" | "cmd" | "super" | "function" | "fn"
    ) {
        return false;
    }
    let m = &keystroke.modifiers;
    let bare = !(m.control || m.alt || m.shift || m.platform || m.function);
    !(bare && matches!(keystroke.key.as_str(), "escape" | "enter" | "tab"))
}

// The app's one copy, loaded from `<config>/shortcuts.json` on first use.

thread_local! {
    static STORE: RefCell<Option<Store>> = const { RefCell::new(None) };
    static REGISTERED: RefCell<Vec<KeymapEntry>> = const { RefCell::new(Vec::new()) };
}

fn config_path() -> Option<PathBuf> {
    crate::settings::default_config_dir().map(|dir| dir.join("shortcuts.json"))
}

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
    STORE.with(|cell| {
        let mut slot = cell.borrow_mut();
        let store = slot.get_or_insert_with(|| {
            config_path()
                .map(|path| Store::load(&path))
                .unwrap_or_default()
        });
        f(store)
    })
}

/// Whether `keystroke` runs `command` right now. Called by every handler.
pub(crate) fn pressed(command: Command, keystroke: &Keystroke) -> bool {
    with_store(|store| store.pressed(command, keystroke))
}

pub(crate) fn bindings(command: Command) -> Vec<Keystroke> {
    with_store(|store| store.bindings(command))
}

pub(crate) fn is_default(command: Command) -> bool {
    with_store(|store| store.is_default(command))
}

/// The first binding, displayed; `None` if unbound.
pub(crate) fn hint(command: Command) -> Option<String> {
    bindings(command).first().map(display)
}

/// A snapshot of the global store, for the window.
pub(crate) fn store() -> Store {
    with_store(|store| store.clone())
}

/// Replaces the global store, saves it, and re-applies [`apply_bindings`].
pub(crate) fn replace(store: Store, cx: &mut App) {
    if let Some(path) = config_path() {
        // A read-only config directory must not stop the rebinding from
        // taking effect for this session.
        let _ = store.save(&path);
    }
    STORE.with(|cell| *cell.borrow_mut() = Some(store));
    apply_bindings(cx);
}

/// The command a menu action stands for, for menu and palette hints.
pub(crate) fn command_for_action(action: &dyn Action) -> Option<Command> {
    use crate::menu_bar::{
        MenuCommandPalette, MenuCopyInstance, MenuCutInstance, MenuDeleteInstance,
        MenuDuplicateInstance, MenuGroup, MenuInsertFolder, MenuInsertObject, MenuInsertPart,
        MenuNewPlace, MenuOpenPlace, MenuPasteInstance, MenuPasteIntoInstance, MenuQuickOpen,
        MenuRedo, MenuSave, MenuStudioSettings, MenuUndo, MenuUngroup,
    };
    let id = action.as_any().type_id();
    let table: [(TypeId, Command); 19] = [
        (TypeId::of::<MenuNewPlace>(), Command::NewPlace),
        (TypeId::of::<MenuOpenPlace>(), Command::OpenPlace),
        (TypeId::of::<MenuInsertObject>(), Command::InsertObject),
        (TypeId::of::<MenuSave>(), Command::Save),
        (TypeId::of::<MenuUndo>(), Command::Undo),
        (TypeId::of::<MenuRedo>(), Command::Redo),
        (TypeId::of::<MenuCutInstance>(), Command::Cut),
        (TypeId::of::<MenuCopyInstance>(), Command::Copy),
        (TypeId::of::<MenuPasteInstance>(), Command::Paste),
        (TypeId::of::<MenuPasteIntoInstance>(), Command::PasteInto),
        (TypeId::of::<MenuDuplicateInstance>(), Command::Duplicate),
        (TypeId::of::<MenuDeleteInstance>(), Command::Delete),
        (TypeId::of::<MenuGroup>(), Command::Group),
        (TypeId::of::<MenuUngroup>(), Command::Ungroup),
        (TypeId::of::<MenuInsertPart>(), Command::InsertPart),
        (TypeId::of::<MenuInsertFolder>(), Command::InsertFolder),
        (TypeId::of::<MenuStudioSettings>(), Command::StudioSettings),
        (TypeId::of::<MenuCommandPalette>(), Command::CommandPalette),
        (TypeId::of::<MenuQuickOpen>(), Command::QuickOpen),
    ];
    table
        .into_iter()
        .find_map(|(candidate, command)| (candidate == id).then_some(command))
}

/// A command whose key lives in GPUI's keymap rather than a key handler.
struct KeymapEntry {
    command: Command,
    action: TypeId,
    bind: Box<dyn Fn(&str) -> KeyBinding>,
}

fn entry<A: Action + Clone>(
    command: Command,
    action: A,
    context: Option<&'static str>,
) -> KeymapEntry {
    KeymapEntry {
        command,
        action: TypeId::of::<A>(),
        bind: Box::new(move |spelling| KeyBinding::new(spelling, action.clone(), context)),
    }
}

/// Lets a module whose action type is private to it put `command` in the
/// keymap; [`apply_bindings`] then manages its keys.
pub(crate) fn register_keymap_action<A: Action + Clone>(
    command: Command,
    action: A,
    context: Option<&'static str>,
) {
    REGISTERED.with(|list| list.borrow_mut().push(entry(command, action, context)));
}

/// Puts the keymap-backed commands (Studio Settings, Command Palette, Quick
/// Open, Summon Handles) in GPUI's keymap with their current keys,
/// replacing whatever bindings those actions had. Called once at startup
/// and after every change.
pub(crate) fn apply_bindings(cx: &mut App) {
    use crate::menu_bar::{MenuCommandPalette, MenuQuickOpen, MenuStudioSettings};
    let builtin = [
        entry(Command::StudioSettings, MenuStudioSettings, None),
        entry(Command::CommandPalette, MenuCommandPalette, None),
        entry(Command::QuickOpen, MenuQuickOpen, None),
    ];
    REGISTERED.with(|list| {
        let list = list.borrow();
        let entries: Vec<&KeymapEntry> = builtin.iter().chain(list.iter()).collect();
        let owned: Vec<TypeId> = entries.iter().map(|entry| entry.action).collect();
        let kept: Vec<KeyBinding> = cx
            .key_bindings()
            .borrow()
            .bindings()
            .filter(|binding| !owned.contains(&binding.action().as_any().type_id()))
            .cloned()
            .collect();
        let added: Vec<KeyBinding> = entries
            .iter()
            .flat_map(|entry| {
                bindings(entry.command)
                    .into_iter()
                    .map(|key| (entry.bind)(&key.unparse()))
            })
            .collect();
        cx.clear_key_bindings();
        cx.bind_keys(kept);
        cx.bind_keys(added);
    });
}

#[cfg(test)]
mod tests;
