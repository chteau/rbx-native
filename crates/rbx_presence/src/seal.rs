//! A presence state as the relay sees it: [`crate::schema::Sealed`], the
//! state encrypted with [`crate::crypt`] plus the few fields the relay needs
//! to route, rate-limit and journal without reading it.

use crate::catchup::JournalEntry;
use crate::crypt;
use crate::schema::{Sealed, SealedEdit, SealedJournalEntry, UserState, SCHEMA_VERSION};

/// Plaintext of one sealed journal entry.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct JournalPlain {
    pub name: String,
    pub path: String,
    pub property: String,
}

/// Seals `state` (clamped first) for `room`: `ct` is the state JSON, `j` one
/// sealed journal entry per edit (value left out), `seen` 0.
pub fn seal_state(key: &[u8; 32], room: &str, state: &UserState) -> Sealed {
    let mut state = state.clone();
    state.clamp();
    state.v = SCHEMA_VERSION;
    let json = serde_json::to_vec(&state).expect("a UserState always serializes");
    let ct = crypt::b64_encode(&crypt::seal(
        key,
        &crypt::state_aad(room, state.user_id, state.client),
        &json,
    ));
    let j = state
        .edits
        .iter()
        .map(|edit| {
            let plain = JournalPlain {
                name: state.name.clone(),
                path: edit.path.clone(),
                property: edit.property.clone(),
            };
            let json = serde_json::to_vec(&plain).expect("a JournalPlain always serializes");
            SealedEdit {
                at: edit.at,
                tag: crypt::journal_tag(key, &edit.path, &edit.property, edit.at),
                ct: crypt::b64_encode(&crypt::seal(
                    key,
                    &crypt::journal_aad(room, state.user_id, edit.at),
                    &json,
                )),
            }
        })
        .collect();
    Sealed {
        v: SCHEMA_VERSION,
        user_id: state.user_id,
        client: state.client,
        ct,
        j,
        seen: 0,
    }
}

/// Opens a sealed state: `None` when it fails to authenticate, is not JSON
/// of the current version, or its inner `userId`/`client` differ from the
/// envelope's. The result carries the envelope's `seen`.
pub fn open_state(key: &[u8; 32], room: &str, sealed: &Sealed) -> Option<UserState> {
    let bytes = crypt::b64_decode(&sealed.ct)?;
    let plain = crypt::open(
        key,
        &crypt::state_aad(room, sealed.user_id, sealed.client),
        &bytes,
    )?;
    let mut state: UserState = serde_json::from_slice(&plain).ok()?;
    if state.v != SCHEMA_VERSION || state.user_id != sealed.user_id || state.client != sealed.client
    {
        return None;
    }
    state.seen = sealed.seen;
    Some(state)
}

/// Opens one relay journal entry; `None` when it fails to authenticate.
pub fn open_journal(
    key: &[u8; 32],
    room: &str,
    entry: &SealedJournalEntry,
) -> Option<JournalEntry> {
    let bytes = crypt::b64_decode(&entry.ct)?;
    let plain = crypt::open(
        key,
        &crypt::journal_aad(room, entry.user_id, entry.at),
        &bytes,
    )?;
    let plain: JournalPlain = serde_json::from_slice(&plain).ok()?;
    Some(JournalEntry {
        user_id: entry.user_id,
        name: plain.name,
        path: plain.path,
        property: plain.property,
        at: entry.at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Client, Edit};

    fn state() -> UserState {
        UserState {
            v: SCHEMA_VERSION,
            user_id: 7,
            name: "Ada".into(),
            color: "#e8590c".into(),
            client: Client::RbxNative,
            camera: None,
            selection: vec!["Workspace.Part".into()],
            script: None,
            edits: vec![Edit {
                path: "Workspace.Part".into(),
                property: "Size".into(),
                value: "1, 2, 3".into(),
                at: 1000,
            }],
            seen: 0,
        }
    }

    #[test]
    fn state_round_trips_with_envelope_seen() {
        let key = crypt::enc_key("s", 1);
        let mut sealed = seal_state(&key, "room", &state());
        assert_eq!(sealed.j.len(), 1);
        assert_eq!(sealed.seen, 0);
        sealed.seen = 55;
        let mut expected = state();
        expected.seen = 55;
        assert_eq!(open_state(&key, "room", &sealed), Some(expected));
    }

    #[test]
    fn wrong_room_key_or_tamper_fails() {
        let key = crypt::enc_key("s", 1);
        let sealed = seal_state(&key, "room", &state());
        assert_eq!(open_state(&key, "other", &sealed), None);
        assert_eq!(open_state(&crypt::enc_key("s", 2), "room", &sealed), None);
        let mut bytes = crypt::b64_decode(&sealed.ct).unwrap();
        bytes[crypt::NONCE_LEN] ^= 1;
        let tampered = Sealed {
            ct: crypt::b64_encode(&bytes),
            ..sealed.clone()
        };
        assert_eq!(open_state(&key, "room", &tampered), None);
        let spoofed = Sealed {
            user_id: 8,
            ..sealed
        };
        assert_eq!(open_state(&key, "room", &spoofed), None);
    }

    #[test]
    fn journal_entry_round_trips() {
        let key = crypt::enc_key("s", 1);
        let sealed = seal_state(&key, "room", &state());
        let edit = &sealed.j[0];
        let entry = SealedJournalEntry {
            user_id: 7,
            at: edit.at,
            ct: edit.ct.clone(),
        };
        let opened = open_journal(&key, "room", &entry).unwrap();
        assert_eq!(opened.name, "Ada");
        assert_eq!(opened.property, "Size");
        assert_eq!(opened.at, 1000);
        assert_eq!(open_journal(&key, "other", &entry), None);
        let later = SealedJournalEntry { at: 1001, ..entry };
        assert_eq!(open_journal(&key, "room", &later), None);
    }
}
