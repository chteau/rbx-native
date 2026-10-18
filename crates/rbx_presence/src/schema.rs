//! The presence message, identical in both directions: the plugin's Luau
//! encoder (`plugins/team-presence/src/Schema.luau`) and these types read and
//! write the same JSON, checked against the shared golden files in
//! `plugins/team-presence/fixtures/`.
//!
//! Field names are camelCase. A missing `camera` or `script` means "none":
//! a Luau table cannot hold `nil`, so the plugin leaves the key out rather
//! than writing `null`, and both are accepted here.

use serde::{Deserialize, Serialize};

/// The `v` every message carries, sealed envelope and the state inside it
/// alike. A relay refuses any other version with `400 {"error":"version"}`,
/// so a client from before encryption fails loudly instead of being ignored.
/// Version 1 was the cleartext wire.
pub const SCHEMA_VERSION: u32 = 2;

/// Largest request or WebSocket message body the relay accepts, in bytes.
pub const MAX_BODY_BYTES: usize = 16 * 1024;
/// Most selection paths one message carries; extra ones are dropped by the
/// sender (the relay rejects the message instead).
pub const MAX_SELECTION: usize = 100;
/// Most recent edits one message carries.
pub const MAX_EDITS: usize = 20;
/// Longest path, property, value or name string, in UTF-8 bytes.
pub const MAX_TEXT: usize = 512;
/// A user the relay has not heard from for this long is dropped.
pub const EXPIRY_MS: u64 = 15_000;
/// How often a client re-sends its state when nothing changed.
pub const HEARTBEAT_MS: u64 = 5_000;

/// Which editor a user is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Client {
    #[serde(rename = "studio")]
    Studio,
    #[serde(rename = "rbx-native")]
    RbxNative,
}

/// One user's presence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserState {
    /// Always [`SCHEMA_VERSION`].
    pub v: u32,
    /// The Roblox user id (below 2^53, so a Luau number holds it exactly).
    pub user_id: u64,
    pub name: String,
    /// `#rrggbb`, lower case.
    pub color: String,
    pub client: Client,
    /// The camera as `CFrame:GetComponents()` orders it: position x, y, z,
    /// then the rotation matrix row by row (R00 R01 R02 R10 … R22).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera: Option<[f64; 12]>,
    /// Selected instances, as [`crate::paths`] paths.
    #[serde(default)]
    pub selection: Vec<String>,
    /// The script open in the editor (Studio's `StudioService.ActiveScript`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// Recent property edits, oldest first.
    #[serde(default)]
    pub edits: Vec<Edit>,
    /// Unix milliseconds the relay last heard from this user; written by the
    /// relay, 0 from a sender.
    #[serde(default)]
    pub seen: u64,
}

/// One property edit someone made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edit {
    pub path: String,
    pub property: String,
    /// The new value as short display text (at most [`MAX_TEXT`] bytes).
    pub value: String,
    /// Unix milliseconds.
    pub at: u64,
}

/// Who sent a state: one user can be in Studio and rbx-native at the same
/// time, and each is a separate sender.
pub fn sender_key(state: &UserState) -> (u64, Client) {
    (state.user_id, state.client)
}

/// Whether `state` is this rbx-native client's own (the same user in Studio
/// is someone else to show).
pub fn is_own(state: &UserState, own_id: u64) -> bool {
    state.user_id == own_id && state.client == Client::RbxNative
}

/// What the relay sends: everyone in the room, the receiver included (a
/// client skips its own state, see [`is_own`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Grows by one each time the room changes; a long-poll waits for a
    /// cursor past the one it names.
    pub cursor: u64,
    pub users: Vec<UserState>,
}

/// One user's state as it crosses the relay: sealed with
/// [`crate::crypt`], plus what the relay needs to route, rate-limit and
/// journal it without the place secret (see [`crate::seal`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sealed {
    /// Always [`SCHEMA_VERSION`].
    pub v: u32,
    pub user_id: u64,
    pub client: Client,
    /// base64(nonce ‖ ciphertext ‖ tag) of the [`UserState`] JSON, AAD
    /// [`crate::crypt::state_aad`].
    pub ct: String,
    /// The state's edits, each sealed on its own for the relay's journal.
    #[serde(default)]
    pub j: Vec<SealedEdit>,
    /// Written by the relay, 0 from a sender.
    #[serde(default)]
    pub seen: u64,
}

/// One edit, sealed for the journal ([`crate::seal::JournalPlain`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SealedEdit {
    pub at: u64,
    /// [`crate::crypt::journal_tag`]: what the relay dedupes on.
    pub tag: String,
    /// AAD [`crate::crypt::journal_aad`].
    pub ct: String,
}

