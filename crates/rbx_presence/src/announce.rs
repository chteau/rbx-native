//! Where the room's host is: the public URL of the relay an rbx-native
//! user is running ([`crate::host`]) behind a Cloudflare quick tunnel, which
//! changes every time it starts. The host posts it to the room's announce
//! topic on an ntfy server (a handful of messages a day, far under ntfy.sh's
//! limits); everyone else reads the newest one and connects there.
//!
//! A message is `{"v":1,"url":…,"at":…,"sig":…}`, signed with the place key
//! like everything else, so only collaborators can announce (or read where
//! the room is: the topic name derives from the room id). An empty `url` is
//! the host saying goodbye. Mirrored by `plugins/team-presence/src/Announce.luau`.

use serde::{Deserialize, Serialize};

use crate::auth;

/// An announcement older than this is ignored (ntfy.sh keeps messages 12 h;
/// a live host re-announces well within it).
pub const MAX_AGE_MS: u64 = 12 * 60 * 60 * 1000;
/// How often a host re-announces itself.
pub const REANNOUNCE_MS: u64 = 3 * 60 * 60 * 1000;
/// An announcement may be stamped this far ahead of the reader's clock.
pub const CLOCK_SKEW_MS: u64 = 60_000;

/// The announce topic of `room`: next to its presence topic, unguessable
/// without the room id.
pub fn topic(room: &str) -> String {
    format!("{}-host", crate::relay::ntfy_topic(room))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Announcement {
    pub v: u32,
    pub url: String,
    pub at: u64,
    pub sig: String,
}

fn canonical(room: &str, url: &str, at: u64) -> String {
    format!("rbx-presence/announce/v1\n{room}\n{url}\n{at}")
}

/// Whether `url` may be connected to: a public HTTPS address, or this
/// machine (tests, and a host's own address).
pub fn allowed_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://127.0.0.1:")
}

/// The signed message announcing `url` (or, empty, the host leaving).
pub fn encode(key: &str, room: &str, url: &str, at: u64) -> String {
    let message = Announcement {
        v: 1,
        url: url.to_owned(),
        at,
        sig: auth::sign(key, &canonical(room, url, at)),
    };
    serde_json::to_string(&message).unwrap_or_default()
}

/// `text` if it is a genuine, current announcement for `room`.
pub fn verify(key: &str, room: &str, now: u64, text: &str) -> Option<Announcement> {
    let message: Announcement = serde_json::from_str(text).ok()?;
    let expected = auth::sign(key, &canonical(room, &message.url, message.at));
    let fresh = message.at <= now + CLOCK_SKEW_MS && now.saturating_sub(message.at) <= MAX_AGE_MS;
    let url_ok = message.url.is_empty() || allowed_url(&message.url);
    (message.v == 1 && expected == message.sig && fresh && url_ok).then_some(message)
}

/// The host's URL from a run of announcements (oldest first or in any
/// order): the newest genuine one, `None` when there is none or the newest
/// says the host left.
pub fn newest<'a>(
    key: &str,
    room: &str,
    now: u64,
    texts: impl IntoIterator<Item = &'a str>,
) -> Option<Announcement> {
    texts
        .into_iter()
        .filter_map(|text| verify(key, room, now, text))
        .max_by_key(|message| message.at)
        .filter(|message| !message.url.is_empty())
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("rbx-native/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .into()
}

/// Posts an announcement to `ntfy` (`https://host[/prefix]`).
pub fn publish(ntfy: &str, key: &str, room: &str, url: &str) -> Result<(), String> {
    let base = ntfy.trim_end_matches('/');
    let body = encode(key, room, url, crate::now_ms());
    let mut resp = agent()
        .post(format!("{base}/{}", topic(room)))
        .header("Content-Type", "text/plain")
        .send(body.as_str())
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let _ = resp.body_mut().read_to_string();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(format!("announce: HTTP {status}"))
    }
}

/// The host currently announced on `ntfy`, if any.
pub fn find(ntfy: &str, key: &str, room: &str) -> Result<Option<Announcement>, String> {
    let base = ntfy.trim_end_matches('/');
    let mut resp = agent()
        .get(format!("{base}/{}/json", topic(room)))
        .query("poll", "1")
        .query("since", "12h")
        .call()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(format!("announce: HTTP {status}"));
    }
    let messages: Vec<String> = body
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|v| v.get("event").and_then(|e| e.as_str()) == Some("message"))
        .filter_map(|v| v.get("message").and_then(|m| m.as_str()).map(str::to_owned))
        .collect();
    Ok(newest(
        key,
        room,
        crate::now_ms(),
        messages.iter().map(String::as_str),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "k3y";
    const ROOM: &str = "0123456789abcdef0123456789abcdef";
    const NOW: u64 = 1_800_000_000_000;

    #[test]
    fn the_newest_genuine_announcement_wins() {
        let old = encode(KEY, ROOM, "https://old.trycloudflare.com", NOW - 60_000);
        let new = encode(KEY, ROOM, "https://new.trycloudflare.com", NOW - 1_000);
        let found = newest(KEY, ROOM, NOW, [new.as_str(), old.as_str()]).unwrap();
        assert_eq!(found.url, "https://new.trycloudflare.com");
    }

    #[test]
    fn a_goodbye_means_no_host() {
        let hello = encode(KEY, ROOM, "https://a.trycloudflare.com", NOW - 60_000);
        let bye = encode(KEY, ROOM, "", NOW - 1_000);
        assert!(newest(KEY, ROOM, NOW, [hello.as_str(), bye.as_str()]).is_none());
    }

    #[test]
    fn forged_stale_future_and_odd_urls_are_ignored() {
        let forged = encode("other", ROOM, "https://evil.example", NOW);
        let wrong_room = encode(
            KEY,
            "ffffffffffffffffffffffffffffffff",
            "https://x.example",
            NOW,
        );
        let stale = encode(KEY, ROOM, "https://x.example", NOW - MAX_AGE_MS - 1);
        let future = encode(KEY, ROOM, "https://x.example", NOW + CLOCK_SKEW_MS + 1);
        let plain_http = encode(KEY, ROOM, "http://evil.example", NOW);
        for text in [forged, wrong_room, stale, future, plain_http] {
            assert!(verify(KEY, ROOM, NOW, &text).is_none(), "{text}");
        }
        let mut tampered: Announcement =
            serde_json::from_str(&encode(KEY, ROOM, "https://a.example", NOW)).unwrap();
        tampered.url = "https://b.example".into();
        assert!(verify(KEY, ROOM, NOW, &serde_json::to_string(&tampered).unwrap()).is_none());
    }
}
