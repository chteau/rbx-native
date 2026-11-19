//! Network ownership: which peer simulates each unanchored assembly. The
//! server decides (the BasePart network ownership docs and the Ownership
//! golden): a character belongs to its player, `SetNetworkOwner` pins an
//! assembly to a player or the server, and the rest goes, under auto, to
//! the nearest player whose character is within reach, once it has been in
//! the world for a couple of frames. Each client simulates what it owns and
//! the parts only it has; everything else moves where the server says.

use std::collections::{HashMap, HashSet};

use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

/// Roblox's error for the ownership API on an anchored assembly.
pub(crate) const ANCHORED: &str =
    "Network Ownership API cannot be called on Anchored parts or parts welded to Anchored parts.";

/// Studs from a character's root within which its player takes unowned
/// parts when the place doesn't say (`Player.MaximumSimulationRadius`'s
/// default). A player's `SimulationRadius`, capped by its
/// `MaximumSimulationRadius`, is its reach.
pub(crate) const REACH: f64 = 1000.0;

/// Frames a new assembly stays with the server before auto ownership looks
/// at it (Ownership `near_trace`: the server's after 1 frame, the player's
/// by 10).
const SETTLE: u64 = 2;

/// Referents from here up are instances a client made itself; the server
/// never has them. Each client allocates from its own slice.
pub const CLIENT_REFS: u32 = 1 << 30;
const CLIENT_SLICE: u32 = 1 << 26;

/// Where client `index` starts allocating referents.
pub fn client_refs(index: usize) -> u32 {
    CLIENT_REFS + CLIENT_SLICE * index as u32
}

pub(crate) fn client_made(part: Ref) -> bool {
    part.value() >= CLIENT_REFS
}

#[derive(Default)]
pub struct Ownership {
    /// The server's `SetNetworkOwner` pins, per part (`None`: the server).
    explicit: HashMap<Ref, Option<Ref>>,
    /// What auto ownership gave the other parts.
    auto: HashMap<Ref, Ref>,
    /// The frame each part was first seen unanchored.
    seen: HashMap<Ref, u64>,
    /// A client: the parts its player owns, as the server last said.
    pub owned: HashSet<Ref>,
    pub client: bool,
}

impl Ownership {
    /// The player who owns `part`, `None` for the server.
    pub fn owner(&self, part: Ref) -> Option<Ref> {
        match self.explicit.get(&part) {
            Some(pinned) => *pinned,
            None => self.auto.get(&part).copied(),
        }
    }

    pub fn is_auto(&self, part: Ref) -> bool {
        !self.explicit.contains_key(&part)
    }

    /// `SetNetworkOwner(owner)` on the assembly of `parts`, or back to auto.
    pub fn pin(&mut self, parts: &[Ref], owner: Option<Ref>, auto: bool) {
        for part in parts {
            if auto {
                self.explicit.remove(part);
            } else {
                self.explicit.insert(*part, owner);
                self.auto.remove(part);
            }
        }
    }

    /// Whether this peer leaves the assembly of `parts` to another peer.
    pub fn remote(&self, parts: &[Ref]) -> bool {
        if self.client {
            let mine = parts.iter().any(|p| self.owned.contains(p))
                || parts.iter().all(|p| client_made(*p));
            !mine
        } else {
            parts.iter().any(|p| self.owner(*p).is_some())
        }
    }

    /// The server's auto ownership for this frame. `assemblies` are the
    /// unanchored ones; `characters` each player's character parts and
    /// root position; `position` where a part is.
    pub fn assign(
        &mut self,
        frame: u64,
        assemblies: &[Vec<Ref>],
        characters: &[(Ref, HashSet<Ref>, [f64; 3])],
        position: impl Fn(Ref) -> Option<[f64; 3]>,
        reach: impl Fn(Ref) -> f64,
    ) {
        let live: HashSet<Ref> = assemblies.iter().flatten().copied().collect();
        self.seen.retain(|p, _| live.contains(p));
        self.auto.clear();
        for parts in assemblies {
            if parts.iter().any(|p| self.explicit.contains_key(p)) {
                continue;
            }
            let born = parts
                .iter()
                .map(|p| *self.seen.entry(*p).or_insert(frame))
                .max()
                .unwrap_or(frame);
            let owner = characters
                .iter()
                .find(|(_, model, _)| parts.iter().any(|p| model.contains(p)))
                .map(|c| c.0)
                .or_else(|| {
                    if frame < born + SETTLE {
                        return None;
                    }
                    let at = position(parts[0])?;
                    characters
                        .iter()
                        .map(|(player, _, root)| {
                            let d: f64 = (0..3).map(|i| (root[i] - at[i]).powi(2)).sum();
                            (*player, d.sqrt())
                        })
                        .filter(|(player, d)| *d <= reach(*player))
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(player, _)| player)
                });
            if let Some(owner) = owner {
                for part in parts {
                    self.auto.insert(*part, owner);
                }
            }
        }
    }

    /// Forgets the pins of parts that are gone.
    pub fn retain(&mut self, dom: &WeakDom) {
        self.explicit.retain(|p, _| dom.get(*p).is_some());
    }

    /// Every part `player` owns.
    pub fn owned_by(&self, player: Ref) -> HashSet<Ref> {
        let pinned = self
            .explicit
            .iter()
            .filter(|(_, o)| **o == Some(player))
            .map(|(p, _)| *p);
        let auto = self
            .auto
            .iter()
            .filter(|(p, o)| **o == player && !self.explicit.contains_key(p))
            .map(|(p, _)| *p);
        pinned.chain(auto).collect()
    }
}

/// The assembly `part` is in as the DOM stands now (parts joined by rigid
/// joints, as the world builds them), and whether any of it is anchored:
/// what the ownership API works on before the world is next rebuilt.
pub(crate) fn assembly_in_dom(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    part: Ref,
) -> (Vec<Ref>, bool) {
    let mut links: HashMap<Ref, Vec<Ref>> = HashMap::new();
    let mut stack: Vec<Ref> = dom.root_refs().to_vec();
    while let Some(at) = stack.pop() {
        let Some(instance) = dom.get(at) else {
            continue;
        };
        stack.extend(instance.children().iter().copied());
        if let Some(joint) = crate::physics::joint_parts(db, instance) {
            links.entry(joint.0).or_default().push(joint.1);
            links.entry(joint.1).or_default().push(joint.0);
        }
    }
    let mut parts = vec![part];
    let mut seen: HashSet<Ref> = parts.iter().copied().collect();
    let mut i = 0;
    while i < parts.len() {
        for next in links.get(&parts[i]).into_iter().flatten() {
            if dom.get(*next).is_some() && seen.insert(*next) {
                parts.push(*next);
            }
        }
        i += 1;
    }
    let anchored = parts.iter().any(|p| {
        dom.get(*p)
            .and_then(|i| db.stored_or_default(i, "Anchored"))
            .is_some_and(|(_, v)| matches!(v, rbx_dom::Variant::Bool(true)))
    });
    (parts, anchored)
}
