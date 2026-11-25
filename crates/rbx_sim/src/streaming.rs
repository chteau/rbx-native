//! Instance streaming (`Workspace.StreamingEnabled`) between a session's
//! server and one client, to the rules the streaming probe measured on a
//! real server (`conformance/goldens/streaming/`). What streams is decided
//! on the server's DOM in units: each `BasePart` under Workspace with the
//! non-part instances inside it (a part inside it is a unit of its own,
//! which brings its parent part along), or a whole Atomic, Persistent or
//! PersistentPerPlayer model or player character. Welded parts stream one
//! by one like the rest. Everything else in Workspace (folders, legacy
//! models, values, scripts) reaches the client at join. A unit is wanted
//! when its box comes within `StreamingTargetRadius` of a focus (the
//! character's root, or `Player.ReplicationFocus`); persistent ones always.

use std::collections::HashSet;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

/// `Workspace.StreamingIntegrityMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Integrity {
    Disabled,
    MinimumRadiusPause,
    PauseOutsideLoadedArea,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Settings {
    pub min: f32,
    pub target: f32,
    /// `StreamOutBehavior.Opportunistic`: beyond the target radius goes
    /// even without memory pressure (Default and LowMemory never stream
    /// out on a machine with memory to spare).
    pub opportunistic: bool,
    pub integrity: Integrity,
}

fn workspace(dom: &WeakDom) -> Option<Ref> {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
}

impl Settings {
    /// The place's streaming settings, `None` with streaming off.
    pub fn read(dom: &WeakDom) -> Option<Settings> {
        let props = dom.get(workspace(dom)?)?.properties();
        if !matches!(props.get("StreamingEnabled"), Some(Variant::Bool(true))) {
            return None;
        }
        let int = |key: &str, default: i32| match props.get(key) {
            Some(Variant::Int32(v)) => *v,
            _ => default,
        };
        let enumeration = |key: &str| match props.get(key) {
            Some(Variant::Enum(v)) => *v,
            _ => 0,
        };
        let min = int("StreamingMinRadius", 64).max(0) as f32;
        let target = (int("StreamingTargetRadius", 1024) as f32).max(min);
        Some(Settings {
            min,
            target,
            opportunistic: enumeration("StreamOutBehavior") == 2,
            integrity: match enumeration("StreamingIntegrityMode") {
                2 => Integrity::MinimumRadiusPause,
                3 => Integrity::PauseOutsideLoadedArea,
                _ => Integrity::Disabled,
            },
        })
    }
}

/// How a unit streams.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// A part, or an Atomic model (or PersistentPerPlayer for others).
    Spatial,
    /// Persistent, or PersistentPerPlayer for this player: sent soon after
    /// the join and never streamed out.
    Persistent,
}

pub(crate) struct Unit {
    pub root: Ref,
    pub kind: Kind,
    /// Its parts' world boxes: (centre, half extents).
    pub boxes: Vec<([f32; 3], [f32; 3])>,
}

fn is_part(db: &ReflectionDatabase, class: &str) -> bool {
    class != "Terrain" && db.is_subclass_of(class, "BasePart")
}

fn mode(dom: &WeakDom, r: Ref) -> u32 {
    match dom
        .get(r)
        .and_then(|i| i.properties().get("ModelStreamingMode"))
    {
        Some(Variant::Enum(m)) => *m,
        _ => 0,
    }
}

/// Whether model `r` streams as one (Atomic, Persistent, PersistentPerPlayer,
/// or a player's character).
fn whole(dom: &WeakDom, r: Ref, characters: &HashSet<Ref>) -> bool {
    dom.get(r).is_some_and(|i| i.class() == "Model")
        && (matches!(mode(dom, r), 1..=3) || characters.contains(&r))
}

/// The unit `r` streams with: `None` when it isn't in Workspace or isn't
/// streamed (it replicates at join like anything outside Workspace). The
/// nearest part at or above `r`, unless a whole model holds it.
pub(crate) fn unit_of(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    r: Ref,
    characters: &HashSet<Ref>,
) -> Option<Ref> {
    let mut part = None;
    let mut model = None;
    let mut at = r;
    loop {
        let instance = dom.get(at)?;
        let parent = dom.parent(at);
        if parent.is_none() {
            return (instance.class() == "Workspace")
                .then_some(model.or(part))
                .flatten();
        }
        if whole(dom, at, characters) {
            model = Some(at);
        } else if part.is_none() && is_part(db, instance.class()) {
            part = Some(at);
        }
        at = parent?;
    }
}

fn position(dom: &WeakDom, r: Ref) -> Option<[f32; 3]> {
    match dom.get(r)?.properties().get("CFrame") {
        Some(Variant::CFrame(c)) => Some([c.position.x, c.position.y, c.position.z]),
        _ => None,
    }
}

/// Part `r`'s world box: its centre and axis-aligned half extents.
fn bounds(dom: &WeakDom, r: Ref) -> Option<([f32; 3], [f32; 3])> {
    let props = dom.get(r)?.properties();
    let Some(Variant::CFrame(c)) = props.get("CFrame") else {
        return None;
    };
    let size = match props.get("Size").or_else(|| props.get("size")) {
        Some(Variant::Vector3(s)) => [s.x, s.y, s.z],
        _ => [4.0, 1.0, 2.0],
    };
    let m = c.rotation;
    let half: [f32; 3] = std::array::from_fn(|row| {
        (0..3)
            .map(|col| (m[row * 3 + col] * size[col] / 2.0).abs())
            .sum()
    });
    Some(([c.position.x, c.position.y, c.position.z], half))
}

