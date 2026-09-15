//! Errors specific to locating and reading Sober's local Roblox APK.

use thiserror::Error;

/// Errors that can occur while probing or reading from a Sober (Flatpak)
/// installation.
#[derive(Debug, Error)]
pub enum SoberError {
    #[error("Sober (org.vinegarhq.Sober) is not installed via Flatpak")]
    NotInstalled,

    #[error(
        "Sober is installed but has never been run (no downloaded Roblox APK yet); launch it once"
    )]
    NeverRun,

    #[error("{0:?} is not present in Sober's Roblox APK")]
    NotFound(String),

    #[error("failed to read Sober's Roblox APK as a zip: {0}")]
    Zip(String),

    #[error("I/O error reading Sober's data: {0}")]
    Io(String),
}
