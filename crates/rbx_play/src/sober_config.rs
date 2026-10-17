//! Session-only changes to Sober's own `config.json`.
//!
//! Sober reads `touch_mode`, `use_console_experience` and
//! `allow_gamepad_permission` from this file at start, and has no command
//! line to override them, so a Play session that needs one writes it before
//! launching and puts the person's file back once the client has gone.
//! The original bytes are kept next to it (`config.json.rbx-native`), so a
//! crashed editor leaves something [`SoberConfigOverlay::apply`] restores on
//! the next launch rather than a changed config for good.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Map, Value};

use crate::launch::SOBER_APP_ID;

/// How long a restore waits for Sober to exit, so the client's own write of
/// its settings on the way out cannot land after the restore.
const EXIT_WAIT: Duration = Duration::from_secs(10);
const EXIT_POLL: Duration = Duration::from_millis(250);
const BACKUP_SUFFIX: &str = "rbx-native";

/// `$HOME/.var/app/org.vinegarhq.Sober/config/sober/config.json`.
pub fn default_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(
        Path::new(&home)
            .join(".var/app")
            .join(SOBER_APP_ID)
            .join("config/sober/config.json"),
    )
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".");
    name.push(BACKUP_SUFFIX);
    PathBuf::from(name)
}

/// Splits Sober's file into its leading `//` comment block and the JSON
/// object after it (Sober writes a warning header that is not valid JSON).
pub fn split_header(text: &str) -> (&str, &str) {
    let mut end = 0;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("//") || line.trim().is_empty() {
            end += line.len();
        } else {
            break;
        }
    }
    text.split_at(end)
}

/// `text` with `changes` set on its top-level object, header kept. An
/// unreadable body is replaced by an object holding only `changes`.
pub fn merged(text: &str, changes: &[(String, Value)]) -> String {
    let (header, body) = split_header(text);
    let mut object = match serde_json::from_str::<Value>(body) {
        Ok(Value::Object(object)) => object,
        _ => Map::new(),
    };
    for (key, value) in changes {
        object.insert(key.clone(), value.clone());
    }
    let body = serde_json::to_string_pretty(&Value::Object(object)).unwrap_or_default();
    format!("{header}{body}\n")
}

/// A changed `config.json` waiting to be put back.
#[derive(Debug)]
pub struct SoberConfigOverlay {
    path: PathBuf,
    /// The person's bytes, or `None` when there was no file.
    original: Option<Vec<u8>>,
    restored: bool,
}

impl SoberConfigOverlay {
    /// Restores a backup a crashed session left, then writes `changes` over
    /// the file at `path`. Nothing is written for an empty `changes`.
    pub fn apply(path: &Path, changes: &[(String, Value)]) -> io::Result<Option<Self>> {
        recover(path)?;
        if changes.is_empty() {
            return Ok(None);
        }
        let original = match fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(err) if err.kind() == io::ErrorKind::NotFound => None,
            Err(err) => return Err(err),
        };
        if let Some(bytes) = &original {
            fs::write(backup_path(path), bytes)?;
        }
        let text = original
            .as_deref()
            .map(String::from_utf8_lossy)
            .unwrap_or_default();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, merged(&text, changes))?;
        Ok(Some(SoberConfigOverlay {
            path: path.to_path_buf(),
            original,
            restored: false,
        }))
    }

    /// Puts the person's file back now (or removes the one this created).
    pub fn restore(&mut self) -> io::Result<()> {
        if self.restored {
            return Ok(());
        }
        match &self.original {
            Some(bytes) => fs::write(&self.path, bytes)?,
            None => match fs::remove_file(&self.path) {
                Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
                _ => {}
            },
        }
        match fs::remove_file(backup_path(&self.path)) {
            Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(err),
            _ => {}
        }
        self.restored = true;
        Ok(())
    }

    /// Restores on a background thread once `flatpak ps` stops listing
    /// Sober (or after 10 s), so the UI thread never waits on the client.
    pub fn restore_after_exit(mut self) {
        thread::spawn(move || {
            let deadline = Instant::now() + EXIT_WAIT;
            while sober_running() && Instant::now() < deadline {
                thread::sleep(EXIT_POLL);
            }
            let _ = self.restore();
        });
    }
}

impl Drop for SoberConfigOverlay {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

/// Puts back a backup left by a session that never restored.
pub fn recover(path: &Path) -> io::Result<()> {
    let backup = backup_path(path);
    match fs::read(&backup) {
        Ok(bytes) => {
            fs::write(path, bytes)?;
            fs::remove_file(backup)
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

fn sober_running() -> bool {
    Command::new("flatpak")
        .args(["ps", "--columns=application"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(SOBER_APP_ID))
}
