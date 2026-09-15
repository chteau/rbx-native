//! Resolves `rbxasset://` paths against the Studio content packages Roblox
//! publishes on `setup.rbxcdn.com` (the same CDN Vinegar/Sober pull from).
//!
//! Nothing is embedded in this crate: packages are fetched on demand, cached
//! whole, then the one requested file is extracted from the cached zip.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use crate::error::AssetError;
use crate::sober::Sober;

const SETUP_CDN: &str = "https://setup.rbxcdn.com";
const VERSION_ENDPOINT: &str = "https://setup.rbxcdn.com/versionQTStudio";
/// Packages have been observed in the tens of MiB; anything past this is
/// almost certainly a manifest pointing at the wrong URL, not a real package.
const MAX_PACKAGE_DOWNLOAD_BYTES: u64 = 200 * 1024 * 1024;

pub struct NativeContent {
    packages_dir: PathBuf,
}

impl NativeContent {
    /// `packages_dir` is where downloaded package zips are cached
    /// (conventionally `<asset cache root>/native-packages/`).
    pub fn new(packages_dir: PathBuf) -> Self {
        Self { packages_dir }
    }

    /// Fetches the single file at `path` (e.g. `"sky/sun.jpg"`) from whichever
    /// Studio content package contains it.
    pub(crate) fn fetch(&self, path: &str) -> Result<Vec<u8>, AssetError> {
        self.fetch_from_cdn(path)
            .or_else(|cdn_err| self.fetch_from_sober(path).ok_or(cdn_err))
    }

    fn fetch_from_cdn(&self, path: &str) -> Result<Vec<u8>, AssetError> {
        let candidates = package_candidates_for_path(path)
            .ok_or_else(|| AssetError::UnknownNativePackage("<none>", path.to_string()))?;

        let version = fetch_studio_version()?;
        let mut last_not_found = None;
        for package in candidates {
            match self.fetch_from_package(&version, package, path) {
                Ok(bytes) => return Ok(bytes),
                Err(AssetError::NativeFileNotFound(p)) => last_not_found = Some(p),
                Err(other) => return Err(other),
            }
        }
        Err(AssetError::NativeFileNotFound(
            last_not_found.unwrap_or_else(|| path.to_string()),
        ))
    }

    /// Best-effort extra fallback through a local Sober (Flatpak) install,
    /// tried only after the CDN attempt above has already failed. `None` on
    /// any failure (Sober not installed, never run, or the file absent from
    /// its APK too) so the caller keeps surfacing the original CDN error
    /// rather than a confusing Sober-specific one.
    fn fetch_from_sober(&self, path: &str) -> Option<Vec<u8>> {
        if !Sober::is_installed() {
            return None;
        }
        Sober::new()?.extract_texture(path).ok()
    }

    fn fetch_from_package(
        &self,
        version: &str,
        package: &'static str,
        path: &str,
    ) -> Result<Vec<u8>, AssetError> {
        let zip_bytes = self.package_zip_bytes(version, package)?;
        extract_file_from_zip(&zip_bytes, path)
    }

    fn package_zip_bytes(&self, version: &str, package: &str) -> Result<Vec<u8>, AssetError> {
        let cache_path = self.packages_dir.join(format!("{version}-{package}"));
        if let Ok(bytes) = std::fs::read(&cache_path) {
            return Ok(bytes);
        }
        // Note: `version` already carries the "version-" prefix returned by
        // versionQTStudio; don't prepend another one here.
        let url = format!("{SETUP_CDN}/{version}-{package}");
        let bytes = http_get_bytes(&url)?;
        write_atomic(&cache_path, &bytes)?;
        Ok(bytes)
    }
}

/// Maps a `rbxasset://` path's top-level directory to the Studio content
/// package(s) that can contain it, in the order they should be tried.
fn package_candidates_for_path(path: &str) -> Option<Vec<&'static str>> {
    let top_level = path.split('/').next().unwrap_or("");
    match top_level {
        "textures" => Some(vec!["content-textures2.zip", "content-textures3.zip"]),
        // Studio's default skybox panels (`sky512_*.tex`) live in the textures
        // package under a `sky\` entry prefix, not in content-sky.zip with the
        // sun/moon (verified against the packages themselves); try the latter
        // first since it holds the common case.
        "sky" => Some(vec!["content-sky.zip", "content-textures3.zip"]),
        "fonts" => Some(vec!["content-fonts.zip"]),
        "sounds" => Some(vec!["content-sounds.zip"]),
        "models" => Some(vec!["content-models.zip"]),
        _ => None,
    }
}

