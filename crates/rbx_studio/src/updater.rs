//! The auto-updater. At launch, a background look at the latest GitHub
//! release (`chteau/rbx-native`); a newer one is downloaded, checked against
//! the release's minisign-signed `SHA256SUMS`, and swapped in over this
//! executable, so the next launch runs it and says so ("Updated to vX.Y.Z",
//! see [`toast`]).
//!
//! Never in the way: it runs off the main thread, says one line on stderr
//! when offline or rate limited, and stays off where something else owns
//! the install: a Flatpak (Flathub updates it), a folder this user can't
//! write (a distro package), or a dev build (no release version compiled
//! in, see [`current`]).
//!
//! Settings › Files & recovery › Updates has the switch (stored in
//! `<config>/updates.json`) and Check now.

mod compat;
mod install;
mod release;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_kinds;
mod toast;
mod verify;

pub(crate) use toast::toast;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The release this binary is: the release workflow builds with
/// `RBX_NATIVE_RELEASE=<tag version>` (checked against Cargo.toml there).
/// `None` in any other build, which never updates itself.
pub(crate) fn current() -> Option<Version> {
    option_env!("RBX_NATIVE_RELEASE").and_then(|version| Version::parse(version).ok())
}

/// The GitHub API root; `RBX_UPDATE_API` points it at a local mock.
fn api_root() -> String {
    std::env::var("RBX_UPDATE_API")
        .ok()
        .filter(|root| !root.is_empty())
        .unwrap_or_else(|| "https://api.github.com".to_owned())
}

/// Why this copy doesn't update itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Blocked {
    DevBuild,
    Flatpak,
    ReadOnly(PathBuf),
    Platform,
}

impl Blocked {
    pub(crate) fn reason(&self) -> String {
        match self {
            Blocked::DevBuild => "A development build doesn\u{2019}t update itself.".to_owned(),
            Blocked::Flatpak => "Installed from Flathub, which keeps it up to date.".to_owned(),
            Blocked::ReadOnly(dir) => format!(
                "{} isn\u{2019}t writable, so whatever installed RbxNative there updates it.",
                dir.display()
            ),
            Blocked::Platform => "There are no release builds for this platform.".to_owned(),
        }
    }
}

/// Whether, and why not, this copy can update itself. Found once: none of
/// it changes while the editor runs, and Settings asks on every frame.
pub(crate) fn blocked() -> Option<Blocked> {
    static BLOCKED: OnceLock<Option<Blocked>> = OnceLock::new();
    BLOCKED.get_or_init(find_blocked).clone()
}

fn find_blocked() -> Option<Blocked> {
    if current().is_none() {
        return Some(Blocked::DevBuild);
    }
    if install::in_flatpak(std::env::var_os("FLATPAK_ID"), Path::new("/.flatpak-info")) {
        return Some(Blocked::Flatpak);
    }
    if release::TARGET.is_none() {
        return Some(Blocked::Platform);
    }
    match install::Location::current() {
        Ok(location) if install::writable(location.dir()) => None,
        Ok(location) => Some(Blocked::ReadOnly(location.dir().to_owned())),
        Err(_) => Some(Blocked::ReadOnly(PathBuf::from("The install folder"))),
    }
}

/// A release swapped in for the next launch: what its toast names and
/// links to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Installed {
    pub(crate) version: String,
    pub(crate) notes: String,
}

/// `<config>/updates.json`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Prefs {
    #[serde(default = "yes")]
    automatic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    installed: Option<Installed>,
}

fn yes() -> bool {
    true
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            automatic: true,
            installed: None,
        }
    }
}

impl Prefs {
    fn path() -> Option<PathBuf> {
        crate::settings::default_config_dir().map(|dir| dir.join("updates.json"))
    }

    fn load() -> Prefs {
        Self::path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    fn update(change: impl FnOnce(&mut Prefs)) -> Result<(), String> {
        // The Settings switch and the background check both write here, so
        // the read-change-write must not interleave.
        static WRITING: Mutex<()> = Mutex::new(());
        let _writing = WRITING.lock().unwrap_or_else(|e| e.into_inner());
        let mut prefs = Self::load();
        change(&mut prefs);
        let path = Self::path().ok_or("no config folder")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|error| error.to_string())?;
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&prefs).map_err(|error| error.to_string())?;
        std::fs::write(&tmp, text).map_err(|error| error.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|error| error.to_string())
    }
}

/// The switch as last read or set, so Settings doesn't read the file on
/// every frame.
static AUTOMATIC: Mutex<Option<bool>> = Mutex::new(None);

/// Whether the launch check is on.
pub(crate) fn automatic() -> bool {
    *AUTOMATIC
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(|| Prefs::load().automatic)
}

pub(crate) fn set_automatic(on: bool) -> Result<(), String> {
    Prefs::update(|prefs| prefs.automatic = on)?;
    *AUTOMATIC.lock().unwrap_or_else(|e| e.into_inner()) = Some(on);
    Ok(())
}

/// What the last check found, for the Settings row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    Idle,
    Checking,
    UpToDate(Version),
    Installed(Version),
    Failed(String),
}

static STATUS: Mutex<Status> = Mutex::new(Status::Idle);
static RUNNING: AtomicBool = AtomicBool::new(false);
/// The update this launch is the first run of, until its toast takes it.
static NOTICE: Mutex<Option<Installed>> = Mutex::new(None);

