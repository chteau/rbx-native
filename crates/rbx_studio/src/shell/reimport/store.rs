//! Where each reimportable instance's file is on this device. As in Studio,
//! the place holds only an id (the `RBX_ReimportId` attribute, a UUID
//! string); the path stays on the machine that imported it, so a place
//! shared with someone else carries no paths of yours and each device can
//! point the same model at its own copy of the file. Studio keeps the same
//! map, `{"<id>": {"filepath": …, "preset": …}}`, in its own settings;
//! here it is `<config>/reimport.json`, without the preset this editor's
//! importer has no choice of.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::settings::default_config_dir;

/// The attribute Studio writes on an imported model. Roblox reserves the
/// `RBX` prefix, so the Properties panel cannot set it by hand.
pub(crate) const ID_ATTRIBUTE: &str = "RBX_ReimportId";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Entry {
    filepath: PathBuf,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Store {
    entries: BTreeMap<String, Entry>,
}

impl Store {
    fn path() -> Option<PathBuf> {
        default_config_dir().map(|dir| dir.join("reimport.json"))
    }

    /// The device's map; missing or unreadable is empty.
    pub(crate) fn load() -> Store {
        Self::path().map(|p| Self::read(&p)).unwrap_or_default()
    }

    pub(crate) fn read(path: &Path) -> Store {
        let entries = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Store { entries }
    }

    pub(crate) fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("no config directory to keep reimport settings in")?;
        self.write(&path)
    }

    pub(crate) fn write(&self, path: &Path) -> Result<(), String> {
        let text = serde_json::to_string_pretty(&self.entries).map_err(|e| e.to_string())?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub(crate) fn file(&self, id: &str) -> Option<&Path> {
        self.entries.get(id).map(|e| e.filepath.as_path())
    }

    pub(crate) fn set(&mut self, id: &str, file: PathBuf) {
        self.entries.insert(id.to_owned(), Entry { filepath: file });
    }

    pub(crate) fn remove(&mut self, id: &str) {
        self.entries.remove(id);
    }
}

/// A random (version 4) UUID in Studio's lowercase hyphenated form.
pub(crate) fn new_id() -> String {
    let mut bytes = [0u8; 16];
    // Two imports given the same id would reimport into each other; the
    // clock is a fallback that stays distinct within a session.
    if getrandom::fill(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        bytes = nanos.to_le_bytes();
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}
