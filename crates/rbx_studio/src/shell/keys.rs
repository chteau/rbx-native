//! Two keyboard-driven Explorer actions: delete the selection and its
//! subtree, or insert a quick `Part`/`Folder` under it. Both push through the
//! same DOM take/put-back path `shell::command` already uses for scripts.
//!
//! `RBX_STUDIO_DELETE=1` and `RBX_STUDIO_INSERT=Part|Folder|Script|
//! LocalScript|ModuleScript` apply one of these once, right after startup,
//! through the exact path a keypress would use — debugging aids for a
//! screenshot, since nothing else can send a keystroke to the Explorer on
//! the editor's behalf (see `AGENTS.md`'s safety rules).

use gpui_kit::{Context, Keystroke, Modifiers, Window};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::change_class;
use crate::explorer;
use crate::script_editor::source;

use super::Shell;

/// `Script`/`LocalScript`'s starter `Source`. Roblox's own default new-script
/// text is this one line; a `LocalScript` differs from a `Script` only in
/// where it runs, not in what's worth starting from, so both share it (see
/// `default_template`).
const SCRIPT_TEMPLATE: &str = "print(\"Hello, world!\")\n";

/// `ModuleScript`'s starter `Source`: a module returning a plain table, the
/// shape most Luau modules start from before they need anything fancier.
const MODULE_TEMPLATE: &str = "local module = {}\n\nreturn module\n";

/// The OOP starter "Insert ModuleScript (Class)" seeds a new `ModuleScript`
/// with (see `Shell::insert_class_module`): a `.new()` constructor over a
/// metatable, the idiomatic shape for a module that models a class rather
/// than a namespace of functions.
const MODULE_CLASS_TEMPLATE: &str = concat!(
    "local ClassName = {}\n",
    "ClassName.__index = ClassName\n",
    "\n",
    "function ClassName.new()\n",
    "\tlocal self = setmetatable({}, ClassName)\n",
    "\treturn self\n",
    "end\n",
    "\n",
    "return ClassName\n",
);

/// The starter `Source` `insert_instance` writes for a freshly inserted
/// `class`, or `None` for anything that isn't a script — `Part`'s own
/// defaults and every other class stay untouched, exactly as before this
/// existed. `ModuleScript` gets [`MODULE_TEMPLATE`]; every other script class
/// (`Script`, `LocalScript`, and any future `LuaSourceContainer` subclass)
/// gets [`SCRIPT_TEMPLATE`].
fn default_template(database: &ReflectionDatabase, class: &str) -> Option<&'static str> {
    if !source::is_script_class(database, class) {
        return None;
    }
    Some(if class == "ModuleScript" {
        MODULE_TEMPLATE
    } else {
        SCRIPT_TEMPLATE
    })
}

/// Read once at startup by `Shell::new`; documented in this module's doc
/// comment.
pub(crate) const INSERT_VARIABLE: &str = "RBX_STUDIO_INSERT";
pub(crate) const DELETE_VARIABLE: &str = "RBX_STUDIO_DELETE";

/// What one keystroke does in the Explorer, or `None` if it means something
/// else there (a filter box or the viewport never dispatch here at all — see
/// `Shell::handle_explorer_key`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Delete,
    InsertPart,
    InsertFolder,
    /// The `+` picker, on the selected row — real Studio's own shortcut for
    /// it (`studio/explorer.md`).
    Insert,
    Rename,
}

/// Maps one keystroke to an Explorer action. `delete` and `backspace` both
/// remove the selection: GPUI's Linux backend reports the two physical keys
/// under those two different `Keystroke::key` names, and Studio itself binds
/// both to the same command.
pub(super) fn action_for(key: &str, modifiers: Modifiers) -> Option<Action> {
    match key {
        "delete" | "backspace" => Some(Action::Delete),
        "p" if modifiers.control && modifiers.shift => Some(Action::InsertPart),
        "f" if modifiers.control && modifiers.shift => Some(Action::InsertFolder),
        // Ctrl alone, not Ctrl+Shift: the other two are this editor's own
        // quick inserts, this one is the shortcut Studio documents.
        "i" if modifiers.control && !modifiers.shift => Some(Action::Insert),
        "f2" if !modifiers.modified() => Some(Action::Rename),
        _ => None,
    }
}

/// Whether an instance can be removed at all. A service cannot: Roblox
/// creates exactly one of each, and a place whose `Workspace` has been
/// deleted is not a place this editor — or Roblox — can open again. The same
/// singleton rule `explorer::reparent` applies to dragging one somewhere
/// else and `shell::clipboard` to copying one.
pub(super) fn removable(dom: &WeakDom, database: &ReflectionDatabase, reference: Ref) -> bool {
    dom.get(reference)
        .is_some_and(|instance| !database.is_service(instance.class()))
}

