//! Two keyboard-driven Explorer actions: delete the selection and its
//! subtree, or insert a quick `Part`/`Folder` under it. Both push through the
//! same DOM take/put-back path `shell::command` already uses for scripts.
//!
//! `RBX_STUDIO_DELETE=1` and `RBX_STUDIO_INSERT=Part|Folder|Script|
//! LocalScript|ModuleScript` apply one of these once, right after startup,
//! through the exact path a keypress would use — debugging aids for a
//! screenshot, since nothing else can send a keystroke to the Explorer on
//! the editor's behalf (see `AGENTS.md`'s safety rules).

use gpui_kit::{Context, Keystroke, Modifiers};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

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
        _ => None,
    }
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
    pub(super) fn handle_explorer_key(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) {
        match action_for(&keystroke.key, keystroke.modifiers) {
            Some(Action::Delete) => self.delete_selected(cx),
            Some(Action::InsertPart) => self.insert_instance("Part", cx),
            Some(Action::InsertFolder) => self.insert_instance("Folder", cx),
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

        // See `shell::history`: snapshotted before the removal below.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let removed = dom.remove(reference);
        self.dom = dom;
        // One `Change::Removed` per instance in the subtree, each taken out
        // of the viewport in place — and put back the same way when this is
        // undone, since the log is reflected against whichever DOM stands.
        let changes = self.dom.take_changes();

        self.rebuild_explorer(cx);
        // The Explorer holds one selection, always the deleted root itself,
        // so this only ever resolves to `None` — going through the pure
        // function anyway keeps the two "was it inside the subtree" checks
        // (this one and its unit tests) reading the same rule.
        match selection_after_removal(Some(reference), &removed) {
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
        self.insert_instance_with_source(class, None, template.as_deref(), cx);
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
        self.insert_instance_with_source(template.class, None, Some(&template.source), cx);
    }

    /// The ribbon Part menu's Block/Sphere/Cylinder items (see
    /// `shell::ribbon::insert_tiles`): all three insert a bare `Part` and are
    /// told apart only by `shape` (`Enum.PartType`) — Wedge/CornerWedge
    /// disambiguate through their own class instead and go through
    /// [`insert_instance`](Self::insert_instance) unchanged.
    pub(crate) fn insert_part(&mut self, class: &str, shape: u32, cx: &mut Context<Self>) {
        self.insert_instance_with_source(class, Some(shape), None, cx);
    }

    /// "Insert ModuleScript (Class)" (see `menu_bar`): the one script insert
    /// that picks a specific template rather than the per-class default
    /// [`insert_instance`](Self::insert_instance) uses — the OOP starter
    /// `ROADMAP.md`'s "New-script templates" entry asks for alongside the
    /// plain-table default `ModuleScript` otherwise gets.
    pub(crate) fn insert_class_module(&mut self, cx: &mut Context<Self>) {
        self.insert_instance_with_source("ModuleScript", None, Some(MODULE_CLASS_TEMPLATE), cx);
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
        cx: &mut Context<Self>,
    ) {
        let parent = self
            .selected()
            .or_else(|| explorer::find_by_name(&self.dom, "Workspace"));

        // See `shell::history`: snapshotted before the insert below.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let reference = dom.new_instance(class, class, parent);
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

/// Roblox's own defaults for `Instance.new("Part")`, duplicated from
/// `rbx_lua::defaults::base_part_defaults` rather than reused: that table sits
/// behind a `pub(crate)` `apply` function private to `rbx_lua`, and making a
/// whole module public across crates for one small constant table is not
/// worth it. Applies to any `BasePart` subclass the insert menus create
/// (`Part`, `WedgePart`, `CornerWedgePart`) — `shape` is written only when
/// the caller passes one, since `WedgePart`/`CornerWedgePart` don't actually
/// declare a `Shape` property in Roblox's own reflection data.
fn apply_part_defaults(dom: &mut WeakDom, referent: Ref, shape: Option<u32>) {
    const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    const MATERIAL_PLASTIC: u32 = 256;

    let defaults: [(&str, Variant); 11] = [
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
    for (key, value) in defaults {
        let _ = dom.set_property(referent, key, value);
    }
    if let Some(shape) = shape {
        let _ = dom.set_property(referent, "shape", Variant::Enum(shape));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctrl_shift() -> Modifiers {
        Modifiers {
            control: true,
            shift: true,
            ..Modifiers::none()
        }
    }

    #[test]
    fn delete_and_backspace_both_delete_with_no_modifiers_required() {
        assert_eq!(
            action_for("delete", Modifiers::none()),
            Some(Action::Delete)
        );
        assert_eq!(
            action_for("backspace", Modifiers::none()),
            Some(Action::Delete)
        );
    }

    #[test]
    fn ctrl_shift_p_and_f_insert_part_and_folder() {
        assert_eq!(action_for("p", ctrl_shift()), Some(Action::InsertPart));
        assert_eq!(action_for("f", ctrl_shift()), Some(Action::InsertFolder));
    }

    #[test]
    fn p_and_f_without_both_modifiers_do_nothing() {
        assert_eq!(action_for("p", Modifiers::none()), None);
        assert_eq!(action_for("f", Modifiers::none()), None);
        let shift_only = Modifiers {
            shift: true,
            ..Modifiers::none()
        };
        assert_eq!(action_for("p", shift_only), None);
    }

    #[test]
    fn any_other_key_does_nothing() {
        assert_eq!(action_for("w", Modifiers::none()), None);
        assert_eq!(action_for("enter", Modifiers::none()), None);
    }

    #[test]
    fn deleting_the_selection_clears_it() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        let removed = dom.remove(part);
        assert_eq!(selection_after_removal(Some(part), &removed), None);
    }

    #[test]
    fn deleting_a_sibling_keeps_the_selection() {
        let mut dom = WeakDom::new();
        let kept = dom.new_instance("Part", "Kept", None);
        let other = dom.new_instance("Part", "Other", None);
        let removed = dom.remove(other);
        assert_eq!(selection_after_removal(Some(kept), &removed), Some(kept));
    }

    #[test]
    fn deleting_an_ancestor_clears_a_selection_inside_its_subtree() {
        let mut dom = WeakDom::new();
        let model = dom.new_instance("Model", "Model", None);
        let child = dom.new_instance("Part", "Part", Some(model));
        let removed = dom.remove(model);
        assert_eq!(selection_after_removal(Some(child), &removed), None);
    }

    #[test]
    fn no_selection_stays_none() {
        assert_eq!(selection_after_removal(None, &[]), None);
    }

    #[test]
    fn inserted_part_gets_studio_s_visible_defaults() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        apply_part_defaults(&mut dom, part, Some(PART_TYPE_BLOCK));

        let instance = dom.get(part).expect("just inserted");
        assert_eq!(
            instance.properties().get("size"),
            Some(&Variant::Vector3(Vector3Data {
                x: 4.0,
                y: 1.2,
                z: 2.0
            }))
        );
        assert_eq!(
            instance.properties().get("Material"),
            Some(&Variant::Enum(256))
        );
        assert_eq!(instance.properties().get("shape"), Some(&Variant::Enum(1)));
        assert!(matches!(
            instance.properties().get("CFrame"),
            Some(Variant::CFrame(_))
        ));
    }

    /// Mirrors what `Shell::insert_instance`/`insert_part` does for one Part
    /// insert-menu item, without needing a real `Shell`/`Context` — the same
    /// reason [`apply_part_defaults`]'s own tests above work on a bare
    /// `WeakDom` instead.
    fn insert_menu_item(class: &str, shape: Option<u32>) -> (WeakDom, Ref) {
        let mut dom = WeakDom::new();
        let reference = dom.new_instance(class, class, None);
        if let Some(part_shape) = part_defaults_shape(&database(), class, shape) {
            apply_part_defaults(&mut dom, reference, part_shape);
        }
        (dom, reference)
    }

    /// One test per `shell::ribbon::insert_tiles` Part-menu item. The bug
    /// `ROADMAP.md` describes was invisible in the Explorer — every item
    /// already showed the right class — and only showed once the viewport
    /// resolved the wrong `ShapeKind`, so each of these checks both.
    #[test]
    fn block_menu_item_is_a_part_that_renders_as_a_box() {
        let (dom, part) = insert_menu_item("Part", None);
        assert_eq!(dom.get(part).expect("just inserted").class(), "Part");
        assert_eq!(
            rbx_viewer::resolved_shape_label(&dom, &database(), part),
            Some("Box")
        );
    }

    #[test]
    fn sphere_menu_item_is_a_part_that_renders_as_a_ball() {
        let (dom, part) = insert_menu_item("Part", Some(PART_TYPE_BALL));
        assert_eq!(dom.get(part).expect("just inserted").class(), "Part");
        assert_eq!(
            rbx_viewer::resolved_shape_label(&dom, &database(), part),
            Some("Ball")
        );
    }

    #[test]
    fn wedge_menu_item_is_a_wedge_part_with_studio_s_visible_defaults() {
        let (dom, part) = insert_menu_item("WedgePart", None);
        let instance = dom.get(part).expect("just inserted");
        assert_eq!(instance.class(), "WedgePart");
        assert_eq!(
            instance.properties().get("Material"),
            Some(&Variant::Enum(256)),
            "Wedge/CornerWedge must get apply_part_defaults too, not just Part"
        );
        assert_eq!(
            rbx_viewer::resolved_shape_label(&dom, &database(), part),
            Some("Wedge")
        );
    }

    #[test]
    fn corner_wedge_menu_item_is_a_corner_wedge_part_with_studio_s_visible_defaults() {
        let (dom, part) = insert_menu_item("CornerWedgePart", None);
        let instance = dom.get(part).expect("just inserted");
        assert_eq!(instance.class(), "CornerWedgePart");
        assert_eq!(
            instance.properties().get("Material"),
            Some(&Variant::Enum(256)),
            "Wedge/CornerWedge must get apply_part_defaults too, not just Part"
        );
        assert_eq!(
            rbx_viewer::resolved_shape_label(&dom, &database(), part),
            Some("CornerWedge")
        );
    }

    #[test]
    fn cylinder_menu_item_is_a_part_that_renders_as_a_cylinder() {
        let (dom, part) = insert_menu_item("Part", Some(PART_TYPE_CYLINDER));
        assert_eq!(dom.get(part).expect("just inserted").class(), "Part");
        assert_eq!(
            rbx_viewer::resolved_shape_label(&dom, &database(), part),
            Some("CylinderX")
        );
    }

    fn database() -> ReflectionDatabase {
        ReflectionDatabase::embedded()
    }

    /// Counts unmatched `function`/`do` openers against `end` closers by
    /// whitespace-splitting the template into tokens — no full Luau parser is
    /// needed to catch a template with a stray or missing `end`.
    fn opens_and_ends(template: &str) -> (usize, usize) {
        let opens = template
            .split_whitespace()
            .filter(|token| *token == "function" || *token == "do")
            .count();
        let ends = template
            .split_whitespace()
            .filter(|token| *token == "end")
            .count();
        (opens, ends)
    }

    #[test]
    fn a_non_script_class_gets_no_template() {
        let db = database();
        for class in ["Part", "Folder", "Model"] {
            assert_eq!(default_template(&db, class), None);
        }
    }

    #[test]
    fn module_script_gets_the_table_template_others_get_the_plain_script_template() {
        let db = database();
        assert_eq!(default_template(&db, "ModuleScript"), Some(MODULE_TEMPLATE));
        assert_eq!(default_template(&db, "Script"), Some(SCRIPT_TEMPLATE));
        assert_eq!(default_template(&db, "LocalScript"), Some(SCRIPT_TEMPLATE));
    }

    #[test]
    fn inserting_each_script_class_seeds_a_non_empty_starter_source() {
        let db = database();
        for class in ["Script", "LocalScript", "ModuleScript"] {
            let mut dom = WeakDom::new();
            let reference = dom.new_instance(class, class, None);
            let template = default_template(&db, class).expect("a script class has a template");
            assert!(source::write(&mut dom, reference, template));

            let text = source::read(&dom, reference).expect("Source was just written");
            assert!(
                !text.trim().is_empty(),
                "{class}'s starter text must not be empty"
            );
        }
    }

    #[test]
    fn inserting_a_part_gets_no_source_property() {
        let mut dom = WeakDom::new();
        let part = dom.new_instance("Part", "Part", None);
        apply_part_defaults(&mut dom, part, Some(PART_TYPE_BLOCK));

        assert_eq!(default_template(&database(), "Part"), None);
        assert_eq!(
            dom.get(part)
                .expect("just inserted")
                .properties()
                .get(source::SOURCE_PROPERTY),
            None,
            "a Part must never get a Source property"
        );
    }

    #[test]
    fn the_class_module_template_differs_from_the_plain_one() {
        assert_ne!(MODULE_CLASS_TEMPLATE, MODULE_TEMPLATE);
    }

    #[test]
    fn the_class_module_template_is_an_idiomatic_oop_stub() {
        assert!(MODULE_CLASS_TEMPLATE.contains("setmetatable"));
        assert!(MODULE_CLASS_TEMPLATE.contains("__index"));
        assert!(MODULE_CLASS_TEMPLATE.contains(".new("));
    }

    #[test]
    fn every_template_has_balanced_function_do_end_blocks() {
        for template in [SCRIPT_TEMPLATE, MODULE_TEMPLATE, MODULE_CLASS_TEMPLATE] {
            let (opens, ends) = opens_and_ends(template);
            assert_eq!(
                opens, ends,
                "unbalanced function/do/end in template: {template:?}"
            );
        }
    }
}