/// Every unit under Workspace, for `player` (whose PersistentPerPlayer
/// models are persistent when `persistent_for` lists them).
pub(crate) fn units(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    player: Ref,
    characters: &HashSet<Ref>,
    persistent_for: &dyn Fn(Ref) -> Vec<Ref>,
) -> Vec<Unit> {
    let mut out = Vec::new();
    let Some(ws) = workspace(dom) else {
        return out;
    };
    let mut stack: Vec<Ref> = dom
        .get(ws)
        .map(|i| i.children().to_vec())
        .unwrap_or_default();
    while let Some(r) = stack.pop() {
        let Some(instance) = dom.get(r) else {
            continue;
        };
        if !whole(dom, r, characters) {
            if is_part(db, instance.class()) {
                out.push(Unit {
                    root: r,
                    kind: Kind::Spatial,
                    boxes: bounds(dom, r).into_iter().collect(),
                });
            }
            stack.extend(instance.children().iter().copied());
            continue;
        }
        let kind = match mode(dom, r) {
            2 => Kind::Persistent,
            3 if persistent_for(r).contains(&player) => Kind::Persistent,
            _ => Kind::Spatial,
        };
        let mut boxes = Vec::new();
        let mut inner = vec![r];
        while let Some(at) = inner.pop() {
            let Some(i) = dom.get(at) else {
                continue;
            };
            if is_part(db, i.class()) {
                boxes.extend(bounds(dom, at));
            }
            inner.extend(i.children().iter().copied());
        }
        out.push(Unit {
            root: r,
            kind,
            boxes,
        });
    }
    out
}

/// The world streams in cubes of this many studs, aligned on the origin;
/// a part is in every cube its box touches. A cube is within a radius of
/// a focus when it is on every axis (not as the crow flies): with the
/// focus a stud or two either side of a cube edge, that's what makes
/// `P1088` and the diagonal `D1536` come in together and `P72` or `N128`
/// stay out of the minimum radius (the streaming goldens).
pub(crate) const CELL: f32 = 64.0;

impl Unit {
    /// How far the cubes its parts touch are from `focus`, on the axis
    /// where that's furthest.
    pub fn reach(&self, focus: [f32; 3]) -> f32 {
        self.boxes
            .iter()
            .map(|(c, h)| {
                (0..3)
                    .map(|i| {
                        let lo = ((c[i] - h[i]) / CELL).floor() * CELL;
                        let hi = ((c[i] + h[i]) / CELL).floor() * CELL + CELL;
                        (lo - focus[i]).max(focus[i] - hi).max(0.0)
                    })
                    .fold(0.0, f32::max)
            })
            .fold(f32::INFINITY, f32::min)
    }
}

/// Where `player` streams around on the server: `ReplicationFocus`, else
/// their character's root part, and the parts `AddReplicationFocus` added.
pub(crate) fn foci(dom: &WeakDom, player: Ref, added: &[Ref]) -> Vec<[f32; 3]> {
    let main = match reference(dom, player, "ReplicationFocus") {
        Some(focus) => position(dom, focus),
        None => character_root(dom, player),
    };
    main.into_iter()
        .chain(added.iter().filter_map(|p| position(dom, *p)))
        .collect()
}

fn reference(dom: &WeakDom, r: Ref, key: &str) -> Option<Ref> {
    match dom.get(r)?.properties().get(key) {
        Some(Variant::Ref(t)) if dom.get(*t).is_some() => Some(*t),
        _ => None,
    }
}

/// Where `player`'s character's root part is on the server: what the
/// integrity pause watches (moving `ReplicationFocus` away never paused).
pub(crate) fn character_root(dom: &WeakDom, player: Ref) -> Option<[f32; 3]> {
    let reference = |r: Ref, key: &str| reference(dom, r, key);
    let character = reference(player, "Character")?;
    let root = reference(character, "PrimaryPart").or_else(|| {
        dom.get(character)?
            .children()
            .iter()
            .copied()
            .find(|c| dom.get(*c).is_some_and(|i| i.name() == "HumanoidRootPart"))
    })?;
    position(dom, root)
}

/// Every player's character on the server.
pub(crate) fn characters(dom: &WeakDom) -> HashSet<Ref> {
    crate::players::players_of(dom)
        .into_iter()
        .filter_map(|p| match dom.get(p)?.properties().get("Character") {
            Some(Variant::Ref(c)) => Some(*c),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(at: [f32; 3]) -> Unit {
        Unit {
            root: Ref::new(1),
            kind: Kind::Spatial,
            boxes: vec![(at, [1.0, 1.0, 1.0])],
        }
    }

    #[test]
    fn reach_counts_whole_cubes_on_the_furthest_axis() {
        let focus = [-3.0, 3.0, -2.0];
        // P64's box touches the focus's cube column; P72's doesn't.
        assert!(unit([64.0, 5.0, 0.0]).reach(focus) <= 64.0);
        assert!(unit([72.0, 5.0, 0.0]).reach(focus) > 64.0);
        // N128's box reaches into the cube 61 studs away.
        assert!(unit([-128.0, 5.0, 0.0]).reach(focus) <= 64.0);
        // On the diagonal it's the furthest axis that counts.
        let at_zero = [0.0, 8.0, 0.0];
        assert_eq!(unit([1086.0, 5.0, 1086.0]).reach(at_zero), 1024.0);
        assert_eq!(unit([1088.0, 5.0, 0.0]).reach(at_zero), 1024.0);
        assert!(unit([1152.0, 5.0, 0.0]).reach(at_zero) > 1024.0);
    }
}
