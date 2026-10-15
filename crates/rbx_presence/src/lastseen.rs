//! Per-place last-seen state, persisted to disk: the newest journal
//! timestamp a client had caught up to, and an opaque snapshot of the place
//! as it last saw it.
//!
//! On reconnect the catch-up summary asks for edits made *since* that
//! timestamp instead of everything the transport still retains, so "While
//! you were away" is scoped to what actually happened while away. The
//! snapshot is the "before" a catch-up diff compares the freshly loaded
//! place against.
//!
//! The store is a directory the caller owns (the editor's config dir); files
//! are keyed by place id. The snapshot bytes are opaque here — the editor
//! serialises its own DOM however it likes. Every function takes the
//! directory, so the whole module is hermetic and tested against a temp dir.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The small sidecar written next to a place's snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    /// Unix milliseconds of the newest edit this client had caught up to.
    journal_ms: u64,
}

/// What [`load`] recovered for a place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The timestamp to ask catch-up for edits since.
    pub journal_ms: u64,
    /// The place snapshot as last saved, if one is on disk. Opaque bytes.
    pub snapshot: Option<Vec<u8>>,
}

fn meta_path(dir: &Path, place_id: u64) -> PathBuf {
    dir.join(format!("{place_id}.json"))
}

fn snapshot_path(dir: &Path, place_id: u64) -> PathBuf {
    dir.join(format!("{place_id}.snapshot"))
}

/// The last-seen record for `place_id`, or `None` if nothing is stored (or
/// the metadata cannot be read). A present record always carries a
/// `journal_ms`; its snapshot is `None` when no snapshot file exists.
pub fn load(dir: &Path, place_id: u64) -> Option<Record> {
    let text = std::fs::read_to_string(meta_path(dir, place_id)).ok()?;
    let meta: Meta = serde_json::from_str(&text).ok()?;
    let snapshot = std::fs::read(snapshot_path(dir, place_id)).ok();
    Some(Record {
        journal_ms: meta.journal_ms,
        snapshot,
    })
}

/// Writes `place_id`'s last-seen timestamp, and its snapshot when one is
/// given. A `None` snapshot leaves any snapshot already on disk untouched,
/// so a periodic timestamp bump need not re-serialise the whole place. Both
/// files are written atomically (temp + rename).
pub fn save(dir: &Path, place_id: u64, journal_ms: u64, snapshot: Option<&[u8]>) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let meta = serde_json::to_vec(&Meta { journal_ms })
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_atomic(&meta_path(dir, place_id), &meta)?;
    if let Some(bytes) = snapshot {
        write_atomic(&snapshot_path(dir, place_id), bytes)?;
    }
    Ok(())
}

/// Writes `bytes` to `path` via a sibling temp file and a rename, so a
/// reader never sees a half-written file.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rbx-presence-lastseen-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn missing_is_none() {
        let dir = temp_dir();
        assert_eq!(load(&dir, 123), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn round_trips_timestamp_and_snapshot() {
        let dir = temp_dir();
        save(&dir, 777, 1_700, Some(b"snapshot-bytes")).unwrap();
        let got = load(&dir, 777).unwrap();
        assert_eq!(got.journal_ms, 1_700);
        assert_eq!(got.snapshot.as_deref(), Some(&b"snapshot-bytes"[..]));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn none_snapshot_keeps_existing() {
        let dir = temp_dir();
        save(&dir, 1, 100, Some(b"first")).unwrap();
        save(&dir, 1, 200, None).unwrap();
        let got = load(&dir, 1).unwrap();
        assert_eq!(got.journal_ms, 200);
        assert_eq!(got.snapshot.as_deref(), Some(&b"first"[..]));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn timestamp_without_any_snapshot() {
        let dir = temp_dir();
        save(&dir, 42, 500, None).unwrap();
        let got = load(&dir, 42).unwrap();
        assert_eq!(got.journal_ms, 500);
        assert_eq!(got.snapshot, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn places_do_not_collide() {
        let dir = temp_dir();
        save(&dir, 1, 10, Some(b"one")).unwrap();
        save(&dir, 2, 20, Some(b"two")).unwrap();
        assert_eq!(
            load(&dir, 1).unwrap().snapshot.as_deref(),
            Some(&b"one"[..])
        );
        assert_eq!(
            load(&dir, 2).unwrap().snapshot.as_deref(),
            Some(&b"two"[..])
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
