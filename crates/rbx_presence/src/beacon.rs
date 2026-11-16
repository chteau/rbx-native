//! Which presence key each editor with a place open uses. The key lives in
//! the place ([`crate::secret`]), so two editors can end up with different
//! ones: each made its own because the place had none saved yet, or one
//! opened an older copy. They then sit in different rooms and wait for each
//! other forever. Each editor posts a short fingerprint of its room to a
//! topic named after the place, on the same ntfy server the announcements
//! use; seeing another fingerprint there is how that gets said out loud.
//!
//! A message is `{"v":1,"fp":…,"from":…,"client":…,"at":…}`. `fp` hashes the
//! room so the place's topic does not point at the room's; `from` is a random
//! id each editor keeps, so reopening replaces its own earlier beacon instead
//! of looking like somebody else; an empty `fp` is the sender leaving. Nothing is signed
//! (the two sides share no key), so a beacon can only ever explain, never
//! steer. Mirrored by `plugins/team-presence/src/Beacon.luau`.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// How long a beacon counts; each editor posts again well within it.
pub const FRESH_MS: u64 = 25 * 60 * 1000;
/// How often an editor posts its beacon again: rare, since ntfy.sh allows
/// 250 messages a day per address.
pub const EVERY_MS: u64 = 10 * 60 * 1000;

/// The topic for `place_id`.
pub fn topic(place_id: u64) -> String {
    let digest = Sha256::digest(format!("rbx-presence/place/v1/{place_id}").as_bytes());
    format!("rbxnp-place-{}", &hex::encode(digest)[..32])
}

/// The fingerprint of `room`.
pub fn fingerprint(room: &str) -> String {
    let digest = Sha256::digest(format!("rbx-presence/fingerprint/v1/{room}").as_bytes());
    hex::encode(digest)[..16].to_owned()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Beacon {
    v: u32,
    fp: String,
    from: String,
    client: String,
    at: u64,
}

/// The message saying `from` (a `client`, "studio" or "rbx-native") uses
/// the key with fingerprint `fp`, or with `fp` empty, that it left.
pub fn encode(fp: &str, from: &str, client: &str, at: u64) -> String {
    let beacon = Beacon {
        v: 1,
        fp: fp.to_owned(),
        from: from.to_owned(),
        client: client.to_owned(),
        at,
    };
    serde_json::to_string(&beacon).unwrap_or_default()
}

/// The client of the newest current beacon from another sender whose key
/// differs from `fp`, if any. Each sender counts by its newest beacon only,
/// so one that left or switched keys stops counting at once.
pub fn conflict<'a>(
    fp: &str,
    from: &str,
    now: u64,
    texts: impl IntoIterator<Item = &'a str>,
) -> Option<String> {
    let mut newest: std::collections::HashMap<String, Beacon> = std::collections::HashMap::new();
    for beacon in texts
        .into_iter()
        .filter_map(|text| serde_json::from_str::<Beacon>(text).ok())
        .filter(|b| b.v == 1 && b.from != from)
    {
        if newest
            .get(&beacon.from)
            .is_none_or(|old| beacon.at > old.at)
        {
            newest.insert(beacon.from.clone(), beacon);
        }
    }
    newest
        .into_values()
        .filter(|b| {
            let current = b.at <= now + crate::announce::CLOCK_SKEW_MS
                && now.saturating_sub(b.at) <= FRESH_MS;
            current && !b.fp.is_empty() && b.fp != fp
        })
        .max_by_key(|b| b.at)
        .map(|b| b.client)
}

/// Posts this editor's beacon to `place_id`'s topic on `ntfy`.
pub fn publish(ntfy: &str, place_id: u64, fp: &str, from: &str) -> Result<(), String> {
    let body = encode(fp, from, "rbx-native", crate::now_ms());
    crate::announce::post(ntfy, &topic(place_id), &body)
}

/// Reads `place_id`'s topic: the client using a key other than `fp`, if any.
pub fn read(ntfy: &str, place_id: u64, fp: &str, from: &str) -> Result<Option<String>, String> {
    let texts = crate::announce::messages(ntfy, &topic(place_id), "15m")?;
    Ok(conflict(
        fp,
        from,
        crate::now_ms(),
        texts.iter().map(|(text, _)| text.as_str()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    #[test]
    fn another_key_is_a_conflict_and_the_same_key_is_not() {
        let mine = fingerprint("room-a");
        let theirs = fingerprint("room-b");
        let other = encode(&theirs, "s2", "studio", NOW - 1_000);
        let same = encode(&mine, "s3", "studio", NOW - 1_000);
        assert_eq!(
            conflict(&mine, "me", NOW, [other.as_str()]).as_deref(),
            Some("studio")
        );
        assert_eq!(conflict(&mine, "me", NOW, [same.as_str()]), None);
    }

    #[test]
    fn our_own_old_and_departed_beacons_do_not_count() {
        let mine = fingerprint("room-a");
        let theirs = fingerprint("room-b");
        let own = encode(&theirs, "me", "rbx-native", NOW);
        let stale = encode(&theirs, "s2", "studio", NOW - FRESH_MS - 1);
        let joined = encode(&theirs, "s3", "studio", NOW - 5_000);
        let left = encode("", "s3", "studio", NOW - 1_000);
        let texts = [own.as_str(), stale.as_str(), joined.as_str(), left.as_str()];
        assert_eq!(conflict(&mine, "me", NOW, texts), None);
    }

    #[test]
    fn the_topic_and_fingerprint_are_stable() {
        assert_eq!(topic(1).len(), "rbxnp-place-".len() + 32);
        assert_ne!(topic(1), topic(2));
        assert_eq!(fingerprint("r").len(), 16);
    }
}
