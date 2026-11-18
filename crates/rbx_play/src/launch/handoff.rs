//! Windows: `cmd /C start roblox://…` returns at once, so the launcher's own
//! exit says nothing about the client. Whether a Roblox process is still
//! around does: once one showed up and none is left, or none ever showed up,
//! the client is gone (Roblox's own launcher failing with a 403 dialog, say,
//! and the dialog closed).

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

/// How long the shell hand-off may take to start any Roblox process.
const GRACE: Duration = Duration::from_secs(30);
/// The editor polls more often than this; a process scan is not free.
const RECHECK: Duration = Duration::from_secs(1);

/// Lowercased image names that count as the client still starting or
/// running: the player, its launcher and installer, and the bootstrappers
/// people register for `roblox://` instead.
#[cfg_attr(not(windows), allow(dead_code))]
const ROBLOX_EXES: &[&str] = &[
    "robloxplayerbeta.exe",
    "robloxplayerlauncher.exe",
    "robloxplayerinstaller.exe",
    "bloxstrap.exe",
    "fishstrap.exe",
];

pub(super) struct HandOff {
    started: Instant,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    seen: bool,
    checked: Option<Instant>,
    gone: bool,
}

impl HandOff {
    pub(super) fn new() -> Self {
        HandOff {
            started: Instant::now(),
            state: Mutex::new(State::default()),
        }
    }

    /// No Roblox process is left (see the module doc). Rescans at most
    /// once per [`RECHECK`].
    pub(super) fn gone(&self) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.checked.is_some_and(|at| at.elapsed() < RECHECK) {
            return state.gone;
        }
        state.checked = Some(Instant::now());
        let running = roblox_running();
        state.seen |= running;
        state.gone = decide(running, state.seen, self.started.elapsed());
        state.gone
    }
}

fn decide(running: bool, seen: bool, since_start: Duration) -> bool {
    !running && (seen || since_start >= GRACE)
}

#[cfg(windows)]
fn roblox_running() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    // SAFETY: a snapshot handle is checked, walked with a correctly sized
    // entry and closed once.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            // Can't tell: never end a session on a failed scan.
            return true;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut more = Process32FirstW(snapshot, &mut entry) != 0;
        while more && !found {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]).to_ascii_lowercase();
            found = ROBLOX_EXES.contains(&name.as_str());
            more = Process32NextW(snapshot, &mut entry) != 0;
        }
        CloseHandle(snapshot);
        found
    }
}

/// Only Windows hands off through the shell.
#[cfg(not(windows))]
fn roblox_running() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gone_once_seen_and_nothing_left() {
        assert!(decide(false, true, Duration::from_secs(2)));
        assert!(!decide(true, true, Duration::from_secs(2)));
    }

    #[test]
    fn waits_out_the_grace_before_anything_showed_up() {
        assert!(!decide(false, false, Duration::from_secs(5)));
        assert!(decide(false, false, GRACE));
    }
}
