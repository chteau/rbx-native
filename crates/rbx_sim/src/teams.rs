//! Teams, our own from the docs and the Teams golden. A player's `Team` is
//! the team (under the Teams service) whose `TeamColor` is the player's,
//! unless the player is `Neutral`: writing `Team` sets `TeamColor` and
//! clears `Neutral`; writing `TeamColor` moves the player to that colour's
//! team, or to none, `Neutral` staying off; destroying a team leaves its
//! players with no team and their colour. A team's `PlayerAdded` and
//! `PlayerRemoved` fire before the write returns. A player joining with
//! auto-assignable teams around goes to the one with fewest players.

use std::collections::HashMap;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, CallReply};
use rbx_reflection::ReflectionDatabase;

use crate::humanoid::{flag, reference};

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    class == "Team" && key == "GetPlayers"
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    class == "Team" && matches!(key, "PlayerAdded" | "PlayerRemoved")
}

fn teams(dom: &WeakDom) -> Vec<Ref> {
    let Some(service) = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Teams"))
    else {
        return Vec::new();
    };
    dom.get(service)
        .map(|i| i.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|t| dom.get(*t).is_some_and(|i| i.class() == "Team"))
        .collect()
}

fn color(dom: &WeakDom, db: &ReflectionDatabase, r: Ref) -> Option<u32> {
    match dom
        .get(r)
        .and_then(|i| db.stored_or_default(i, "TeamColor"))
    {
        Some((_, Variant::BrickColor(c))) => Some(*c),
        _ => None,
    }
}

/// The team `player` is on.
pub(crate) fn team_of(dom: &WeakDom, db: &ReflectionDatabase, player: Ref) -> Option<Ref> {
    if flag(dom, db, player, "Neutral") {
        return None;
    }
    let mine = color(dom, db, player)?;
    teams(dom)
        .into_iter()
        .find(|t| color(dom, db, *t) == Some(mine))
}

/// Each player's team as last seen, to tell who moved.
#[derive(Default)]
pub struct Teams {
    seen: HashMap<Ref, Option<Ref>>,
}

impl Teams {
    /// Brings every player's `Team` up to date: the teams' `PlayerRemoved`
    /// and `PlayerAdded` for those who moved, in that order.
    pub(crate) fn sync(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
    ) -> Vec<(Ref, String, Vec<Arg>)> {
        let mut fire = Vec::new();
        for player in crate::players::players_of(dom) {
            let now = team_of(dom, db, player);
            let before = self.seen.insert(player, now).flatten();
            if reference(dom, player, "Team") != now {
                match now {
                    Some(t) => {
                        let _ = dom.set_property(player, "Team", Variant::Ref(t));
                    }
                    None => {
                        let _ = dom.remove_property(player, "Team");
                    }
                }
            }
            if before != now {
                let me = vec![Arg::Instance(player)];
                if let Some(old) = before.filter(|t| dom.get(*t).is_some()) {
                    fire.push((old, "PlayerRemoved".into(), me.clone()));
                }
                if let Some(new) = now {
                    fire.push((new, "PlayerAdded".into(), me));
                }
            }
        }
        self.seen.retain(|p, _| dom.get(*p).is_some());
        fire
    }

    /// A script wrote `class.key`: a player's `Team` sets its colour and
    /// neutrality; then everyone's team is brought up to date.
    pub(crate) fn written(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        referent: Ref,
        class: &str,
        key: &str,
    ) -> Vec<(Ref, String, Vec<Arg>)> {
        let relevant = matches!(
            (class, key),
            ("Player", "Team" | "TeamColor" | "Neutral") | ("Team", "TeamColor")
        );
        if !relevant {
            return Vec::new();
        }
        if class == "Player" && key == "Team" {
            match reference(dom, referent, "Team") {
                Some(team) => {
                    if let Some(c) = color(dom, db, team) {
                        let _ = dom.set_property(referent, "TeamColor", Variant::BrickColor(c));
                    }
                    let _ = dom.set_property(referent, "Neutral", Variant::Bool(false));
                }
                None => {
                    let _ = dom.set_property(referent, "Neutral", Variant::Bool(true));
                }
            }
        }
        self.sync(dom, db)
    }

    /// A joining player's team: with auto-assignable teams around, the one
    /// with fewest players (the first of those tied).
    pub(crate) fn assign(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, player: Ref) {
        let open: Vec<Ref> = teams(dom)
            .into_iter()
            .filter(|t| flag(dom, db, *t, "AutoAssignable"))
            .collect();
        let players = crate::players::players_of(dom);
        let count = |t: Ref| {
            players
                .iter()
                .filter(|p| **p != player && team_of(dom, db, **p) == Some(t))
                .count()
        };
        let Some(team) = open.iter().copied().min_by_key(|t| count(*t)) else {
            return;
        };
        if let Some(c) = color(dom, db, team) {
            let _ = dom.set_property(player, "TeamColor", Variant::BrickColor(c));
            let _ = dom.set_property(player, "Neutral", Variant::Bool(false));
        }
        self.sync(dom, db);
    }
}

/// `Team:GetPlayers()`.
pub(crate) fn call(dom: &WeakDom, db: &ReflectionDatabase, team: Ref) -> CallReply {
    let on: Vec<Ref> = crate::players::players_of(dom)
        .into_iter()
        .filter(|p| team_of(dom, db, *p) == Some(team))
        .collect();
    CallReply::value(Arg::Instances(on))
}
