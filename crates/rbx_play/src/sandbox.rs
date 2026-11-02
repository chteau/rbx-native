//! Running inside a Flatpak sandbox.
//!
//! The Flatpak build can't run host programs such as `flatpak`, `unshare` or
//! `systemctl`, and can't open `/dev/uinput`. Callers check [`is_flatpak`]
//! and say so in the UI instead of failing obscurely.

use std::path::Path;
use std::sync::OnceLock;

/// Whether this process runs inside a Flatpak sandbox: Flatpak puts
/// `/.flatpak-info` at the root of every one. Checked once.
pub fn is_flatpak() -> bool {
    static FLATPAK: OnceLock<bool> = OnceLock::new();
    *FLATPAK.get_or_init(|| Path::new("/.flatpak-info").exists())
}
