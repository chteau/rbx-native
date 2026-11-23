//! The `.rbxrepro` file: one recorded playtest packed with what it needs to
//! be replayed on someone else's computer, for bug reports.
//!
//! It is a zip archive (any unzip tool lists it) holding:
//!
//! - `repro.json`, the [`Manifest`]: `format` is always `"rbxrepro"`,
//!   `version` this layout's number (1), and `kind` which recording it
//!   carries. `"rbxrec"` (a [`super::format`] file) is the only kind so
//!   far; a later kind (Local Run's deterministic `.rbxnrec`) gets its own
//!   name, and an editor that doesn't know a kind says so instead of
//!   guessing. Then the editor version that wrote it, when (Unix seconds),
//!   the sender's note, the recording's entry name, and `place`: its name,
//!   the SHA-256 and size of the place bytes, the Roblox universe and place
//!   ids when it is published, and `file`, the entry holding the place, or
//!   null when the sender left it out (published places only).
//! - the recording, `recording.rbxrec`.
//! - the place, `place.rbxl` (binary), when embedded.
//!
//! A reader refuses a newer `version` or an unknown `kind` with a sentence
//! saying why; fields it doesn't know are ignored, so adding one doesn't
//! need a new version.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{format, Recording};

pub(crate) const EXTENSION: &str = "rbxrepro";
pub(crate) const FORMAT: &str = "rbxrepro";
pub(crate) const VERSION: u32 = 1;
/// The recording kind this editor writes and replays.
pub(crate) const KIND_RBXREC: &str = "rbxrec";
const MANIFEST: &str = "repro.json";
const RECORDING: &str = "recording.rbxrec";
const PLACE: &str = "place.rbxl";
/// No entry is read past this, so a hostile archive can't claim terabytes.
const ENTRY_LIMIT: u64 = 2 << 30;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Manifest {
    pub(crate) format: String,
    pub(crate) version: u32,
    pub(crate) kind: String,
    pub(crate) editor_version: String,
    pub(crate) created: u64,
    #[serde(default)]
    pub(crate) note: String,
    pub(crate) recording: String,
    pub(crate) place: PlaceInfo,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct PlaceInfo {
    pub(crate) name: String,
    /// The archive entry holding the place; `None` when it was left out.
    pub(crate) file: Option<String>,
    pub(crate) sha256: String,
    pub(crate) size: u64,
    pub(crate) universe_id: Option<u64>,
    pub(crate) place_id: Option<u64>,
}

/// What [`pack`] needs to know about the place besides its bytes.
pub(crate) struct PlaceMeta {
    pub(crate) name: String,
    pub(crate) universe_id: Option<u64>,
    pub(crate) place_id: Option<u64>,
}

/// A read repro.
#[derive(Debug)]
pub(crate) struct Repro {
    pub(crate) manifest: Manifest,
    pub(crate) recording: Recording,
    pub(crate) place: Option<Vec<u8>>,
}

/// A repro ready for an editor window: the place file to open (a copy, so
/// nothing the window does reaches anyone's real place) and what to say
/// about it.
#[derive(Debug)]
pub(crate) struct Opened {
    pub(crate) file: PathBuf,
    pub(crate) place: PathBuf,
    pub(crate) manifest: Manifest,
    pub(crate) recording: Recording,
    /// Version and place mismatches, one sentence each.
    pub(crate) notes: Vec<String>,
}

pub(crate) fn is_repro(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(EXTENSION))
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// `512 B`, `12.3 KB`, `4.0 MB`.
pub(crate) fn size_label(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{:.1} KB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.0),
    }
}

/// The archive for `recording` recorded against `place` (the place's
/// binary bytes; hashed always, stored only when `embed`).
pub(crate) fn pack(
    recording: &Recording,
    place: &[u8],
    meta: PlaceMeta,
    embed: bool,
    note: &str,
    created: u64,
) -> Result<Vec<u8>, String> {
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        kind: KIND_RBXREC.into(),
        editor_version: env!("CARGO_PKG_VERSION").into(),
        created,
        note: note.trim().into(),
        recording: RECORDING.into(),
        place: PlaceInfo {
            name: meta.name,
            file: embed.then(|| PLACE.into()),
            sha256: sha256_hex(place),
            size: place.len() as u64,
            universe_id: meta.universe_id,
            place_id: meta.place_id,
        },
    };
    let json = serde_json::to_vec_pretty(&manifest).map_err(|err| err.to_string())?;
    let mut entries: Vec<(&str, &[u8])> = vec![(MANIFEST, &json)];
    let encoded = format::encode(recording);
    entries.push((RECORDING, &encoded));
    if embed {
        entries.push((PLACE, place));
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .large_file(place.len() as u64 >= u32::MAX as u64);
    for (name, bytes) in entries {
        zip.start_file(name, options)
            .and_then(|()| zip.write_all(bytes).map_err(Into::into))
            .map_err(|err| err.to_string())?;
    }
    zip.finish()
        .map(Cursor::into_inner)
        .map_err(|err| err.to_string())
}