/// Extracts a file from a Studio content package.
///
/// Packages are rooted at the `rbxasset://` directory (not `sky/` within the
/// zip), with nested entries using backslash separators. Fallbacks handle
/// slash-separated and `content/`-prefixed paths in case Roblox changes the
/// convention.
fn extract_file_from_zip(zip_bytes: &[u8], path: &str) -> Result<Vec<u8>, AssetError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| AssetError::Zip(e.to_string()))?;

    let remainder = path.split_once('/').map(|(_, rest)| rest).unwrap_or(path);
    let candidates = [
        remainder.replace('/', "\\"),
        remainder.to_string(),
        path.to_string(),
        // Some packages (e.g. content-textures3.zip's `sky\` entries) keep
        // the rbxasset directory itself as a backslash-joined path segment,
        // rather than rooting the zip at that directory like content-sky.zip.
        path.replace('/', "\\"),
        format!("content/{path}"),
    ];
    for candidate in candidates {
        if let Ok(mut entry) = archive.by_name(&candidate) {
            let mut buf = Vec::new();
            entry
                .read_to_end(&mut buf)
                .map_err(|e| AssetError::Zip(e.to_string()))?;
            return Ok(buf);
        }
    }
    Err(AssetError::NativeFileNotFound(path.to_string()))
}

fn fetch_studio_version() -> Result<String, AssetError> {
    let body = http_get_text(VERSION_ENDPOINT)?;
    Ok(body.trim().to_string())
}

fn http_get_text(url: &str) -> Result<String, AssetError> {
    let mut response = ureq::get(url)
        .call()
        .map_err(|e| AssetError::Network(e.to_string()))?;
    response
        .body_mut()
        .read_to_string()
        .map_err(|e| AssetError::Network(e.to_string()))
}

fn http_get_bytes(url: &str) -> Result<Vec<u8>, AssetError> {
    let mut response = ureq::get(url)
        .call()
        .map_err(|e| AssetError::Network(e.to_string()))?;
    check_content_length(url, response.headers())?;
    // ureq caps in-memory body reads at 10 MiB by default: content packages
    // are typically 20-50 MiB, so we override that limit with our safety check.
    response
        .body_mut()
        .with_config()
        .limit(MAX_PACKAGE_DOWNLOAD_BYTES)
        .read_to_vec()
        .map_err(|e| AssetError::Network(e.to_string()))
}