/// Whichever selection survives a delete: cleared only if it sat inside the
/// just-removed subtree (`removed` is `WeakDom::remove`'s own return value).
pub(super) fn selection_after_removal(selected: Option<Ref>, removed: &[Ref]) -> Option<Ref> {
    selected.filter(|reference| !removed.contains(reference))
}

impl Shell {
    /// The Explorer's `on_key_down` handler (see `Shell::instance_tree`). It
    /// only ever runs while some row in the tree holds focus, since that is
    /// the only place GPUI's dispatch path puts it — a filter box or the
    /// viewport keeps its own focus and never bubbles a key here.
    pub(super) fn handle_explorer_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // See `Shell::renaming_in_place`: Backspace in an open name box is a
        // character, not the instance being renamed.
        if self.renaming_in_place() {
            return;
        }
        match action_for(&keystroke.key, keystroke.modifiers) {
            Some(Action::Delete) => self.delete_selected(cx),
            Some(Action::InsertPart) => self.insert_instance("Part", cx),
            Some(Action::InsertFolder) => self.insert_instance("Folder", cx),
            Some(Action::Insert) => self.open_insert_picker_on_selection(window, cx),
            Some(Action::Rename) => self.begin_rename_selection(window, cx),
            None => {}
        }
    }

    /// Removes the selected instance and its subtree, exactly as a script's
    /// `:Destroy()` would, through the same take/put-back path `shell::command`
    /// uses. A no-op with nothing selected. `pub(crate)`: also `menu_bar`'s
    /// Delete item's entry point, so a menu click runs the exact same path
    /// the Delete key does.
    pub(crate) fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let Some(reference) = self.selected() else {
            return;
        };
        self.remove_instances(&[reference], cx);
    }

    /// Removes each of `references` and its subtree as one undo step. Shared
    /// with `shell::clipboard`'s Cut, which has a whole selection to take
    /// out rather than the Delete key's single row.
    pub(super) fn remove_instances(&mut self, references: &[Ref], cx: &mut Context<Self>) {
        let doomed: Vec<Ref> = references
            .iter()
            .copied()
            .filter(|&reference| removable(&self.dom, &self.database, reference))
            .collect();
        if doomed.is_empty() {
            return;
        }

        // See `shell::history`: snapshotted before the removals below.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let removed: Vec<Ref> = doomed
            .iter()
            .flat_map(|&reference| dom.remove(reference))
            .collect();
        self.dom = dom;
        // One `Change::Removed` per instance in each subtree, each taken out
        // of the viewport in place — and put back the same way when this is
        // undone, since the log is reflected against whichever DOM stands.
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        // Going through the pure function rather than assuming the selection
        // died with the subtree keeps the two "was it inside" checks (this
        // one and its unit tests) reading the same rule.
        match selection_after_removal(self.selected(), &removed) {
            Some(kept) => self.select(kept, cx),
            None => self.deselect(cx),
        }
        // After the selection settled, not before: what the viewport is
        // told to refresh depends on what is selected now.
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        cx.notify();
    }

    /// Inserts a new `class` instance under the current selection, or under
    /// `Workspace` when nothing is selected, seeded with `class`'s starter
    /// `Source` if it's a script (see [`default_template`]). Roblox parents
    /// almost anything almost anywhere in practice, so this stays permissive
    /// rather than special-casing classes that cannot sensibly take
    /// children — the two quick-insert keys only ever pass `"Part"` or
    /// `"Folder"` here anyway. `pub(crate)`: also `menu_bar`'s Insert
    /// Part/Folder/Script/LocalScript/ModuleScript items' entry point, so a
    /// menu click runs the exact same path the quick-insert keys do.
    pub(crate) fn insert_instance(&mut self, class: &str, cx: &mut Context<Self>) {
        // A user's `Default.luau` for the class wins over the built-in
        // starter, so "every new Script looks like this" is one file.
        let template = self
            .script_templates
            .default_for(class)
            .or_else(|| default_template(&self.database, class))
            .map(str::to_owned);
        self.insert_instance_with_source(class, None, template.as_deref(), None, cx);
    }

    /// The `+` picker's entry point: the same insert, under the row whose
    /// `+` was clicked rather than under the selection.
    pub(super) fn insert_instance_under(
        &mut self,
        parent: Option<Ref>,
        class: &str,
        cx: &mut Context<Self>,
    ) {
        let template = self
            .script_templates
            .default_for(class)
            .or_else(|| default_template(&self.database, class))
            .map(str::to_owned);
        self.insert_instance_with_source(class, None, template.as_deref(), parent, cx);
    }

    /// The ribbon Script menu's user-defined entries (see
    /// `crate::script_templates`): inserts the `index`th extra template as
    /// its own class with its own source. An index that no longer resolves
    /// is a no-op rather than a panic — the menu is built from the same list
    /// a frame earlier, but nothing ties the two lifetimes together.
    pub(crate) fn insert_user_template(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(template) = self.script_templates.extras().get(index).cloned() else {
            return;
        };
        self.insert_instance_with_source(template.class, None, Some(&template.source), None, cx);
    }

    /// The ribbon Part menu's Block/Sphere/Cylinder items (see
    /// `shell::ribbon::insert_tiles`): all three insert a bare `Part` and are
    /// told apart only by `shape` (`Enum.PartType`) — Wedge/CornerWedge
    /// disambiguate through their own class instead and go through
    /// [`insert_instance`](Self::insert_instance) unchanged.
    pub(crate) fn insert_part(&mut self, class: &str, shape: u32, cx: &mut Context<Self>) {
        self.insert_instance_with_source(class, Some(shape), None, None, cx);
    }

    /// "Insert ModuleScript (Class)" (see `menu_bar`): the one script insert
    /// that picks a specific template rather than the per-class default
    /// [`insert_instance`](Self::insert_instance) uses — the OOP starter
    /// `ROADMAP.md`'s "New-script templates" entry asks for alongside the
    /// plain-table default `ModuleScript` otherwise gets.
    pub(crate) fn insert_class_module(&mut self, cx: &mut Context<Self>) {
        self.insert_instance_with_source(
            "ModuleScript",
            None,
            Some(MODULE_CLASS_TEMPLATE),
            None,
            cx,
        );
    }

    /// Shared by [`insert_instance`](Self::insert_instance),
    /// [`insert_part`](Self::insert_part) and
    /// [`insert_class_module`](Self::insert_class_module): inserts `class`,
    /// applies `BasePart` defaults (writing `shape` too, for the classes that
    /// actually declare it — see [`apply_part_defaults`]) and, when `source`
    /// is `Some`, writes it to the new instance's `Source` through
    /// `script_editor::source::write` — the same path a script tab's own
    /// edits commit through, so undo, Ctrl+S and the script editor need to
    /// know nothing about how the instance got here.
    fn insert_instance_with_source(
        &mut self,
        class: &str,
        shape: Option<u32>,
        source: Option<&str>,
        under: Option<Ref>,
        cx: &mut Context<Self>,
    ) {
        let parent = under
            .or_else(|| self.selected())
            .or_else(|| explorer::find_by_name(&self.dom, "Workspace"));
        // A new instance is named after its class; the increment preference
        // is what turns a second `Part` into `Part1` (see
        // `explorer::insert::incremented_name`).
        let name = if self.increment_names() {
            explorer::insert::incremented_name(&self.dom, parent, class)
        } else {
            class.to_owned()
        };

        // See `shell::history`: snapshotted before the insert below.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let reference = dom.new_instance(class, &name, parent);
        if let Some(part_shape) = part_defaults_shape(&self.database, class, shape) {
            apply_part_defaults(&mut dom, reference, part_shape);
        }
        if let Some(text) = source {
            crate::script_editor::source::write(&mut dom, reference, text);
        }
        self.dom = dom;
        // An insert logs a `Change::Added` plus, for a `Part`, a dozen
        // `Property` writes for its defaults (or, for a script, one `Source`
        // write), all on that same new instance: one instance for the
        // viewport to build however many writes set it up, and one to take
        // out again when this is undone.
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        self.select(reference, cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        cx.notify();
    }

    /// `RBX_STUDIO_DELETE=1` / `RBX_STUDIO_INSERT=Part|Folder|Script|...`:
    /// documented in this module's doc comment. An unrecognized class is
    /// silently ignored, like `RBX_STUDIO_EDIT`'s malformed specs — a
    /// screenshot aid, not user input, and must never crash a debugging
    /// session.
    pub(super) fn apply_debug_explorer_action(&mut self, cx: &mut Context<Self>) {
        if let Ok(class) = std::env::var(INSERT_VARIABLE) {
            let recognized = class == "Part"
                || class == "Folder"
                || source::is_script_class(&self.database, &class);
            if recognized {
                self.insert_instance(&class, cx);
            }
        }
        if std::env::var(DELETE_VARIABLE).is_ok() {
            self.delete_selected(cx);
        }
    }
}

