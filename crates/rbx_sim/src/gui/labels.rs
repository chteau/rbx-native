//! The name and health over every model with a Humanoid, from the
//! Humanoid's docs and the real client's captures: the label's baseline sits
//! a stud over the head's top (measured at two distances, 0.95 and 0.97
//! stud), white text with a dark edge, the health bar under it.
//!
//! - `DisplayDistanceType` `Viewer` reads the viewer's own
//!   `NameDisplayDistance`/`HealthDisplayDistance`, `Subject` the model's,
//!   `None` shows nothing.
//! - `HealthDisplayType` `DisplayWhenDamaged` shows the bar below full health
//!   only, `AlwaysOn` always, `AlwaysOff` never.
//! - `NameOcclusion` `OccludeAll` hides the label behind any part,
//!   `EnemyOcclusion` only for someone not on the viewer's team.
//! - The viewer's own character gets no label; `DisplayName` replaces the
//!   model's name when set.

use std::collections::HashSet;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::humanoid::{number, reference};

/// Studs from the head's top to the label's baseline.
pub const LABEL_LIFT: f32 = 1.0;

/// One label to draw over a model.
#[derive(Clone, Debug, PartialEq)]
pub struct NameLabel {
    pub text: String,
    /// The baseline's centre in the world.
    pub at: [f32; 3],
    /// Health over MaxHealth when the bar shows.
    pub health: Option<f32>,
}

fn enum_of(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> u32 {
    match dom.get(r).and_then(|i| db.stored_or_default(i, name)) {
        Some((_, Variant::Enum(v))) => *v,
        _ => 0,
    }
}

fn humanoid_of(dom: &WeakDom, model: Ref) -> Option<Ref> {
    dom.get(model)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.class() == "Humanoid"))
}

/// Every Humanoid under `root`, its model and its head (top centre).
fn subjects(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    root: Ref,
    out: &mut Vec<(Ref, Ref, [f32; 3])>,
) {
    let Some(instance) = dom.get(root) else {
        return;
    };
    for child in instance.children() {
        let Some(c) = dom.get(*child) else { continue };
        if c.class() == "Humanoid" {
            let head = instance
                .children()
                .iter()
                .copied()
                .find(|h| dom.get(*h).is_some_and(|i| i.name() == "Head"));
            let top = head.and_then(|h| {
                let cf = crate::humanoid::cframe(dom, db, h)?;
                let size = crate::humanoid::vector(dom, db, h, "Size");
                Some([
                    cf.position.x,
                    cf.position.y + size[1] as f32 / 2.0,
                    cf.position.z,
                ])
            });
            if let Some(top) = top {
                out.push((*child, root, top));
            }
        } else {
            subjects(dom, db, *child, out);
        }
    }
}

fn team(dom: &WeakDom, player: Option<Ref>) -> Option<Ref> {
    player.and_then(|p| reference(dom, p, "Team"))
}

fn descendants(dom: &WeakDom, root: Ref, out: &mut HashSet<Ref>) {
    out.insert(root);
    for child in dom.get(root).map(|i| i.children()).unwrap_or_default() {
        descendants(dom, *child, out);
    }
}

