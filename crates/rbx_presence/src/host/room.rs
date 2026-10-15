//! One room's presence, the Worker's `relay/src/room.ts` in Rust: who is
//! in it (one entry per user per editor), a token bucket and replay guard
//! per user, and the value-less edit journal. Pure, so it is unit-tested.

use std::collections::HashMap;

use crate::catchup::JournalEntry;
use crate::schema::{Client, Snapshot, UserState, EXPIRY_MS};

/// Most senders in one room.
pub const MAX_USERS: usize = 32;
/// A signed request's time may differ from the host's clock by this much.
pub const CLOCK_SKEW_MS: u64 = 60_000;
/// Token bucket per user: burst size and refill per second.
pub const RATE_BURST: f64 = 8.0;
pub const RATE_PER_SECOND: f64 = 4.0;
/// Most journal entries kept.
pub const JOURNAL_CAP: usize = 256;
/// Journal entries older than this are dropped.
pub const JOURNAL_TTL_MS: u64 = 12 * 60 * 60 * 1000;

/// Why a request is turned away: an HTTP status and the Worker's word for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal {
    pub status: u16,
    pub error: &'static str,
}

impl Refusal {
    pub const fn new(status: u16, error: &'static str) -> Self {
        Refusal { status, error }
    }
}

struct Bucket {
    tokens: f64,
    at: u64,
}

#[derive(Default)]
pub struct Room {
    pub cursor: u64,
    users: HashMap<(u64, Client), UserState>,
    buckets: HashMap<u64, Bucket>,
    last_ts: HashMap<u64, u64>,
    journal: Vec<JournalEntry>,
}

impl Room {
    /// Stores `state` (stamping `seen`), journals its edits once each, and
    /// advances the cursor; refused when the user is over their rate or the
    /// room is full.
    pub fn put(&mut self, state: UserState, now: u64) -> Result<(), Refusal> {
        if !self.take(state.user_id, now) {
            return Err(Refusal::new(429, "rate"));
        }
        let key = (state.user_id, state.client);
        if !self.users.contains_key(&key) && self.users.len() >= MAX_USERS {
            return Err(Refusal::new(403, "full"));
        }
        // Clients re-send their recent edits on every heartbeat, so the
        // journal keys on the client's own edit time and skips one it
        // already holds. The value is never stored.
        for edit in &state.edits {
            let seen = self.journal.iter().any(|e| {
                e.user_id == state.user_id
                    && e.at == edit.at
                    && e.path == edit.path
                    && e.property == edit.property
            });
            if !seen {
                self.journal.push(JournalEntry {
                    user_id: state.user_id,
                    name: state.name.clone(),
                    path: edit.path.clone(),
                    property: edit.property.clone(),
                    at: edit.at,
                });
            }
        }
        self.journal
            .retain(|e| e.at >= now.saturating_sub(JOURNAL_TTL_MS));
        if self.journal.len() > JOURNAL_CAP {
            let drop = self.journal.len() - JOURNAL_CAP;
            self.journal.drain(..drop);
        }
        self.users.insert(key, UserState { seen: now, ..state });
        self.cursor += 1;
        Ok(())
    }

    /// Journal entries after `since` and within the TTL, oldest first.
    pub fn journal_since(&self, since: u64, now: u64) -> Vec<JournalEntry> {
        let mut out: Vec<JournalEntry> = self
            .journal
            .iter()
            .filter(|e| e.at > since && e.at >= now.saturating_sub(JOURNAL_TTL_MS))
            .cloned()
            .collect();
        out.sort_by_key(|e| e.at);
        out
    }

    /// Drops one editor's state; the user's rate and replay state go with
    /// their last one.
    pub fn remove(&mut self, user_id: u64, client: Client) -> bool {
        let had = self.users.remove(&(user_id, client)).is_some();
        self.forget_if_gone(user_id);
        if had {
            self.cursor += 1;
        }
        had
    }

    /// Drops everyone not heard from within `EXPIRY_MS`.
    pub fn expire(&mut self, now: u64) -> bool {
        let stale: Vec<(u64, Client)> = self
            .users
            .iter()
            .filter(|(_, u)| u.seen < now.saturating_sub(EXPIRY_MS))
            .map(|(k, _)| *k)
            .collect();
        for key in &stale {
            self.users.remove(key);
            self.forget_if_gone(key.0);
        }
        if !stale.is_empty() {
            self.cursor += 1;
        }
        !stale.is_empty()
    }

    pub fn snapshot(&self) -> Snapshot {
        let mut users: Vec<UserState> = self.users.values().cloned().collect();
        users.sort_by_key(|u| (u.user_id, u.client as u8));
        Snapshot {
            cursor: self.cursor,
            users,
        }
    }

