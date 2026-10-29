//! What update planning asks of Roblox, behind a trait so the window and the
//! runner work the same against `rbx_cloud` and against the mock
//! `RBX_STUDIO_UPDATE_PLAN_MOCK=1` serves (nothing leaves the machine: the
//! mock's "applied" icons are written to `<root>/mock-applied/`).

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use rbx_cloud::{Client, Moderation, Owner};

pub(crate) const MOCK_VARIABLE: &str = "RBX_STUDIO_UPDATE_PLAN_MOCK";

pub(crate) trait IconApi: Send + Sync {
    /// Makes `png` the experience's icon for `language`.
    fn set_icon(&self, universe_id: u64, language: &str, png: &[u8]) -> Result<(), String>;
    /// Uploads `png` for review ahead of time; the new asset's id.
    fn upload(&self, name: &str, owner: Owner, png: &[u8]) -> Result<u64, String>;
    fn review(&self, asset_id: u64) -> Result<Moderation, String>;
    /// The icon as it is now, and the language code it is set for.
    fn current_icon(&self, universe_id: u64) -> Result<(Vec<u8>, String), String>;
    /// Who owns the experience, so the uploads land in the same inventory.
    fn owner(&self, universe_id: u64) -> Result<Owner, String>;
}

pub(crate) fn mock_enabled() -> bool {
    std::env::var_os(MOCK_VARIABLE).is_some_and(|v| !v.is_empty())
}

/// The mock when it is on, else the stored key's client; `None` without
/// a key.
pub(crate) fn connect() -> Option<Box<dyn IconApi>> {
    if mock_enabled() {
        return Some(Box::new(Mock {
            applied: super::store::root()?.join("mock-applied"),
        }));
    }
    rbx_cloud::ApiKey::from_env_or_config().map(|key| Box::new(Cloud(Client::new(Some(key)))) as _)
}

struct Cloud(Client);

impl IconApi for Cloud {
    fn set_icon(&self, universe_id: u64, language: &str, png: &[u8]) -> Result<(), String> {
        self.0
            .set_game_icon(universe_id, language, png)
            .map_err(|e| e.to_string())
    }

    fn upload(&self, name: &str, owner: Owner, png: &[u8]) -> Result<u64, String> {
        self.0
            .upload_icon_image(name, owner, png)
            .map_err(|e| e.to_string())
    }

    fn review(&self, asset_id: u64) -> Result<Moderation, String> {
        self.0.image_moderation(asset_id).map_err(|e| e.to_string())
    }

    fn current_icon(&self, universe_id: u64) -> Result<(Vec<u8>, String), String> {
        let urls = self
            .0
            .game_icon_urls(&[universe_id])
            .map_err(|e| e.to_string())?;
        let (_, url) = urls
            .into_iter()
            .next()
            .ok_or("Roblox has no approved icon for this experience yet")?;
        let png = self.0.download_image(&url).map_err(|e| e.to_string())?;
        // The icon list names the languages that have an icon; without the
        // scope, or with none listed, English is the usual source language.
        let language = self
            .0
            .game_icons(universe_id)
            .ok()
            .and_then(|icons| icons.into_iter().next())
            .map(|icon| icon.language)
            .filter(|code| !code.is_empty())
            .unwrap_or_else(|| "en".to_string());
        Ok((png, language))
    }

    fn owner(&self, universe_id: u64) -> Result<Owner, String> {
        self.0
            .universe(universe_id)
            .map(|u| u.owner)
            .map_err(|e| e.to_string())
    }
}

/// Seconds a mock upload stays "Reviewing".
const MOCK_REVIEW_SECONDS: u64 = 6;

struct Mock {
    applied: PathBuf,
}

/// When each mock upload was made, by `id - MOCK_FIRST_ID`; process-wide,
/// since every call gets a fresh `Mock`.
static MOCK_UPLOADS: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
const MOCK_FIRST_ID: u64 = 9_000_000;

impl IconApi for Mock {
    fn set_icon(&self, universe_id: u64, language: &str, png: &[u8]) -> Result<(), String> {
        let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%S");
        let path = self
            .applied
            .join(format!("{universe_id}-{language}-{stamp}.png"));
        super::store::write(&path, png).map_err(|e| e.to_string())?;
        eprintln!("rbxstudio: mock icon applied: {}", path.display());
        Ok(())
    }

    fn upload(&self, _name: &str, _owner: Owner, _png: &[u8]) -> Result<u64, String> {
        let mut uploads = MOCK_UPLOADS.lock().unwrap_or_else(|e| e.into_inner());
        uploads.push(Instant::now());
        Ok(MOCK_FIRST_ID + uploads.len() as u64 - 1)
    }

    fn review(&self, asset_id: u64) -> Result<Moderation, String> {
        let uploads = MOCK_UPLOADS.lock().unwrap_or_else(|e| e.into_inner());
        let index = asset_id.checked_sub(MOCK_FIRST_ID).unwrap_or(u64::MAX);
        Ok(match uploads.get(index as usize) {
            Some(at) if at.elapsed().as_secs() < MOCK_REVIEW_SECONDS => Moderation::Reviewing,
            _ => Moderation::Approved,
        })
    }

    fn current_icon(&self, universe_id: u64) -> Result<(Vec<u8>, String), String> {
        let image = super::mock::icon(universe_id).ok_or("mock icon failed to draw")?;
        Ok((super::overlay::png(&image), "en".to_string()))
    }

    fn owner(&self, universe_id: u64) -> Result<Owner, String> {
        super::mock::experiences()
            .into_iter()
            .find(|g| g.universe_id == universe_id)
            .map(|g| g.owner)
            .ok_or_else(|| "not a mock experience".to_string())
    }
}