/// `Enum.PartType`, for the classes the Part insert menu can create through a
/// bare `Part` — Wedge/CornerWedge disambiguate through their own class
/// instead (see `rbx_viewer::scene::shape::resolve` and
/// `Shell::insert_instance_with_source`).
pub(super) const PART_TYPE_BALL: u32 = 0;
pub(super) const PART_TYPE_BLOCK: u32 = 1;
pub(super) const PART_TYPE_CYLINDER: u32 = 2;

/// Whether `class` gets [`apply_part_defaults`] at all, and if so, what
/// `shape` to pass it: `None` for anything that isn't a `BasePart` subclass;
/// `Some(None)` for a `BasePart` subclass with no `Shape` property of its own
/// (`WedgePart`, `CornerWedgePart`); `Some(Some(_))` for `Part` itself, using
/// the caller's requested `shape` or [`PART_TYPE_BLOCK`] if it didn't ask for
/// one.
fn part_defaults_shape(
    database: &ReflectionDatabase,
    class: &str,
    shape: Option<u32>,
) -> Option<Option<u32>> {
    database.is_subclass_of(class, "BasePart").then(|| {
        database
            .is_subclass_of(class, "Part")
            .then(|| shape.unwrap_or(PART_TYPE_BLOCK))
    })
}

/// What Change Class gives an instance that had no value at all for these:
/// [`part_defaults`]'s keys for a `BasePart`, with `Part`'s own block `shape`,
/// each at `class`'s own default where the per-class table records one in the
/// type the DOM stores it as — a `TrussPart` is 2 × 2 × 2, not a `Part`'s
/// 4 × 1.2 × 2 — and nothing for any other class.
pub(super) fn class_defaults(
    database: &ReflectionDatabase,
    class: &str,
) -> Vec<(&'static str, Variant)> {
    let own = |key: &str, value: &Variant| {
        let property = rbx_lua::reflected_property(database, class, key)?;
        change_class::stock(database, class, &property.name)
            .filter(|own| std::mem::discriminant(*own) == std::mem::discriminant(value))
            .cloned()
    };
    part_defaults_shape(database, class, None)
        .map(part_defaults)
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| {
            let value = own(key, &value).unwrap_or(value);
            (key, value)
        })
        .collect()
}

