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
//!
//! It also carries `host` ([`host_id`]), which the host also answers its
//! loopback health check with, so Studio on the same computer knows an
//! address announced from this computer (left behind by an RbxNative that
//! was killed, so said no goodbye) from another collaborator's. It sits outside
//! the signature: readers that check the v1 signature keep working, and a
//! forged one can only make a Studio skip an address it would not use.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::auth;

/// An announcement older than this is ignored (ntfy.sh keeps messages 12 h;
/// a live host re-announces well within it).
pub const MAX_AGE_MS: u64 = 12 * 60 * 60 * 1000;
/// How often a host re-announces itself.
pub const REANNOUNCE_MS: u64 = 3 * 60 * 60 * 1000;
/// An announcement may be stamped this far ahead of the reader's clock, or
/// this far from when the ntfy server received it.
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub host: String,
}

fn canonical(room: &str, url: &str, at: u64) -> String {
    format!("rbx-presence/announce/v1\n{room}\n{url}\n{at}")
}

/// Whether `url` may be connected to: a public HTTPS address, or this
/// machine (tests, and a host's own address).
pub fn allowed_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://127.0.0.1:")
}

/// The signed message announcing `url` (or, empty, the host leaving) for
/// the host with id `host`.
pub fn encode(key: &str, room: &str, url: &str, host: &str, at: u64) -> String {
    let message = Announcement {
        v: 1,
        url: url.to_owned(),
        at,
        sig: auth::sign(key, &canonical(room, url, at)),
        host: host.to_owned(),
    };
    serde_json::to_string(&message).unwrap_or_default()
}

/// The loopback port a host on this computer serves `room` on, so Studio
/// on the same computer reaches it at `127.0.0.1`, an address it asks about
/// once, rather than through the tunnel, whose address is new every time
/// RbxNative starts. Derived from the room id (hex), below the ports the
/// system hands out by itself.
pub fn loopback_port(room: &str) -> u16 {
    let word = room
        .get(..4)
        .and_then(|hex| u16::from_str_radix(hex, 16).ok())
        .unwrap_or(0);
    40960 + word % 8192
}

/// The `host` id of hosts started by the rbx-native install `install` (its
/// beacon sender id, hashed so the two don't link the place's topic to the
/// room's): the same for every room and every start, so Studio on that
/// computer knows an address announced from it however many restarts ago,
/// for one that only the loopback port can stand in for.
pub fn host_id(install: &str) -> String {
    let digest = Sha256::digest(format!("rbx-presence/host/v1/{install}").as_bytes());
    hex::encode(digest)[..16].to_owned()
}

/// A post ntfy didn't take. `retry_after` is set when it refused for the
/// rate limit (ntfy.sh's 429): how long it asked to wait, or `ZERO` when it
/// didn't say.
#[derive(Debug)]
pub struct PostError {
    pub why: String,
    pub retry_after: Option<Duration>,
}

impl std::fmt::Display for PostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.why)
    }
}

/// `text` if it is a genuine, current announcement for `room`. `received`
/// is when the ntfy server got it (unix ms): a message stamped hours away
/// from that comes from a machine whose clock is wrong (a VM or dual boot
/// reading the hardware clock as the wrong time zone), and would otherwise
/// turn "newest" hours later, long after its host is gone.
pub fn verify(key: &str, room: &str, now: u64, received: u64, text: &str) -> Option<Announcement> {
    let message: Announcement = serde_json::from_str(text).ok()?;
    let expected = auth::sign(key, &canonical(room, &message.url, message.at));
    let fresh = message.at <= now + CLOCK_SKEW_MS
        && now.saturating_sub(message.at) <= MAX_AGE_MS
        && message.at.abs_diff(received) <= CLOCK_SKEW_MS;
    let url_ok = message.url.is_empty() || allowed_url(&message.url);
    (message.v == 1 && expected == message.sig && fresh && url_ok).then_some(message)
}

/// The host's URL from a run of announcements, each with when the server
/// received it (oldest first or in any order): the newest genuine one,
/// `None` when there is none or the newest says the host left.
pub fn newest<'a>(
    key: &str,
    room: &str,
    now: u64,
    messages: impl IntoIterator<Item = (&'a str, u64)>,
) -> Option<Announcement> {
    messages
        .into_iter()
        .filter_map(|(text, received)| verify(key, room, now, received, text))
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
pub fn publish(ntfy: &str, key: &str, room: &str, url: &str, host: &str) -> Result<(), PostError> {
    post(
        ntfy,
        &topic(room),
        &encode(key, room, url, host, crate::now_ms()),
    )
}

/// Posts `body` to `topic` on `ntfy`.
pub(crate) fn post(ntfy: &str, topic: &str, body: &str) -> Result<(), PostError> {
    let base = ntfy.trim_end_matches('/');
    let mut resp = agent()
        .post(format!("{base}/{topic}"))
        .header("Content-Type", "text/plain")
        .send(body)
        .map_err(|e| PostError {
            why: e.to_string(),
            retry_after: None,
        })?;
    let status = resp.status().as_u16();
    let retry_after = (status == 429).then(|| {
        let asked = resp.headers().get("retry-after");
        let secs = asked.and_then(|v| v.to_str().ok()?.trim().parse().ok());
        Duration::from_secs(secs.unwrap_or(0))
    });
    let _ = resp.body_mut().read_to_string();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(PostError {
            why: format!("announce: HTTP {status}"),
            retry_after,
        })
    }
}