pub(crate) fn status() -> Status {
    STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

fn set_status(status: Status) {
    *STATUS.lock().unwrap_or_else(|e| e.into_inner()) = status;
}

fn take_notice() -> Option<Installed> {
    NOTICE.lock().unwrap_or_else(|e| e.into_inner()).take()
}

/// At launch, before any window: clears what the last swap left behind,
/// picks up the update this launch is the first run of, and starts the
/// background check when it's on.
pub(crate) fn startup() {
    let Some(current) = current() else {
        return;
    };
    if let Ok(location) = install::Location::current() {
        location.clear_leftovers();
        #[cfg(unix)]
        if let (install::Location::Binary(exe), None) = (&location, blocked()) {
            if let Some(new) = compat::migrate_legacy_name(exe) {
                eprintln!("rbxstudio: renamed {} to {}", exe.display(), new.display());
            }
        }
    }
    let prefs = Prefs::load();
    if let Some(installed) = prefs.installed {
        if Version::parse(&installed.version).ok() == Some(current) {
            *NOTICE.lock().unwrap_or_else(|e| e.into_inner()) = Some(installed);
        }
        if let Err(error) = Prefs::update(|prefs| prefs.installed = None) {
            eprintln!("rbxstudio: could not save updates.json: {error}");
        }
    }
    if prefs.automatic && blocked().is_none() && begin() {
        let spawned = std::thread::Builder::new()
            .name("update check".to_owned())
            .spawn(|| {
                finish();
            });
        if spawned.is_err() {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }
}

/// Starts a check unless one is already running (`false` then). The check
/// itself is [`finish`], on a background thread.
/// Whether a check started by [`begin`] hasn't finished yet.
pub(crate) fn running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

pub(crate) fn begin() -> bool {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return false;
    }
    set_status(Status::Checking);
    true
}

/// The check [`begin`] started: blocking, network and disk.
pub(crate) fn finish() -> Status {
    let status = match blocked() {
        Some(blocked) => Status::Failed(blocked.reason()),
        None => run().unwrap_or_else(|error| {
            eprintln!("rbxstudio: update check failed: {error}");
            Status::Failed(error)
        }),
    };
    set_status(status.clone());
    RUNNING.store(false, Ordering::SeqCst);
    status
}

fn run() -> Result<Status, String> {
    let current = current().ok_or("not a release build")?;
    let agent = agent();
    let latest = release::Release::parse(&get_text(
        &agent,
        &format!("{}/repos/{}/releases/latest", api_root(), release::REPO),
    )?)?;
    let version = latest.version()?;
    if !latest.is_newer_than(&current)? {
        return Ok(Status::UpToDate(current));
    }
    if Prefs::load()
        .installed
        .is_some_and(|installed| installed.version == version.to_string())
    {
        return Ok(Status::Installed(version));
    }
    let target = release::TARGET.ok_or("no release builds for this platform")?;
    let location = install::Location::current()?;
    let package = latest.asset(&location.package_name(&version, target))?;
    let sums = get_text(&agent, &latest.asset(release::SUMS)?.browser_download_url)?;
    let signature = get_text(
        &agent,
        &latest.asset(release::SIGNATURE)?.browser_download_url,
    )?;
    verify::signature(verify::PUBLIC_KEY, sums.as_bytes(), &signature)?;
    let expected = verify::sum_for(&sums, &package.name)?;

    let staging = install::Staging::new(location.dir())?;
    let archive = staging.path().join(&package.name);
    let actual = download(&agent, &package.browser_download_url, &archive)?;
    if actual != expected {
        return Err(format!(
            "{} does not match its signed checksum",
            package.name
        ));
    }
    location.install(&archive, staging.path())?;
    #[cfg(windows)]
    compat::record_version(location.dir(), &version);
    Prefs::update(|prefs| {
        prefs.installed = Some(Installed {
            version: version.to_string(),
            notes: release::notes_page(&latest.html_url),
        })
    })?;
    eprintln!("rbxstudio: v{version} is installed and runs from the next launch");
    Ok(Status::Installed(version))
}

/// A package bigger than this is not ours, whatever the server says.
const MAX_PACKAGE: u64 = 512 << 20;

/// Every request, the package download included, ends within
/// `timeout_global`: a connection that stalls mid-body would otherwise hold
/// the check (and its "Checking…" row) for the rest of the session.
fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("rbxstudio/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_global(Some(Duration::from_secs(20 * 60)))
        .build()
        .into()
}

/// A small text body: the release JSON, SHA256SUMS, its signature.
fn get_text(agent: &ureq::Agent, url: &str) -> Result<String, String> {
    agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|error| format!("{url}: {error}"))?
        .body_mut()
        .with_config()
        .limit(1 << 20)
        .read_to_string()
        .map_err(|error| format!("{url}: {error}"))
}

/// Streams `url` into `path`, returning the body's SHA-256 in hex.
fn download(agent: &ureq::Agent, url: &str, path: &Path) -> Result<String, String> {
    let failed = |error: &dyn std::fmt::Display| format!("{url}: {error}");
    let mut response = agent.get(url).call().map_err(|e| failed(&e))?;
    let mut reader = response.body_mut().as_reader();
    let mut file = std::fs::File::create(path).map_err(|e| failed(&e))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = reader.read(&mut buffer).map_err(|e| failed(&e))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_PACKAGE {
            return Err(failed(&format!("bigger than {} MiB", MAX_PACKAGE >> 20)));
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read]).map_err(|e| failed(&e))?;
    }
    file.sync_all().map_err(|e| failed(&e))?;
    Ok(verify::hex(&hasher.finalize()))
}