    /// Clock skew always; with `monotonic`, also a replay guard: a write's
    /// time must be later than the user's last one.
    pub fn check_time(
        &mut self,
        user_id: u64,
        ts: u64,
        now: u64,
        monotonic: bool,
    ) -> Result<(), Refusal> {
        if now.abs_diff(ts) > CLOCK_SKEW_MS {
            return Err(Refusal::new(401, "time"));
        }
        if monotonic {
            if self.last_ts.get(&user_id).is_some_and(|&last| ts <= last) {
                return Err(Refusal::new(401, "replay"));
            }
            self.last_ts.insert(user_id, ts);
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.users.is_empty()
    }

    fn forget_if_gone(&mut self, user_id: u64) {
        if !self.users.keys().any(|(id, _)| *id == user_id) {
            self.buckets.remove(&user_id);
            self.last_ts.remove(&user_id);
        }
    }

    fn take(&mut self, user_id: u64, now: u64) -> bool {
        let bucket = self.buckets.entry(user_id).or_insert(Bucket {
            tokens: RATE_BURST,
            at: now,
        });
        let elapsed = now.saturating_sub(bucket.at) as f64 / 1000.0;
        bucket.tokens = (bucket.tokens + elapsed * RATE_PER_SECOND).min(RATE_BURST);
        bucket.at = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Edit, SCHEMA_VERSION};

    fn user(user_id: u64) -> UserState {
        UserState {
            v: SCHEMA_VERSION,
            user_id,
            name: format!("u{user_id}"),
            color: "#e8590c".into(),
            client: Client::Studio,
            camera: None,
            selection: Vec::new(),
            script: None,
            edits: Vec::new(),
            seen: 0,
        }
    }

    fn with_edits(user_id: u64, n: u64, base_at: u64) -> UserState {
        UserState {
            edits: (0..n)
                .map(|i| Edit {
                    path: format!("Workspace.P{i}"),
                    property: "Size".into(),
                    value: "secret".into(),
                    at: base_at + i,
                })
                .collect(),
            ..user(user_id)
        }
    }

    #[test]
    fn stores_users_stamps_seen_and_advances_the_cursor() {
        let mut room = Room::default();
        assert!(room.is_empty());
        room.put(user(1), 1000).unwrap();
        let snap = room.snapshot();
        assert_eq!(snap.cursor, 1);
        assert_eq!(snap.users.len(), 1);
        assert_eq!(snap.users[0].seen, 1000);
    }

    #[test]
    fn journals_each_edit_once_and_never_its_value() {
        let mut room = Room::default();
        room.put(with_edits(7, 2, 5), 1000).unwrap();
        room.put(with_edits(7, 2, 5), 6000).unwrap();
        let journal = room.journal_since(0, 6000);
        assert_eq!(journal.len(), 2);
        assert!(!serde_json::to_string(&journal).unwrap().contains("secret"));
        assert_eq!(room.journal_since(5, 6000).len(), 1);
    }

    #[test]
    fn the_journal_keeps_its_cap_and_ttl() {
        let mut room = Room::default();
        // 14 heartbeats of 20 fresh edits: 280 entries, past the cap.
        for beat in 0..14u64 {
            room.buckets.clear();
            room.put(with_edits(1, 20, beat * 20 + 1), 1000).unwrap();
        }
        let journal = room.journal_since(0, 1000);
        assert_eq!(journal.len(), JOURNAL_CAP);
        // The oldest went first.
        assert_eq!(journal[0].at, 280 - JOURNAL_CAP as u64 + 1);
        let later = JOURNAL_TTL_MS + 100_000;
        room.put(user(2), later).unwrap();
        assert!(room.journal_since(0, later).is_empty());
    }

    #[test]
    fn rate_limits_after_the_burst_and_refills() {
        let mut room = Room::default();
        for _ in 0..RATE_BURST as usize {
            room.put(user(1), 0).unwrap();
        }
        assert_eq!(room.put(user(1), 0), Err(Refusal::new(429, "rate")));
        room.put(user(2), 0).unwrap();
        room.put(user(1), 1000).unwrap();
    }

    #[test]
    fn refuses_a_new_sender_when_full() {
        let mut room = Room::default();
        for i in 0..MAX_USERS as u64 {
            room.put(user(i), 0).unwrap();
        }
        assert_eq!(room.put(user(999), 0), Err(Refusal::new(403, "full")));
        room.put(user(0), 1000).unwrap();
    }

    #[test]
    fn one_user_in_both_editors_is_two_senders() {
        let mut room = Room::default();
        room.put(user(1), 0).unwrap();
        room.put(
            UserState {
                client: Client::RbxNative,
                ..user(1)
            },
            100,
        )
        .unwrap();
        assert_eq!(room.snapshot().users.len(), 2);
        assert!(room.remove(1, Client::RbxNative));
        assert_eq!(room.snapshot().users[0].client, Client::Studio);
    }

    #[test]
    fn expires_the_silent_and_removes_bump_the_cursor_once() {
        let mut room = Room::default();
        room.put(user(1), 0).unwrap();
        room.put(user(2), EXPIRY_MS).unwrap();
        let before = room.cursor;
        assert!(!room.expire(EXPIRY_MS));
        assert!(room.expire(EXPIRY_MS + 1));
        assert_eq!(room.cursor, before + 1);
        assert_eq!(room.snapshot().users[0].user_id, 2);
        assert!(room.remove(2, Client::Studio));
        assert!(!room.remove(2, Client::Studio));
    }

    #[test]
    fn checks_skew_and_replays() {
        let mut room = Room::default();
        assert!(room
            .check_time(1, 10_000, 10_000 + CLOCK_SKEW_MS, false)
            .is_ok());
        assert_eq!(
            room.check_time(1, 10_000, 10_001 + CLOCK_SKEW_MS, false),
            Err(Refusal::new(401, "time"))
        );
        assert!(room.check_time(1, 70_000, 70_000, true).is_ok());
        assert_eq!(
            room.check_time(1, 70_000, 70_001, true),
            Err(Refusal::new(401, "replay"))
        );
        assert!(room.check_time(1, 70_001, 70_002, true).is_ok());
    }
}
