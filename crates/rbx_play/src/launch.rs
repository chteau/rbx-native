//! Opening the published sandbox in the Roblox client.

use std::io;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

/// Everything outside `A-Z a-z 0-9 - _ . ~` is escaped.
const LAUNCH_DATA_SET: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// `roblox://placeId=<id>&launchData=<percent-encoded data>`; `launchData`
/// is omitted when empty. Encodes everything outside `A-Z a-z 0-9 - _ . ~`.
pub fn join_uri(place_id: u64, launch_data: &str) -> String {
    if launch_data.is_empty() {
        return format!("roblox://placeId={place_id}");
    }
    let encoded = utf8_percent_encode(launch_data, LAUNCH_DATA_SET);
    format!("roblox://placeId={place_id}&launchData={encoded}")
}

/// `roblox://experiences/start?placeId=<id>&gameInstanceId=<job id>`: joins
/// that exact server, for a collaborator. `job_id` is percent-encoded.
pub fn join_server_uri(place_id: u64, job_id: &str) -> String {
    let encoded = utf8_percent_encode(job_id, LAUNCH_DATA_SET);
    format!("roblox://experiences/start?placeId={place_id}&gameInstanceId={encoded}")
}

/// Which program opens the URI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launcher {
    /// Linux: `flatpak run org.vinegarhq.Sober <uri>`.
    Sober,
    /// Linux fallback: `xdg-open <uri>`.
    XdgOpen,
    /// Windows: `cmd /C start "" <uri>`.
    WindowsShell,
    /// macOS: `open <uri>`.
    MacOpen,
}

const SOBER_APP_ID: &str = "org.vinegarhq.Sober";
const CLIENT_EXE: &str = "RobloxPlayerBeta.exe";

const CLOSE_TIMEOUT: Duration = Duration::from_secs(5);
const CLOSE_POLL: Duration = Duration::from_millis(200);

impl Launcher {
    /// The launcher for this OS. On Linux: `Sober` when
    /// `flatpak info org.vinegarhq.Sober` exits 0, else `XdgOpen`.
    pub fn detect() -> Self {
        if cfg!(target_os = "windows") {
            Launcher::WindowsShell
        } else if cfg!(target_os = "macos") {
            Launcher::MacOpen
        } else {
            let sober_installed = Command::new("flatpak")
                .args(["info", SOBER_APP_ID])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok_and(|status| status.success());
            if sober_installed {
                Launcher::Sober
            } else {
                Launcher::XdgOpen
            }
        }
    }

    /// The command that opens `uri`; not spawned.
    pub fn command(self, uri: &str) -> Command {
        match self {
            Launcher::Sober => {
                let mut cmd = Command::new("flatpak");
                cmd.args(["run", SOBER_APP_ID, uri]);
                cmd
            }
            Launcher::XdgOpen => {
                let mut cmd = Command::new("xdg-open");
                cmd.arg(uri);
                cmd
            }
            Launcher::WindowsShell => {
                let mut cmd = Command::new("cmd");
                cmd.args(["/C", "start", "", uri]);
                cmd
            }
            Launcher::MacOpen => {
                let mut cmd = Command::new("open");
                cmd.arg(uri);
                cmd
            }
        }
    }

    /// Whether a Roblox client this launcher would start is already running.
    /// Sober: `flatpak ps --columns=application` lists `org.vinegarhq.Sober`.
    /// Windows: `tasklist /FI "IMAGENAME eq RobloxPlayerBeta.exe" /NH` lists it.
    /// Others: false. Errors running the probe count as not running.
    pub fn client_running(self) -> bool {
        let (program, args, needle): (&str, &[&str], &str) = match self {
            Launcher::Sober => ("flatpak", &["ps", "--columns=application"], SOBER_APP_ID),
            Launcher::WindowsShell => (
                "tasklist",
                &["/FI", "IMAGENAME eq RobloxPlayerBeta.exe", "/NH"],
                CLIENT_EXE,
            ),
            Launcher::XdgOpen | Launcher::MacOpen => return false,
        };
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(needle))
    }

    /// Closes that running client so a new one can start (each allows one
    /// instance). Sober: `flatpak kill org.vinegarhq.Sober`. Windows:
    /// `taskkill /IM RobloxPlayerBeta.exe /F`. Then waits up to 5 s for
    /// `client_running` to turn false. Others: Ok without doing anything.
    pub fn close_running_client(self) -> io::Result<()> {
        let mut cmd = match self {
            Launcher::Sober => {
                let mut cmd = Command::new("flatpak");
                cmd.args(["kill", SOBER_APP_ID]);
                cmd
            }
            Launcher::WindowsShell => {
                let mut cmd = Command::new("taskkill");
                cmd.args(["/IM", CLIENT_EXE, "/F"]);
                cmd
            }
            Launcher::XdgOpen | Launcher::MacOpen => return Ok(()),
        };
        // Exit status is ignored: the kill tool reports failure when nothing
        // is running, which is the state the caller wants anyway.
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()?;

        let deadline = Instant::now() + CLOSE_TIMEOUT;
        while self.client_running() {
            if Instant::now() >= deadline {
                return Err(io::Error::from(io::ErrorKind::TimedOut));
            }
            thread::sleep(CLOSE_POLL);
        }
        Ok(())
    }

    /// Whether this launcher's process lives as long as the game client, so
    /// its exit means the client closed. True for `Sober` (`flatpak run`
    /// waits for the app); false for the others, which hand off and return.
    pub fn tracks_client(self) -> bool {
        matches!(self, Launcher::Sober)
    }
}

/// A launched client. The editor polls `exited` (e.g. every 500 ms) and
/// stops the Play session when it returns true; dropping it kills the
/// process when the launcher tracks the client.
pub struct ClientProcess {
    pub launcher: Launcher,
    pub child: std::process::Child,
}

impl ClientProcess {
    /// Spawns `launcher.command(uri)` with stdin null and stdout/stderr null.
    pub fn spawn(launcher: Launcher, uri: &str) -> std::io::Result<Self> {
        let mut cmd = launcher.command(uri);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = cmd.spawn()?;
        Ok(ClientProcess { launcher, child })
    }

    /// `try_wait` reports an exit and the launcher tracks the client.
    pub fn exited(&mut self) -> bool {
        self.launcher.tracks_client() && matches!(self.child.try_wait(), Ok(Some(_)))
    }
}

impl Drop for ClientProcess {
    /// Ends the game client, not only the launcher: `flatpak run` going away
    /// leaves the sandboxed Sober running, and the Windows shell hand-off
    /// returns at once. The kill is fired without waiting for it, since
    /// this runs on the editor's UI thread.
    fn drop(&mut self) {
        if self.launcher.tracks_client() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        let kill = match self.launcher {
            Launcher::Sober => Some(("flatpak", vec!["kill", SOBER_APP_ID])),
            Launcher::WindowsShell => Some(("taskkill", vec!["/IM", "RobloxPlayerBeta.exe", "/F"])),
            Launcher::XdgOpen | Launcher::MacOpen => None,
        };
        if let Some((program, args)) = kill {
            let _ = Command::new(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
    }
}
