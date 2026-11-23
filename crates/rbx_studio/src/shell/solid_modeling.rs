//! The Model tab's Solid Modeling group: Union, Intersect, Separate and
//! Negate on the selection, carved by `rbx_lua::csg` exactly as
//! `plugin:Union` and its siblings do, one `Ctrl+Z` each, on Studio's keys
//! (Ctrl+Shift+G, I, U and N). A command that fails says why over the 3D
//! view, in Studio's words where Studio has them ("Union failed").

use gpui_kit::assets::IconName;
use gpui_kit::*;
use rbx_dom::{Ref, WeakDom};
use rbx_lua::csg::{self, Operation};
use rbx_reflection::ReflectionDatabase;

use super::animation::toast::{show, ToastKind};
use super::ribbon::{tile, unavailable_tile};
use super::Shell;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Command {
    Union,
    Intersect,
    Separate,
    Negate,
}

impl Command {
    fn name(self) -> &'static str {
        match self {
            Command::Union => "Union",
            Command::Intersect => "Intersect",
            Command::Separate => "Separate",
            Command::Negate => "Negate",
        }
    }

    /// The command `keystroke` is bound to, if any.
    fn for_key(keystroke: &Keystroke) -> Option<Command> {
        use crate::shortcuts::{pressed, Command as Key};
        [
            (Key::Union, Command::Union),
            (Key::Intersect, Command::Intersect),
            (Key::Separate, Command::Separate),
            (Key::Negate, Command::Negate),
        ]
        .into_iter()
        .find(|(key, _)| pressed(*key, keystroke))
        .map(|(_, command)| command)
    }
}

fn is_part(dom: &WeakDom, db: &ReflectionDatabase, r: Ref) -> bool {
    dom.get(r)
        .is_some_and(|i| db.is_subclass_of(i.class(), "BasePart") && i.class() != "Terrain")
}

/// Whether `command` has anything to act on, the tile's grey state, as
/// Studio greys its own: a part to union or negate (Studio unions a lone
/// part too), two to intersect, an operation to separate.
pub(super) fn can(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    selected: &[Ref],
    command: Command,
) -> bool {
    let parts = selected.iter().filter(|&&r| is_part(dom, db, r)).count();
    match command {
        Command::Union | Command::Negate => parts >= 1,
        Command::Intersect => parts >= 2,
        Command::Separate => selected.iter().any(|&r| {
            dom.get(r)
                .is_some_and(|i| db.is_subclass_of(i.class(), "PartOperation"))
        }),
    }
}

/// The kernel's reasons (`rbx_csg::Failure`) told the way Studio tells them.
fn explain(reason: &str) -> String {
    match reason {
        // Studio's own words, from its "Intersect failed" dialog.
        "Empty" => "Result was empty, possibly because everything got subtracted away.".into(),
        "TooComplex" => "The result has too many triangles.".into(),
        "Leaky" | "Inverted" => "These parts don't make a closed solid together.".into(),
        other => other.into(),
    }
}

/// The DOM half of `command`: what is selected afterwards, or why it made
/// nothing.
fn apply(
    dom: &mut WeakDom,
    db: &ReflectionDatabase,
    selected: &[Ref],
    command: Command,
) -> Result<Vec<Ref>, String> {
    let made = match command {
        Command::Union => vec![csg::operate_in_place(dom, db, Operation::Union, selected)?],
        Command::Intersect => {
            vec![csg::operate_in_place(
                dom,
                db,
                Operation::Intersect,
                selected,
            )?]
        }
        Command::Negate => csg::negate_in_place(dom, db, selected),
        Command::Separate => csg::separate_in_place(dom, db, selected),
    };
    if made.is_empty() {
        return Err(match command {
            Command::Separate => "The union holds no parts to give back.".into(),
            _ => "None of the selected parts could be carved.".into(),
        });
    }
    Ok(made)
}

/// Each tile, in Studio's order: its command, id, icon and why it greys out.
const TILES: [(Command, &str, IconName, &str); 4] = [
    (
        Command::Union,
        "ribbon-union",
        IconName::Blend,
        "select a part",
    ),
    (
        Command::Intersect,
        "ribbon-intersect",
        IconName::Contrast,
        "select two or more parts",
    ),
    (
        Command::Separate,
        "ribbon-separate",
        IconName::Scissors,
        "select a union",
    ),
    (
        Command::Negate,
        "ribbon-negate",
        IconName::SquareDashed,
        "select a part",
    ),
];

impl Shell {
    /// The Model tab's Solid Modeling group, greyed on [`can`].
    pub(super) fn solid_modeling_tiles(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let selected = self.selected_all();
        TILES
            .into_iter()
            .map(|(command, id, icon, reason)| {
                let label = command.name();
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

    /// The window-level key handler's Solid Modeling half, beside Group's.
    pub(super) fn handle_solid_modeling_key(
        &mut self,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) {
        if let Some(command) = Command::for_key(keystroke) {
            self.solid_modeling(command, cx);
        }
    }

    /// Runs `command` on the selection as one undo step; a command that
    /// makes nothing leaves the place and the history as they were and says
    /// why.
    pub(super) fn solid_modeling(&mut self, command: Command, cx: &mut Context<Self>) {
        let selected = self.selected_all().to_vec();
        if !can(&self.dom, &self.database, &selected, command) {
            return;
        }
        self.dom.take_changes();
        let before = self.dom.clone();
        let made = match apply(&mut self.dom, &self.database, &selected, command) {
            Ok(made) => made,
            Err(reason) => {
                self.dom = before;
                let text = format!("{} failed: {}", command.name(), explain(&reason));
                show(&mut self.solid_toast, ToastKind::Error, text.into());
                cx.notify();
                return;
            }
        };
        self.solid_toast = None;
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
    use super::{apply, can, explain, Command};

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
        let mut xs: Vec<f32> = parts
            .iter()
            .map(|&p| match dom.get(p).unwrap().properties().get("CFrame") {
                Some(Variant::CFrame(cf)) => cf.position.x,
                _ => f32::NAN,
            })
            .collect();
        xs.sort_by(f32::total_cmp);
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
        assert_eq!((negate, dom.get(negate).unwrap().class()), (b, "Part"));
        let union = apply(&mut dom, &db, &[a, negate], Command::Union).unwrap()[0];
        let size = match dom.get(union).unwrap().properties().get("size") {
            Some(Variant::Vector3(v)) => v.x,
            _ => f32::NAN,
        };
        assert!((size - 1.0).abs() < 1e-4, "{size}");
    }

    #[test]
    fn a_lone_part_unions_but_does_not_intersect() {
        let db = database();
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let a = part(&mut dom, workspace, 0.0);
        assert!(can(&dom, &db, &[a], Command::Union));
        assert!(!can(&dom, &db, &[a], Command::Intersect));
        assert!(!can(&dom, &db, &[a], Command::Separate));
        let union = apply(&mut dom, &db, &[a], Command::Union).unwrap()[0];
        assert_eq!(dom.get(union).unwrap().class(), "UnionOperation");
    }

    #[test]
    fn nothing_to_intersect_says_so_in_studios_words() {
        let db = database();
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let a = part(&mut dom, workspace, 0.0);
        let b = part(&mut dom, workspace, 10.0);
        let reason = apply(&mut dom, &db, &[a, b], Command::Intersect).unwrap_err();
        assert_eq!(
            explain(&reason),
            "Result was empty, possibly because everything got subtracted away."
        );
        assert!(dom.get(a).is_some() && dom.get(b).is_some());
    }
}
