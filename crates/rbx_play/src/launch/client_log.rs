//! What the client says on stderr, and what the editor learns from it before
//! any window appears: Sober logs the remote manifest it starts with, and a
//! `fatal…` startup treatment there means it will only show a "Gameplay
//! Unavailable" dialog instead of joining.

use std::io::{BufRead, BufReader, Read};
use std::process::Child;
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use super::Launcher;

const TREATMENT_KEY: &str = "\"startup_message_treatment\":";

/// The `startup_message_treatment` of a Sober manifest log line, when it is a
/// fatal one, e.g. `fatal_v1__update_pending` from
/// `info: state: using manifest object: {…,"startup_message_treatment":"fatal_v1__update_pending",…}`.
pub fn fatal_treatment(line: &str) -> Option<&str> {
    let rest = line[line.find(TREATMENT_KEY)? + TREATMENT_KEY.len()..].trim_start();
    let value = rest.strip_prefix('"')?;
    let value = &value[..value.find('"')?];
    value.starts_with("fatal").then_some(value)
}

/// A shareable view of a launched client, for threads other than the one
/// owning its [`super::ClientProcess`] (such as one waiting for its window).
#[derive(Clone)]
pub struct ClientWatch {
    launcher: Launcher,
    child: Arc<Mutex<Child>>,
    unavailable: Arc<OnceLock<String>>,
}

impl ClientWatch {
    /// Takes `child`'s piped stderr, if any, and reads it to the end on its
    /// own thread, so the client never blocks on a full pipe.
    pub(super) fn new(launcher: Launcher, mut child: Child) -> Self {
        let unavailable = Arc::new(OnceLock::new());
        if let Some(stderr) = child.stderr.take() {
            let found = Arc::clone(&unavailable);
            let _ = std::thread::Builder::new()
                .name("client-stderr".into())
                .spawn(move || read_log(stderr, &found));
        }
        ClientWatch {
            launcher,
            child: Arc::new(Mutex::new(child)),
            unavailable,
        }
    }

    /// The client process exited, and the launcher tracks the client.
    pub fn exited(&self) -> bool {
        self.launcher.tracks_client() && matches!(self.child().try_wait(), Ok(Some(_)))
    }

    /// The fatal startup treatment the client logged: it cannot join games.
    pub fn unavailable(&self) -> Option<String> {
        self.unavailable.get().cloned()
    }

    pub(super) fn child(&self) -> std::sync::MutexGuard<'_, Child> {
        self.child.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn read_log(stderr: impl Read, found: &OnceLock<String>) {
    for line in BufReader::new(stderr).split(b'\n') {
        let Ok(line) = line else { return };
        if let Some(treatment) = fatal_treatment(&String::from_utf8_lossy(&line)) {
            let _ = found.set(treatment.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fatal_treatment;

    #[test]
    fn finds_the_fatal_treatment_in_sobers_manifest_line() {
        let line = r#"info: state: using manifest object: {"app_settings":{"FFlagUIGpuGradient11":false},"r1":1,"startup_message_treatment":"fatal_v1__update_pending","u":1}"#;
        assert_eq!(fatal_treatment(line), Some("fatal_v1__update_pending"));
    }

    #[test]
    fn ignores_other_treatments_and_lines() {
        let fine = r#"info: state: using manifest object: {"r1":1,"startup_message_treatment":"none","u":1}"#;
        assert_eq!(fatal_treatment(fine), None);
        assert_eq!(
            fatal_treatment("info: app: lifecycle: fs_init delta=131.4ms"),
            None
        );
        assert_eq!(
            fatal_treatment(r#"{"startup_message_treatment":"fatal"#),
            None
        );
    }
}