/// What the relay sends: [`Snapshot`] before it is opened.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SealedSnapshot {
    pub cursor: u64,
    pub users: Vec<Sealed>,
}

/// One entry of the relay's journal reply (`{"entries": [...]}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedJournalEntry {
    pub user_id: u64,
    pub at: u64,
    pub ct: String,
}

/// One co-editing log entry as a client posts it (`POST …/ops`): a sealed
/// chunk ([`crate::coedit::chunk::Chunk`] JSON, AAD [`crate::crypt::ops_aad`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SealedOp {
    /// Always [`SCHEMA_VERSION`].
    pub v: u32,
    pub user_id: u64,
    pub client: Client,
    pub ct: String,
}

/// One entry of the relay's op log, as `GET …/ops?since=N` returns it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoggedOp {
    pub seq: u64,
    pub user_id: u64,
    pub client: Client,
    pub ct: String,
}

/// `GET …/ops?since=N`: entries after `since`, oldest first. `head` is the
/// newest seq logged (0 for none); `floor` the oldest still held (entries
/// below it were trimmed, so a reader behind it has missed some).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpsReply {
    pub head: u64,
    pub floor: u64,
    pub entries: Vec<LoggedOp>,
}

/// The colours users are drawn in. A receiver assigns them from who is in
/// the room ([`assign_colors`]) rather than trusting each sender's `color`,
/// so two people in one room never share one while there are colours left.
/// `Schema.PALETTE` in the plugin is the same list in the same order.
pub const PALETTE: [&str; 8] = [
    "#e8590c", "#1c7ed6", "#2f9e44", "#ae3ec9", "#f08c00", "#0c8599", "#e03131", "#7048e8",
];

/// A user's preferred palette slot: FNV-1a (32-bit) of the decimal user id,
/// modulo the palette size. Pinned by `fixtures/colors.json`.
pub fn color_slot(user_id: u64) -> usize {
    let hash = user_id.to_string().bytes().fold(2_166_136_261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16_777_619)
    });
    hash as usize % PALETTE.len()
}

/// `PALETTE[color_slot(user_id)]`: what a sender puts in its own `color`.
pub fn color_for(user_id: u64) -> &'static str {
    PALETTE[color_slot(user_id)]
}

/// Colours for everyone in a room: `room` (the relay snapshot's users, the
/// receiver included) in ascending id order, then `extra` ids not in `room`
/// (people known only from elsewhere) in ascending order. Each takes its
/// preferred slot, or the next free one after it (wrapping); once all eight
/// are taken, its preferred slot again. Duplicates are ignored. Every client
/// that sees the same room computes the same colours.
pub fn assign_colors(room: &[u64], extra: &[u64]) -> std::collections::HashMap<u64, &'static str> {
    let mut room_ids = room.to_vec();
    room_ids.sort_unstable();
    room_ids.dedup();
    let mut extra_ids: Vec<u64> = extra
        .iter()
        .copied()
        .filter(|id| room_ids.binary_search(id).is_err())
        .collect();
    extra_ids.sort_unstable();
    extra_ids.dedup();

    let mut taken = [false; PALETTE.len()];
    let mut colors = std::collections::HashMap::new();
    for id in room_ids.into_iter().chain(extra_ids) {
        let preferred = color_slot(id);
        let slot = (0..PALETTE.len())
            .map(|step| (preferred + step) % PALETTE.len())
            .find(|&slot| !taken[slot])
            .unwrap_or(preferred);
        taken[slot] = true;
        colors.insert(id, PALETTE[slot]);
    }
    colors
}

impl UserState {
    /// Cuts the lists and strings down to the limits above, so the relay
    /// never refuses a message for its size. Truncates strings on a char
    /// boundary; keeps the first selections and the newest edits.
    pub fn clamp(&mut self) {
        fn cut(text: &mut String) {
            if text.len() > MAX_TEXT {
                let mut end = MAX_TEXT;
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                text.truncate(end);
            }
        }
        cut(&mut self.name);
        self.selection.truncate(MAX_SELECTION);
        self.selection.iter_mut().for_each(cut);
        if let Some(script) = &mut self.script {
            cut(script);
        }
        if self.edits.len() > MAX_EDITS {
            self.edits.drain(..self.edits.len() - MAX_EDITS);
        }
        for edit in &mut self.edits {
            cut(&mut edit.path);
            cut(&mut edit.property);
            cut(&mut edit.value);
        }
    }
}
