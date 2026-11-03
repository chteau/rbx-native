//! The GitHub release the updater reads, and which of its files is ours.
//! The names are the release workflow's (`.github/workflows/release.yml`).

use semver::Version;
use serde::Deserialize;

pub(super) const REPO: &str = "chteau/rbx-native";
pub(super) const SUMS: &str = "SHA256SUMS";
pub(super) const SIGNATURE: &str = "SHA256SUMS.minisig";

/// The target triple this binary was built for, among the three the
/// release workflow builds.
pub(super) const TARGET: Option<&str> = if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
    Some("x86_64-unknown-linux-gnu")
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("aarch64-apple-darwin")
} else if cfg!(all(windows, target_arch = "x86_64")) {
    Some("x86_64-pc-windows-msvc")
} else {
    None
};

/// The executable's name inside a package.
pub(super) const EXE_NAME: &str = if cfg!(windows) {
    "rbxstudio.exe"
} else {
    "rbxstudio"
};

#[derive(Debug, Deserialize)]
pub(super) struct Release {
    pub(super) tag_name: String,
    pub(super) html_url: String,
    #[serde(default)]
    pub(super) prerelease: bool,
    #[serde(default)]
    pub(super) draft: bool,
    #[serde(default)]
    pub(super) assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Asset {
    pub(super) name: String,
    pub(super) browser_download_url: String,
}

impl Release {
    pub(super) fn parse(json: &str) -> Result<Release, String> {
        serde_json::from_str(json).map_err(|error| format!("unreadable release: {error}"))
    }

    pub(super) fn version(&self) -> Result<Version, String> {
        parse_tag(&self.tag_name)
    }

    /// Whether this is a published, non-prerelease version above `current`.
    pub(super) fn is_newer_than(&self, current: &Version) -> Result<bool, String> {
        let version = self.version()?;
        Ok(!self.prerelease && !self.draft && version.pre.is_empty() && version > *current)
    }

    pub(super) fn asset(&self, name: &str) -> Result<&Asset, String> {
        self.assets
            .iter()
            .find(|asset| asset.name == name)
            .ok_or_else(|| format!("release {} has no {name}", self.tag_name))
    }

    /// The package for `target`.
    pub(super) fn package(&self, version: &Version, target: &str) -> Result<&Asset, String> {
        self.asset(&package_name(version, target))
    }
}

pub(super) fn package_name(version: &Version, target: &str) -> String {
    let extension = if target.contains("windows") {
        "zip"
    } else {
        "tar.gz"
    };
    format!("rbxstudio-{version}-{target}.{extension}")
}

/// `v0.2.0` or `0.2.0`.
pub(super) fn parse_tag(tag: &str) -> Result<Version, String> {
    Version::parse(tag.strip_prefix('v').unwrap_or(tag))
        .map_err(|error| format!("release tag {tag} is not a version: {error}"))
}