/// Applies [`part_defaults`] to a freshly inserted instance.
fn apply_part_defaults(dom: &mut WeakDom, referent: Ref, shape: Option<u32>) {
    for (key, value) in part_defaults(shape) {
        let _ = dom.set_property(referent, key, value);
    }
}

/// Roblox's own defaults for `Instance.new("Part")`, duplicated from
/// `rbx_lua::defaults::base_part_defaults` rather than reused: that table sits
/// behind a `pub(crate)` `apply` function private to `rbx_lua`, and making a
/// whole module public across crates for one small constant table is not
/// worth it. Applies to any `BasePart` subclass the insert menus create
/// (`Part`, `WedgePart`, `CornerWedgePart`) — `shape` is listed only when
/// the caller passes one, since `WedgePart`/`CornerWedgePart` don't actually
/// declare a `Shape` property in Roblox's own reflection data.
fn part_defaults(shape: Option<u32>) -> Vec<(&'static str, Variant)> {
    const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    const MATERIAL_PLASTIC: u32 = 256;

    let mut defaults = vec![
        (
            "size",
            Variant::Vector3(Vector3Data {
                x: 4.0,
                y: 1.2,
                z: 2.0,
            }),
        ),
        (
            "CFrame",
            Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: IDENTITY_ROTATION,
            }),
        ),
        (
            "Color3uint8",
            Variant::Color3uint8 {
                r: 163,
                g: 162,
                b: 165,
            },
        ),
        ("Material", Variant::Enum(MATERIAL_PLASTIC)),
        ("Anchored", Variant::Bool(false)),
        ("CanCollide", Variant::Bool(true)),
        ("Transparency", Variant::Float32(0.0)),
        ("Reflectance", Variant::Float32(0.0)),
        ("CastShadow", Variant::Bool(true)),
        ("Locked", Variant::Bool(false)),
        ("Massless", Variant::Bool(false)),
    ];
    if let Some(shape) = shape {
        defaults.push(("shape", Variant::Enum(shape)));
    }
    defaults
}

#[cfg(test)]
#[path = "keys/tests.rs"]
mod tests;