/// The labels `player` sees from `camera`. `blocked(from, to, ignore)` says
/// whether a part not in `ignore` is in the way.
pub fn name_labels(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    player: Ref,
    camera: [f32; 3],
    blocked: &dyn Fn([f32; 3], [f32; 3], HashSet<Ref>) -> bool,
) -> Vec<NameLabel> {
    let Some(workspace) = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
    else {
        return Vec::new();
    };
    let own = reference(dom, player, "Character");
    let viewer = own.and_then(|c| humanoid_of(dom, c));
    let players: Vec<Ref> = crate::players::players_of(dom);
    let my_team = team(dom, Some(player));
    let mut found = Vec::new();
    subjects(dom, db, workspace, &mut found);
    let mut labels = Vec::new();
    for (humanoid, model, top) in found {
        if Some(model) == own {
            continue;
        }
        let distances_from = match enum_of(dom, db, humanoid, "DisplayDistanceType") {
            0 => viewer.unwrap_or(humanoid),
            1 => humanoid,
            _ => continue,
        };
        let at = [top[0], top[1] + LABEL_LIFT, top[2]];
        let distance = f64::from(
            ((at[0] - camera[0]).powi(2)
                + (at[1] - camera[1]).powi(2)
                + (at[2] - camera[2]).powi(2))
            .sqrt(),
        );
        let show_name = distance <= number(dom, db, distances_from, "NameDisplayDistance");
        let (health, max) = (
            number(dom, db, humanoid, "Health"),
            number(dom, db, humanoid, "MaxHealth"),
        );
        let show_health = distance <= number(dom, db, distances_from, "HealthDisplayDistance")
            && match enum_of(dom, db, humanoid, "HealthDisplayType") {
                0 => health < max,
                1 => true,
                _ => false,
            };
        if !show_name && !show_health {
            continue;
        }
        let owner = players
            .iter()
            .copied()
            .find(|p| reference(dom, *p, "Character") == Some(model));
        let occlude = match enum_of(dom, db, humanoid, "NameOcclusion") {
            2 => true,
            1 => owner.is_none() || my_team.is_none() || team(dom, owner) != my_team,
            _ => false,
        };
        let ignore = || {
            let mut parts = HashSet::new();
            for r in [Some(model), own].into_iter().flatten() {
                descendants(dom, r, &mut parts);
            }
            parts
        };
        if occlude && blocked(camera, top, ignore()) {
            continue;
        }
        let display = match dom.get(humanoid).map(|i| i.properties().get("DisplayName")) {
            Some(Some(Variant::String(name))) if !name.is_empty() => name.clone(),
            _ => dom
                .get(model)
                .map(|m| m.name().to_owned())
                .unwrap_or_default(),
        };
        labels.push(NameLabel {
            text: if show_name { display } else { String::new() },
            at,
            health: show_health.then(|| (health / max.max(1e-9)).clamp(0.0, 1.0) as f32),
        });
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::Vector3Data;

    fn world() -> (WeakDom, Ref, Ref) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let players = dom.new_instance("Players", "Players", None);
        let player = dom.new_instance("Player", "Me", Some(players));
        let rig = dom.new_instance("Model", "Rig", Some(workspace));
        let head = dom.new_instance("Part", "Head", Some(rig));
        let mut cf = crate::humanoid::identity();
        cf.position = Vector3Data {
            x: 0.0,
            y: 4.5,
            z: -20.0,
        };
        dom.set_property(head, "CFrame", Variant::CFrame(cf)).ok();
        dom.set_property(
            head,
            "Size",
            Variant::Vector3(Vector3Data {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            }),
        )
        .ok();
        dom.new_instance("Humanoid", "Humanoid", Some(rig));
        (dom, player, rig)
    }

    fn humanoid(dom: &WeakDom, rig: Ref) -> Ref {
        humanoid_of(dom, rig).unwrap()
    }

    fn labels(dom: &WeakDom, player: Ref, camera: [f32; 3]) -> Vec<NameLabel> {
        let db = ReflectionDatabase::shared();
        name_labels(dom, db, player, camera, &|_, _, _| false)
    }

    #[test]
    fn a_rig_shows_its_name_a_stud_over_its_head() {
        let (dom, player, _) = world();
        let got = labels(&dom, player, [0.0, 5.0, 0.0]);
        assert_eq!(
            got,
            vec![NameLabel {
                text: "Rig".into(),
                at: [0.0, 6.0, -20.0],
                health: None
            }]
        );
    }

    #[test]
    fn display_name_distance_damage_and_none() {
        let (mut dom, player, rig) = world();
        let h = humanoid(&dom, rig);
        dom.set_property(h, "DisplayName", Variant::String("Bob".into()))
            .ok();
        dom.set_property(h, "Health", Variant::Float32(25.0)).ok();
        let got = labels(&dom, player, [0.0, 5.0, 0.0]);
        assert_eq!(got[0].text, "Bob");
        assert_eq!(got[0].health, Some(0.25));
        // Past both distances (100 by default): nothing.
        assert!(labels(&dom, player, [0.0, 5.0, 200.0]).is_empty());
        dom.set_property(h, "HealthDisplayType", Variant::Enum(2))
            .ok();
        assert_eq!(labels(&dom, player, [0.0, 5.0, 0.0])[0].health, None);
        dom.set_property(h, "DisplayDistanceType", Variant::Enum(2))
            .ok();
        assert!(labels(&dom, player, [0.0, 5.0, 0.0]).is_empty());
    }

    #[test]
    fn subject_distance_and_occlusion() {
        let (mut dom, player, rig) = world();
        let h = humanoid(&dom, rig);
        dom.set_property(h, "DisplayDistanceType", Variant::Enum(1))
            .ok();
        dom.set_property(h, "NameDisplayDistance", Variant::Float32(10.0))
            .ok();
        assert!(labels(&dom, player, [0.0, 5.0, 0.0]).is_empty());
        dom.set_property(h, "NameDisplayDistance", Variant::Float32(50.0))
            .ok();
        let db = ReflectionDatabase::shared();
        let wall = |_: [f32; 3], _: [f32; 3], _: HashSet<Ref>| true;
        dom.set_property(h, "NameOcclusion", Variant::Enum(2)).ok();
        assert!(name_labels(&dom, db, player, [0.0; 3], &wall).is_empty());
        dom.set_property(h, "NameOcclusion", Variant::Enum(0)).ok();
        assert_eq!(name_labels(&dom, db, player, [0.0; 3], &wall).len(), 1);
    }
}
