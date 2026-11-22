//! The Model tab's Solid Modeling group: Union, Intersect, Negate and
//! Separate on the selection, carved by `rbx_lua::csg` exactly as
//! `plugin:Union` and its siblings do, one `Ctrl+Z` each.

use gpui_kit::assets::IconName;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_lua::csg::{self, Operation};
use rbx_reflection::ReflectionDatabase;

use super::ribbon::{tile, unavailable_tile};
use super::Shell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Command {
    Union,
    Intersect,
    Negate,
    Separate,
}

fn is_part(dom: &WeakDom, db: &ReflectionDatabase, r: Ref) -> bool {
    dom.get(r)
        .is_some_and(|i| db.is_subclass_of(i.class(), "BasePart") && i.class() != "Terrain")
}

/// Whether `command` has anything to act on, the tile's grey state: two
/// parts to union or intersect, one to negate, one operation to separate.
pub(super) fn can(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    selected: &[Ref],
    command: Command,
) -> bool {
    let parts = selected.iter().filter(|&&r| is_part(dom, db, r)).count();
    match command {
        Command::Union | Command::Intersect => parts >= 2,
        Command::Negate => parts >= 1,
        Command::Separate => selected.iter().any(|&r| {
            dom.get(r)
                .is_some_and(|i| db.is_subclass_of(i.class(), "PartOperation"))
        }),
    }
}

/// The DOM half of `command`: what is selected afterwards, or `None` when
/// it made nothing (an empty intersection, say).
fn apply(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    selected: &[Ref],
    command: Command,
) -> Option<Vec<Ref>> {
    let made = match command {
        Command::Union => vec![csg::operate_in_place(dom, db, Operation::Union, selected).ok()?],
        Command::Intersect => {
            vec![csg::operate_in_place(dom, db, Operation::Intersect, selected).ok()?]
        }
        Command::Negate => csg::negate_in_place(dom, db, selected),
        Command::Separate => csg::separate_in_place(dom, db, selected),
    };
    (!made.is_empty()).then_some(made)
}

/// Each tile: its command, id, icon, label and why it greys out.
const TILES: [(Command, &str, IconName, &str, &str); 4] = [
    (
        Command::Union,
        "ribbon-union",
        IconName::Blend,
        "Union",
        "select two or more parts",
    ),
    (
        Command::Intersect,
        "ribbon-intersect",
        IconName::Contrast,
        "Intersect",
        "select two or more parts",
    ),
    (
        Command::Negate,
        "ribbon-negate",
        IconName::SquareDashed,
        "Negate",
        "select a part",
    ),
    (
        Command::Separate,
        "ribbon-separate",
        IconName::Scissors,
        "Separate",
        "select a union",
    ),
];

impl Shell {
    /// The Model tab's Solid Modeling group, greyed on [`can`].
    pub(super) fn solid_modeling_tiles(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let selected = self.selected_all();
        TILES
            .into_iter()
            .map(|(command, id, icon, label, reason)| {
                if can(&self.dom, &self.database, selected, command) {
                    tile(&self.ribbon_nav, id, icon, label, cx)
                        .on_click(
                            cx.listener(move |shell, _, _, cx| shell.solid_modeling(command, cx)),
                        )
                        .into_any_element()
                } else {
                    unavailable_tile(id, icon, label, reason).into_any_element()
                }
            })
            .collect()
    }

    /// Runs `command` on the selection as one undo step; a command that
    /// makes nothing leaves the place and the history as they were.
    pub(super) fn solid_modeling(&mut self, command: Command, cx: &mut Context<Self>) {
        let selected = self.selected_all().to_vec();
        if !can(&self.dom, &self.database, &selected, command) {
            return;
        }
        self.dom.take_changes();
        let before = self.dom.clone();
        let Some(made) = apply(&mut self.dom, &self.database, &selected, command) else {
            self.dom = before;
            return;
        };
        self.push_history_snapshot(before);
        let changes = self.dom.take_changes();
        self.rebuild_explorer(cx);
        self.reselect(made, cx);
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
    use rbx_reflection::ReflectionDatabase;

    // Not `super::*`: the glob of `gpui_kit` would shadow `#[test]`.
    use super::{apply, can, Command};

    fn database() -> ReflectionDatabase {
        rbx_reflection::ReflectionDatabase::embedded()
    }

    fn part(dom: &mut WeakDom, parent: Ref, x: f32) -> Ref {
        let r = dom.new_instance("Part", "Part", Some(parent));
        let props = dom.get_mut(r).unwrap().properties_mut();
        props.insert(
            "size".into(),
            Variant::Vector3(Vector3Data {
                x: 2.0,
                y: 2.0,
                z: 2.0,
            }),
        );
        props.insert(
            "CFrame".into(),
            Variant::CFrame(CFrameData {
                position: Vector3Data { x, y: 0.0, z: 0.0 },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        );
        r
    }

    #[test]
    fn union_then_separate_gives_the_parts_back_where_they_were() {
        let db = database();
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let a = part(&mut dom, workspace, 0.0);
        let b = part(&mut dom, workspace, 1.0);
        let union = apply(&mut dom, &db, &[a, b], Command::Union).unwrap()[0];
        assert_eq!(dom.get(union).unwrap().class(), "UnionOperation");
        assert_eq!(dom.parent(union), Some(workspace));
        assert!(dom.get(a).is_none() && dom.get(b).is_none());
        assert!(can(&dom, &db, &[union], Command::Separate));
        let parts = apply(&mut dom, &db, &[union], Command::Separate).unwrap();
        assert_eq!(parts.len(), 2);
        let xs: Vec<f32> = parts
            .iter()
            .map(|&p| match dom.get(p).unwrap().properties().get("CFrame") {
                Some(Variant::CFrame(cf)) => cf.position.x,
                _ => f32::NAN,
            })
            .collect();
        assert!(
            (xs[0] - 0.0).abs() < 1e-4 && (xs[1] - 1.0).abs() < 1e-4,
            "{xs:?}"
        );
        assert!(dom.get(union).is_none());
    }

    #[test]
    fn a_negated_part_is_carved_out_of_the_union() {
        let db = database();
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let a = part(&mut dom, workspace, 0.0);
        let b = part(&mut dom, workspace, 1.0);
        let negate = apply(&mut dom, &db, &[b], Command::Negate).unwrap()[0];
        assert_eq!(dom.get(negate).unwrap().class(), "NegateOperation");
        let union = apply(&mut dom, &db, &[a, negate], Command::Union).unwrap()[0];
        let size = match dom.get(union).unwrap().properties().get("size") {
            Some(Variant::Vector3(v)) => v.x,
            _ => f32::NAN,
        };
        assert!((size - 1.0).abs() < 1e-4, "{size}");
    }

    #[test]
    fn nothing_to_intersect_changes_nothing() {
        let db = database();
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let a = part(&mut dom, workspace, 0.0);
        let b = part(&mut dom, workspace, 10.0);
        assert!(apply(&mut dom, &db, &[a, b], Command::Intersect).is_none());
    }
}
