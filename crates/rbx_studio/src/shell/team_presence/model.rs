//! What the Collaborators dock, the viewport and the Explorer show about
//! other people, worked out from the relay's snapshot and the open place —
//! no GPUI here, so all of it is unit-tested.

use std::collections::VecDeque;

use rbx_dom::{Ref, WeakDom};
use rbx_presence::paths;
use rbx_presence::schema::{self, Client, Edit, Snapshot, UserState};

/// How long someone who stopped sending stays listed as "Not in Studio".
pub(crate) const DEPARTED_KEEP_MS: u64 = 10 * 60 * 1000;
/// Two edits of the same property of the same instance this close together
/// are one entry (a drag writes `CFrame` on every mouse move).
pub(crate) const EDIT_MERGE_MS: u64 = 2_000;
/// Edits older than this are no longer sent.
pub(crate) const EDIT_KEEP_MS: u64 = 60_000;

/// A collaborator's state, as the dock words it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Status {
    /// Studio (or rbx-native) is open and the plugin is sending.
    Sending,
    /// Roblox lists them in the Team Create session, but nothing arrives:
    /// no plugin, or it is turned off.
    InSessionWithoutPlugin,
    /// They were sending earlier and stopped.
    NotInStudio,
}

impl Status {
    /// "Sending presence", "In session without the plugin", "Not in Studio".
    pub(crate) fn label(self) -> &'static str {
        match self {
            Status::Sending => "Sending presence",
            Status::InSessionWithoutPlugin => "In session without the plugin",
            Status::NotInStudio => "Not in Studio",
        }
    }
}

/// Someone Roblox says is in the Team Create session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    pub(crate) user_id: u64,
    pub(crate) name: String,
}

/// Someone who was sending and stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Departed {
    pub(crate) user_id: u64,
    pub(crate) name: String,
    pub(crate) color: String,
    pub(crate) left_at: u64,
}

/// One row of the dock, with what it points at in this place resolved.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Collaborator {
    pub(crate) user_id: u64,
    pub(crate) name: String,
    pub(crate) color: (u8, u8, u8),
    /// `None` for someone known only from the member list or a departure.
    pub(crate) client: Option<Client>,
    pub(crate) status: Status,
    /// The selection paths that resolve in this place.
    pub(crate) selection: Vec<Ref>,
    /// How many selection paths did not resolve (renamed, deleted, or not
    /// in this copy of the place).
    pub(crate) unresolved: usize,
    /// The open script's path as sent, and what it resolves to here.
    pub(crate) script: Option<(String, Option<Ref>)>,
    pub(crate) camera: Option<[f64; 12]>,
    pub(crate) edits: Vec<Edit>,
}

/// `#rrggbb` (either case) as bytes; `None` otherwise.
pub(crate) fn parse_color(hex: &str) -> Option<(u8, u8, u8)> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 || !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).ok();
    Some((byte(0)?, byte(2)?, byte(4)?))
}

