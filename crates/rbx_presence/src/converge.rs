//! Getting every editor on a place onto one presence key.
//!
//! The key lives in the place ([`crate::secret`]), so two editors that each
//! opened a copy without one make their own and sit in different rooms. The
//! secret never travels over anything public (the beacon only carries a
//! fingerprint of the room), so the one channel both sides trust is the
//! place saved on Roblox: whoever can read it is already a collaborator.
//!
//! The rule is that the key in the saved place wins. Studio cannot read the
//! saved place, but rbx-native can (it has the Open Cloud key), so rbx-native
//! is the side that moves: when it opens a place with no key it takes the
//! saved one before making its own, and when the beacon says another editor
//! uses a different key it reads the saved place again and switches to the
//! key there. What is left for a person is one step on the other side: save
//! the place from Studio, or reopen an older copy.

/// What to do once the beacon says another editor uses a different key,
/// given the key in the place as saved on Roblox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// The saved place has another key than ours: switch to it.
    Adopt(String),
    /// We already use the saved key; the other editor has an older copy and
    /// has to reopen it (another rbx-native does that on its own).
    PeerReopens,
    /// The saved place has no key yet; the other editor has to save the
    /// place so its key reaches Roblox, and we pick it up from there.
    PeerSaves,
}

pub fn on_conflict(ours: &str, saved: Option<&str>) -> Next {
    match saved {
        Some(saved) if saved != ours => Next::Adopt(saved.to_owned()),
        Some(_) => Next::PeerReopens,
        None => Next::PeerSaves,
    }
}

/// The beacon fingerprint an editor connected with `secret` posts, so a
/// key can be matched against a beacon without anyone revealing it.
pub fn fingerprint(secret: &str, place_id: u64) -> String {
    let key = crate::auth::derive_key(secret, place_id);
    crate::beacon::fingerprint(&crate::auth::room_id(&key))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OURS: &str = "aa";
    const SAVED: &str = "bb";

    #[test]
    fn the_saved_key_wins_over_ours() {
        assert_eq!(on_conflict(OURS, Some(SAVED)), Next::Adopt(SAVED.into()));
    }

    #[test]
    fn holding_the_saved_key_leaves_the_move_to_the_other_side() {
        assert_eq!(on_conflict(OURS, Some(OURS)), Next::PeerReopens);
        assert_eq!(on_conflict(OURS, None), Next::PeerSaves);
    }

    #[test]
    fn adopting_the_saved_key_lands_in_the_peers_room() {
        let saved = crate::secret::generate();
        let ours = crate::secret::generate();
        let peer = fingerprint(&saved, 104_688_317_714_596);
        let Next::Adopt(next) = on_conflict(&ours, Some(&saved)) else {
            panic!("expected to adopt the saved key");
        };
        assert_eq!(fingerprint(&next, 104_688_317_714_596), peer);
        assert_ne!(fingerprint(&ours, 104_688_317_714_596), peer);
    }
}