fn check_content_length(url: &str, headers: &ureq::http::HeaderMap) -> Result<(), AssetError> {
    let Some(len) = headers
        .get(ureq::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    else {
        return Ok(());
    };
    if len > MAX_PACKAGE_DOWNLOAD_BYTES {
        return Err(AssetError::PackageTooLarge {
            package: url.to_string(),
            size_mb: len as f64 / (1024.0 * 1024.0),
            limit_mb: MAX_PACKAGE_DOWNLOAD_BYTES / (1024 * 1024),
        });
    }
    Ok(())
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), AssetError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| AssetError::Network(e.to_string()))?;
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp_path = parent.join(format!("{file_name}.tmp-{}", std::process::id()));
    std::fs::write(&tmp_path, bytes).map_err(|e| AssetError::Network(e.to_string()))?;
    std::fs::rename(&tmp_path, path).map_err(|e| AssetError::Network(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_textures_to_both_packages_in_order() {
        assert_eq!(
            package_candidates_for_path("textures/SpawnLocation.png"),
            Some(vec!["content-textures2.zip", "content-textures3.zip"])
        );
    }

    #[test]
    fn maps_sky_fonts_sounds_models() {
        assert_eq!(
            package_candidates_for_path("sky/sun.jpg"),
            Some(vec!["content-sky.zip", "content-textures3.zip"])
        );
        assert_eq!(
            package_candidates_for_path("fonts/foo.ttf"),
            Some(vec!["content-fonts.zip"])
        );
        assert_eq!(
            package_candidates_for_path("sounds/foo.ogg"),
            Some(vec!["content-sounds.zip"])
        );
        assert_eq!(
            package_candidates_for_path("models/foo.mesh"),
            Some(vec!["content-models.zip"])
        );
    }

    #[test]
    fn unknown_top_level_directory_has_no_candidates() {
        assert_eq!(package_candidates_for_path("unknown/foo.bin"), None);
    }

    fn build_test_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut writer = zip::ZipWriter::new(Cursor::new(&mut buf));
            let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for (name, data) in entries {
                writer.start_file(*name, options).unwrap();
                std::io::Write::write_all(&mut writer, data).unwrap();
            }
            writer.finish().unwrap();
        }
        buf
    }

    /// This is the layout actually observed in `content-sky.zip`: the
    /// package root is the `sky/` directory, so the entry is just `sun.jpg`.
    #[test]
    fn extracts_package_root_entry() {
        let zip = build_test_zip(&[("sun.jpg", b"jpegdata")]);
        assert_eq!(
            extract_file_from_zip(&zip, "sky/sun.jpg").unwrap(),
            b"jpegdata"
        );
    }

    /// Observed in `content-models.zip`: nested paths use a backslash, not a
    /// forward slash.
    #[test]
    fn extracts_backslash_nested_entry() {
        let zip = build_test_zip(&[("AnimationEditor\\Gui.rbxm", b"rbxmdata")]);
        assert_eq!(
            extract_file_from_zip(&zip, "models/AnimationEditor/Gui.rbxm").unwrap(),
            b"rbxmdata"
        );
    }

    #[test]
    fn extracts_direct_path_entry() {
        let zip = build_test_zip(&[("sky/sun.jpg", b"jpegdata")]);
        assert_eq!(
            extract_file_from_zip(&zip, "sky/sun.jpg").unwrap(),
            b"jpegdata"
        );
    }

    /// Observed in content-textures3.zip: the default skybox panels sit under
    /// a `sky\` entry prefix that matches the `rbxasset://` path's own
    /// directory segment, unlike content-sky.zip which roots at it instead.
    #[test]
    fn extracts_entry_keeping_the_directory_segment_backslash_joined() {
        let zip = build_test_zip(&[("sky\\sky512_up.tex", b"ddsdata")]);
        assert_eq!(
            extract_file_from_zip(&zip, "sky/sky512_up.tex").unwrap(),
            b"ddsdata"
        );
    }

    #[test]
    fn extracts_content_prefixed_entry() {
        let zip = build_test_zip(&[("content/sky/sun.jpg", b"jpegdata")]);
        assert_eq!(
            extract_file_from_zip(&zip, "sky/sun.jpg").unwrap(),
            b"jpegdata"
        );
    }

    #[test]
    fn missing_entry_is_reported() {
        let zip = build_test_zip(&[("sky/moon.jpg", b"x")]);
        let err = extract_file_from_zip(&zip, "sky/sun.jpg");
        assert!(matches!(err, Err(AssetError::NativeFileNotFound(_))));
    }

    #[test]
    fn content_length_under_limit_is_accepted() {
        let mut headers = ureq::http::HeaderMap::new();
        headers.insert(ureq::http::header::CONTENT_LENGTH, "1024".parse().unwrap());
        assert!(check_content_length("http://example.test/pkg.zip", &headers).is_ok());
    }

    #[test]
    fn content_length_over_limit_is_rejected() {
        let mut headers = ureq::http::HeaderMap::new();
        let too_big = MAX_PACKAGE_DOWNLOAD_BYTES + 1;
        headers.insert(
            ureq::http::header::CONTENT_LENGTH,
            too_big.to_string().parse().unwrap(),
        );
        let err = check_content_length("http://example.test/pkg.zip", &headers);
        assert!(matches!(err, Err(AssetError::PackageTooLarge { .. })));
    }

    #[test]
    fn missing_content_length_is_accepted() {
        let headers = ureq::http::HeaderMap::new();
        assert!(check_content_length("http://example.test/pkg.zip", &headers).is_ok());
    }
}