/// Everyone to list, `own_id` left out: senders from `snapshot` whose
/// `seen` is within `schema::EXPIRY_MS` of `now_ms` (`Sending`), `members`
/// who are not sending (`InSessionWithoutPlugin`), then `departed` within
/// `DEPARTED_KEEP_MS` who are in neither (`NotInStudio`). Sorted by status,
/// then name (case-insensitive). Paths resolve against `dom` with
/// `rbx_presence::paths::resolve`. Colours are not taken from what senders
/// claim: `schema::assign_colors` gives everyone in the room (the snapshot's
/// users plus `own_id` when it is not 0) a distinct one, with members and
/// departed ids placed after them, so every client agrees.
pub(crate) fn collaborators(
    snapshot: Option<&Snapshot>,
    members: Option<&[Member]>,
    departed: &[Departed],
    own_id: u64,
    dom: &WeakDom,
    now_ms: u64,
) -> Vec<Collaborator> {
    let mut out: Vec<Collaborator> = Vec::new();
    let mut listed: Vec<u64> = vec![own_id];

    let senders = snapshot.map(|s| s.users.as_slice()).unwrap_or(&[]);
    let mut room: Vec<u64> = senders.iter().map(|u| u.user_id).collect();
    if own_id != 0 {
        room.push(own_id);
    }
    let extra: Vec<u64> = members
        .unwrap_or(&[])
        .iter()
        .map(|m| m.user_id)
        .chain(departed.iter().map(|d| d.user_id))
        .collect();
    let assigned = schema::assign_colors(&room, &extra);
    let color_of = |id: u64| {
        parse_color(assigned.get(&id).copied().unwrap_or(schema::color_for(id)))
            .unwrap_or((128, 128, 128))
    };
    let mut senders_seen: Vec<(u64, Client)> = Vec::new();
    for user in senders {
        let key = schema::sender_key(user);
        if schema::is_own(user, own_id)
            || senders_seen.contains(&key)
            || now_ms.saturating_sub(user.seen) > schema::EXPIRY_MS
        {
            continue;
        }
        senders_seen.push(key);
        listed.push(user.user_id);
        let mut selection = Vec::new();
        let mut unresolved = 0;
        for path in &user.selection {
            match paths::resolve(dom, path) {
                Some(r) => selection.push(r),
                None => unresolved += 1,
            }
        }
        out.push(Collaborator {
            user_id: user.user_id,
            // The signed-in user in Studio at the same time (another
            // machine, or both editors side by side).
            name: if user.user_id == own_id {
                format!("{} (you, in Studio)", user.name)
            } else {
                user.name.clone()
            },
            color: color_of(user.user_id),
            client: Some(user.client),
            status: Status::Sending,
            selection,
            unresolved,
            script: user
                .script
                .as_ref()
                .map(|p| (p.clone(), paths::resolve(dom, p))),
            camera: user.camera,
            edits: user.edits.clone(),
        });
    }

    for member in members.unwrap_or(&[]) {
        if listed.contains(&member.user_id) {
            continue;
        }
        listed.push(member.user_id);
        out.push(bare(
            member.user_id,
            &member.name,
            color_of(member.user_id),
            Status::InSessionWithoutPlugin,
        ));
    }

    for gone in departed {
        if listed.contains(&gone.user_id) || now_ms.saturating_sub(gone.left_at) > DEPARTED_KEEP_MS
        {
            continue;
        }
        listed.push(gone.user_id);
        out.push(bare(
            gone.user_id,
            &gone.name,
            color_of(gone.user_id),
            Status::NotInStudio,
        ));
    }

    out.sort_by_cached_key(|c| (c.status, c.name.to_lowercase()));
    out
}

fn bare(user_id: u64, name: &str, color: (u8, u8, u8), status: Status) -> Collaborator {
    Collaborator {
        user_id,
        name: name.to_string(),
        color,
        client: None,
        status,
        selection: Vec::new(),
        unresolved: 0,
        script: None,
        camera: None,
        edits: Vec::new(),
    }
}

/// The senders in `previous` (not `own_id`) missing from `next`, as
/// departures at `now_ms`.
pub(crate) fn departures(
    previous: &Snapshot,
    next: &Snapshot,
    own_id: u64,
    now_ms: u64,
) -> Vec<Departed> {
    previous
        .users
        .iter()
        .filter(|u| {
            // Yourself, in either editor, never "leaves" the list.
            u.user_id != own_id
                && !next
                    .users
                    .iter()
                    .any(|n| schema::sender_key(n) == schema::sender_key(u))
        })
        .map(|u| Departed {
            user_id: u.user_id,
            name: u.name.clone(),
            color: u.color.clone(),
            left_at: now_ms,
        })
        .collect()
}

/// The user's own recent property edits, as sent.
#[derive(Debug, Default)]
pub(crate) struct EditLog {
    entries: VecDeque<Edit>,
}

