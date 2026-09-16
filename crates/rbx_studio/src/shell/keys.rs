//! Two keyboard-driven Explorer actions: delete the selection and its
//! subtree, or insert a quick `Part`/`Folder` under it. Both push through the
//! same DOM take/put-back path `shell::command` already uses for scripts.
//!
//! `RBX_STUDIO_DELETE=1` and `RBX_STUDIO_INSERT=Part|Folder` apply one of
//! these once, right after startup, through the exact path a keypress would
//! use — debugging aids for a screenshot, since nothing else can send a
//! keystroke to the Explorer on the editor's behalf (see `AGENTS.md`'s
//! safety rules).

use gpui_kit::{Context, Keystroke, Modifiers};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};

use crate::explorer;

use super::Shell;

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
        // A subtree delete always logs more than one `Change::Removed`, or a
        // lone one that still isn't a `Property`/`Parent` write — either way
        // `single_instance_change` reads it as unclassifiable, so undoing this
        // always falls back to a full reload, correctly.
        let changes = self.dom.take_changes();
        self.record_history_change(changes);

        self.rebuild_explorer(cx);
        // The Explorer holds one selection, always the deleted root itself,
        // so this only ever resolves to `None` — going through the pure
        // function anyway keeps the two "was it inside the subtree" checks
        // (this one and its unit tests) reading the same rule.
        match selection_after_removal(Some(reference), &removed) {
            Some(kept) => self.select(kept, cx),
            None => self.deselect(cx),
        }
        self.reload_viewport(cx);
        cx.notify();
    }

    /// Inserts a new `class` instance under the current selection, or under
    /// `Workspace` when nothing is selected. Roblox parents almost anything
    /// almost anywhere in practice, so this stays permissive rather than
    /// special-casing classes that cannot sensibly take children — the two
    /// quick-insert keys only ever pass `"Part"` or `"Folder"` here anyway.
    /// `pub(crate)`: also `menu_bar`'s Insert Part/Folder items' entry point,
    /// so a menu click runs the exact same path the quick-insert keys do.
    pub(crate) fn insert_instance(&mut self, class: &str, cx: &mut Context<Self>) {
        let parent = self
            .selected()
            .or_else(|| explorer::find_by_name(&self.dom, "Workspace"));

        // See `shell::history`: snapshotted before the insert below.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        let reference = dom.new_instance(class, class, parent);
        if class == "Part" {
            apply_part_defaults(&mut dom, reference);
        }
        self.dom = dom;
        // An insert always logs a `Change::Added` (plus, for a `Part`, a
        // dozen more `Property` writes for its defaults, all on that same
        // new instance) — and an `Added` is what `single_instance_change`
        // refuses to classify however many writes sit beside it, so undoing
        // this falls back to a full reload, correctly.
        let changes = self.dom.take_changes();
        self.record_history_change(changes);

        self.rebuild_explorer(cx);
        self.select(reference, cx);
        self.reload_viewport(cx);
        cx.notify();
    }

    /// `RBX_STUDIO_DELETE=1` / `RBX_STUDIO_INSERT=Part|Folder`: documented in
    /// this module's doc comment. An unrecognized class is silently ignored,
    /// like `RBX_STUDIO_EDIT`'s malformed specs — a screenshot aid, not user
    /// input, and must never crash a debugging session.
    pub(super) fn apply_debug_explorer_action(&mut self, cx: &mut Context<Self>) {
        if let Ok(class) = std::env::var(INSERT_VARIABLE) {
            if class == "Part" || class == "Folder" {
                self.insert_instance(&class, cx);
            }
        }
        if std::env::var(DELETE_VARIABLE).is_ok() {
            self.delete_selected(cx);
        }
    }
}

/// Roblox's own defaults for `Instance.new("Part")`, duplicated from
/// `rbx_lua::defaults::base_part_defaults` rather than reused: that table sits
/// behind a `pub(crate)` `apply` function private to `rbx_lua`, and making a
/// whole module public across crates for one small constant table is not
/// worth it. Kept to just the shared `BasePart` geometry/appearance set plus
/// `Part`'s own `shape`, since only `Part` is ever created through this
/// quick-insert.
fn apply_part_defaults(dom: &mut WeakDom, referent: Ref) {
    const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    const MATERIAL_PLASTIC: u32 = 256;
    const PART_TYPE_BLOCK: u32 = 1;

    let defaults: [(&str, Variant); 12] = [
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
        ("shape", Variant::Enum(PART_TYPE_BLOCK)),
    ];
    for (key, value) in defaults {
        let _ = dom.set_property(referent, key, value);
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
        apply_part_defaults(&mut dom, part);

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
}