/// The host currently announced on `ntfy`, if any.
pub fn find(ntfy: &str, key: &str, room: &str) -> Result<Option<Announcement>, String> {
    let messages = messages(ntfy, &topic(room), "12h")?;
    Ok(newest(
        key,
        room,
        crate::now_ms(),
        messages.iter().map(|(text, at)| (text.as_str(), *at)),
    ))
}

/// The messages `topic` on `ntfy` still holds from the last `since`: each
/// text with when the server received it (unix ms).
pub(crate) fn messages(ntfy: &str, topic: &str, since: &str) -> Result<Vec<(String, u64)>, String> {
    let base = ntfy.trim_end_matches('/');
    let mut resp = agent()
        .get(format!("{base}/{topic}/json"))
        .query("poll", "1")
        .query("since", since)
        .call()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let body = resp.body_mut().read_to_string().unwrap_or_default();
    if !(200..300).contains(&status) {
        return Err(format!("announce: HTTP {status}"));
    }
    Ok(body
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|v| v.get("event").and_then(|e| e.as_str()) == Some("message"))
        .filter_map(|v| {
            let text = v.get("message")?.as_str()?.to_owned();
            Some((text, v.get("time")?.as_u64()? * 1000))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "k3y";
    const ROOM: &str = "0123456789abcdef0123456789abcdef";
    const NOW: u64 = 1_800_000_000_000;

    #[test]
    fn the_loopback_port_comes_from_the_room() {
        assert_eq!(loopback_port(ROOM), 40960 + 0x0123);
        assert_eq!(loopback_port("ffff"), 40960 + 8191);
        assert_eq!(loopback_port(""), 40960);
    }

    #[test]
    fn the_newest_genuine_announcement_wins() {
        let old = encode(KEY, ROOM, "https://old.trycloudflare.com", "", NOW - 60_000);
        let new = encode(KEY, ROOM, "https://new.trycloudflare.com", "", NOW - 1_000);
        let found = newest(KEY, ROOM, NOW, [(new.as_str(), NOW), (old.as_str(), NOW)]).unwrap();
        assert_eq!(found.url, "https://new.trycloudflare.com");
    }

    #[test]
    fn a_goodbye_means_no_host() {
        let hello = encode(KEY, ROOM, "https://a.trycloudflare.com", "", NOW - 60_000);
        let bye = encode(KEY, ROOM, "", "", NOW - 1_000);
        assert!(newest(KEY, ROOM, NOW, [(hello.as_str(), NOW), (bye.as_str(), NOW)]).is_none());
    }

    #[test]
    fn a_wrong_clock_announcement_never_becomes_the_newest() {
        let live = encode(
            KEY,
            ROOM,
            "https://live.trycloudflare.com",
            "",
            NOW - 60_000,
        );
        // Sent nine hours ago from a clock nine hours fast: by its own stamp
        // it is from just now, newer than the live host's.
        let skewed = encode(KEY, ROOM, "https://gone.trycloudflare.com", "", NOW - 1_000);
        let sent = NOW - 9 * 60 * 60 * 1000;
        let found = newest(
            KEY,
            ROOM,
            NOW,
            [(live.as_str(), NOW - 59_000), (skewed.as_str(), sent)],
        )
        .unwrap();
        assert_eq!(found.url, "https://live.trycloudflare.com");
    }

    #[test]
    fn forged_stale_future_and_odd_urls_are_ignored() {
        let forged = encode("other", ROOM, "https://evil.example", "", NOW);
        let wrong_room = encode(
            KEY,
            "ffffffffffffffffffffffffffffffff",
            "https://x.example",
            "",
            NOW,
        );
        let stale = encode(KEY, ROOM, "https://x.example", "", NOW - MAX_AGE_MS - 1);
        let future = encode(KEY, ROOM, "https://x.example", "", NOW + CLOCK_SKEW_MS + 1);
        let plain_http = encode(KEY, ROOM, "http://evil.example", "", NOW);
        for text in [forged, wrong_room, stale, future, plain_http] {
            assert!(verify(KEY, ROOM, NOW, NOW, &text).is_none(), "{text}");
        }
        let mut tampered: Announcement =
            serde_json::from_str(&encode(KEY, ROOM, "https://a.example", "", NOW)).unwrap();
        tampered.url = "https://b.example".into();
        let tampered = serde_json::to_string(&tampered).unwrap();
        assert!(verify(KEY, ROOM, NOW, NOW, &tampered).is_none());
    }

    #[test]
    fn the_host_id_rides_along_outside_the_signature() {
        let text = encode(KEY, ROOM, "https://a.example", "h1", NOW);
        assert_eq!(host_id("abc"), host_id("abc"));
        assert_ne!(host_id("abc"), host_id("abd"));
        assert_eq!(verify(KEY, ROOM, NOW, NOW, &text).unwrap().host, "h1");
        // One from a host that predates the id still verifies.
        let mut bare: serde_json::Value = serde_json::from_str(&text).unwrap();
        bare.as_object_mut().unwrap().remove("host");
        let bare = verify(KEY, ROOM, NOW, NOW, &bare.to_string()).unwrap();
        assert_eq!(bare.host, "");
        assert!(!encode(KEY, ROOM, "", "", NOW).contains("host"));
    }
}