impl EditLog {
    /// Records one edit: replaces the newest entry for the same path and
    /// property when it is within `EDIT_MERGE_MS`, else appends; keeps at
    /// most `schema::MAX_EDITS`. `value` is cut to 80 chars with "…".
    pub(crate) fn note(&mut self, path: String, property: String, value: String, now_ms: u64) {
        let value = if value.chars().count() > 80 {
            let mut cut: String = value.chars().take(79).collect();
            cut.push('…');
            cut
        } else {
            value
        };
        let edit = Edit {
            path,
            property,
            value,
            at: now_ms,
        };
        let same = self
            .entries
            .iter_mut()
            .rev()
            .find(|e| e.path == edit.path && e.property == edit.property);
        match same {
            Some(old) if now_ms.saturating_sub(old.at) <= EDIT_MERGE_MS => *old = edit,
            _ => self.entries.push_back(edit),
        }
        while self.entries.len() > schema::MAX_EDITS {
            self.entries.pop_front();
        }
    }

    /// The entries newer than `EDIT_KEEP_MS`, oldest first.
    pub(crate) fn recent(&self, now_ms: u64) -> Vec<Edit> {
        self.entries
            .iter()
            .filter(|e| now_ms.saturating_sub(e.at) < EDIT_KEEP_MS)
            .cloned()
            .collect()
    }
}

/// A canned room for `RBX_STUDIO_PRESENCE_MOCK`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Mock {
    pub(crate) snapshot: Snapshot,
    pub(crate) members: Vec<Member>,
    pub(crate) departed: Vec<Departed>,
}

/// `spec` is `fixture` (the committed
/// `plugins/team-presence/fixtures/mock_room.json`, via `include_str!`) or a
/// path to a file of the same shape: `{snapshot, members:[{userId,name}],
/// departed:[{userId,name,color}]}`. A user `seen` of 0 becomes `now_ms`,
/// an edit `at` of 0 becomes `now_ms - 4000`, and departures leave at
/// `now_ms - 120000`.
pub(crate) fn load_mock(spec: &str, now_ms: u64) -> Result<Mock, String> {
    let text = if spec == "fixture" {
        include_str!("../../../../../plugins/team-presence/fixtures/mock_room.json").to_string()
    } else {
        std::fs::read_to_string(spec).map_err(|e| format!("{spec}: {e}"))?
    };
    let mut json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("mock room: {e}"))?;
    let snapshot = json
        .get_mut("snapshot")
        .map(serde_json::Value::take)
        .ok_or("mock room: missing snapshot")?;
    let mut snapshot: Snapshot =
        serde_json::from_value(snapshot).map_err(|e| format!("mock snapshot: {e}"))?;
    for user in &mut snapshot.users {
        if user.seen == 0 {
            user.seen = now_ms;
        }
        for edit in &mut user.edits {
            if edit.at == 0 {
                edit.at = now_ms.saturating_sub(4_000);
            }
        }
    }

    let list = |key: &str| {
        json.get(key)
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default()
    };
    let id_name = |v: &serde_json::Value| -> Result<(u64, String), String> {
        let id = v
            .get("userId")
            .and_then(|x| x.as_u64())
            .ok_or("mock room: entry without userId")?;
        let name = v.get("name").and_then(|x| x.as_str()).unwrap_or_default();
        Ok((id, name.to_string()))
    };
    let mut members = Vec::new();
    for v in list("members") {
        let (user_id, name) = id_name(&v)?;
        members.push(Member { user_id, name });
    }
    let mut departed = Vec::new();
    for v in list("departed") {
        let (user_id, name) = id_name(&v)?;
        let color = v
            .get("color")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();
        departed.push(Departed {
            user_id,
            name,
            color,
            left_at: now_ms.saturating_sub(120_000),
        });
    }
    Ok(Mock {
        snapshot,
        members,
        departed,
    })
}

/// The user's own message (`client` rbx-native, colour `color_for`,
/// `seen` 0), clamped.
pub(crate) fn own_state(
    user_id: u64,
    name: &str,
    selection: Vec<String>,
    camera: Option<[f64; 12]>,
    script: Option<String>,
    edits: Vec<Edit>,
) -> UserState {
    let mut state = UserState {
        v: schema::SCHEMA_VERSION,
        user_id,
        name: name.to_string(),
        color: schema::color_for(user_id).to_string(),
        client: Client::RbxNative,
        camera,
        selection,
        script,
        edits,
        seen: 0,
    };
    state.clamp();
    state
}

#[cfg(test)]
mod tests;