fn entry(archive: &mut zip::ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, String> {
    let file = archive
        .by_name(name)
        .map_err(|_| format!("it has no {name} inside"))?;
    if file.size() > ENTRY_LIMIT {
        return Err(format!("its {name} is too big to read"));
    }
    let mut out = Vec::with_capacity(file.size() as usize);
    file.take(ENTRY_LIMIT)
        .read_to_end(&mut out)
        .map_err(|err| format!("its {name} is damaged ({err})"))?;
    Ok(out)
}

/// `Err` is a sentence for the user: not a repro, made by a newer editor,
/// a recording kind this one can't replay, or damage.
pub(crate) fn unpack(bytes: &[u8]) -> Result<Repro, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "it isn't a repro file (not a zip archive)".to_string())?;
    let json = entry(&mut archive, MANIFEST)
        .map_err(|_| "it isn't a repro file (no repro.json inside)".to_string())?;
    let value: serde_json::Value = serde_json::from_slice(&json)
        .map_err(|err| format!("its repro.json can't be read ({err})"))?;
    if value.get("format").and_then(|v| v.as_str()) != Some(FORMAT) {
        return Err("it isn't a repro file (repro.json names another format)".into());
    }
    let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0);
    if version > VERSION as u64 {
        return Err(format!(
            "it was made by a newer RbxNative ({}, repro format {version}); this one, {}, reads up to format {VERSION}. Update RbxNative to open it",
            value
                .get("editor_version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown version"),
            env!("CARGO_PKG_VERSION"),
        ));
    }
    let manifest: Manifest = serde_json::from_value(value)
        .map_err(|err| format!("its repro.json is incomplete ({err})"))?;
    if manifest.kind != KIND_RBXREC {
        return Err(format!(
            "it holds a {} recording, which this RbxNative ({}) can't replay",
            manifest.kind,
            env!("CARGO_PKG_VERSION"),
        ));
    }
    let recording = format::decode(&entry(&mut archive, &manifest.recording)?)
        .map_err(|err| format!("its recording can't be read: {err}"))?;
    let place = match &manifest.place.file {
        Some(name) => {
            let place = entry(&mut archive, name)?;
            if sha256_hex(&place) != manifest.place.sha256 {
                return Err(
                    "its place is damaged (it doesn't match the hash it was saved with)".into(),
                );
            }
            Some(place)
        }
        None => None,
    };
    Ok(Repro {
        manifest,
        recording,
        place,
    })
}

/// A sentence when the repro was written by another editor version.
pub(crate) fn version_note(made_with: &str, running: &str) -> Option<String> {
    let parse = |v: &str| -> Option<Vec<u64>> {
        v.split(['.', '-', '+'])
            .take(3)
            .map(|part| part.parse().ok())
            .collect()
    };
    if made_with == running {
        return None;
    }
    let newer = match (parse(made_with), parse(running)) {
        (Some(a), Some(b)) => a > b,
        _ => true,
    };
    Some(if newer {
        format!("Made with RbxNative {made_with}, newer than this one ({running}): anything it recorded that this version doesn't know is left out of the replay.")
    } else {
        format!("Made with RbxNative {made_with}; this is {running}.")
    })
}

/// Unpacks `path` and puts its place in a folder of its own under `root`:
/// the embedded one, else a copy of the user's own copy of the published
/// place (`find_local` looks it up by place id), else an empty Baseplate,
/// with a note for each mismatch.
pub(crate) fn open(
    path: &Path,
    root: &Path,
    find_local: impl FnOnce(u64) -> Option<PathBuf>,
) -> Result<Opened, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let repro = unpack(&bytes)?;
    let mut notes: Vec<String> =
        version_note(&repro.manifest.editor_version, env!("CARGO_PKG_VERSION"))
            .into_iter()
            .collect();
    let dir = root.join(format!("{:016x}", crate::recovery::fnv1a(&bytes)));
    std::fs::create_dir_all(&dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    let place = dir.join(PLACE);
    let _ = std::fs::remove_file(&place);
    let info = &repro.manifest.place;
    let which = match info.place_id {
        Some(id) => format!("place {id}"),
        None => "its place".into(),
    };
    let local = match repro.place {
        Some(_) => None,
        None => info.place_id.and_then(find_local),
    };
    match (&repro.place, local) {
        (Some(bytes), _) => crate::save::write_atomic(&place, bytes)?,
        (None, Some(local)) => {
            let bytes =
                std::fs::read(&local).map_err(|err| format!("{}: {err}", local.display()))?;
            if sha256_hex(&bytes) != info.sha256 {
                notes.push(format!(
                    "This repro doesn't carry its place, so it replays over your copy of {which} ({}), which isn't the same file as the one recorded: it was saved since, or in another format, so things may not line up.",
                    local.display()
                ));
            }
            crate::save::write_atomic(&place, &bytes)?;
        }
        (None, None) => {
            notes.push(format!(
                "This repro doesn't carry its place, and {which} isn't on this computer, so it replays over an empty Baseplate. Open {} from Home first to replay it over the real place.",
                info.name
            ));
            crate::home::Template::Baseplate.create(&place)?;
        }
    }
    Ok(Opened {
        file: path.to_path_buf(),
        place,
        manifest: repro.manifest,
        recording: repro.recording,
        notes,
    })
}

/// [`open`] for the editor: the place goes in the OS temp folder, and a
/// published place that was left out is looked for in Recent.
pub(crate) fn open_for_editor(path: &Path) -> Result<Opened, String> {
    open(path, &std::env::temp_dir().join("rbxnative-repro"), |id| {
        crate::home::recent()
            .into_iter()
            .find(|place| place.place_id == Some(id) && place.path.is_file())
            .map(|place| place.path)
    })
}
