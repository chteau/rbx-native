//! Opening the published sandbox in the Roblox client.

use std::io;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use percent_encoding::{utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

use crate::sober_config::SoberConfigOverlay;

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

/// `Command::new(program)`, without the console window Windows would flash
/// for a console program (`cmd`, `tasklist`, `taskkill`) started from a GUI.
pub(crate) fn command_for(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// `CREATE_NO_WINDOW` from `winbase.h`.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub(crate) const SOBER_APP_ID: &str = "org.vinegarhq.Sober";

/// The user's own Flatpak installation when `flatpak` would miss it: it
/// looks under `$XDG_DATA_HOME/flatpak`, so a process given a data home of
/// its own (an isolated test setup) finds no Sober, a user install. `Some`
/// when `FLATPAK_USER_DIR` is unset, the data home's installation has no
/// Sober, and `~/.local/share/flatpak` has it; the editor sets it as
/// `FLATPAK_USER_DIR` at start, which every `flatpak` it runs inherits.
/// Sober itself is checked for, not just the directory: any `flatpak` run
/// with that data home leaves an empty installation behind.
pub fn missed_user_flatpaks() -> Option<std::path::PathBuf> {
    missed_user_installation(
        std::env::var_os("FLATPAK_USER_DIR"),
        std::env::var_os("XDG_DATA_HOME"),
        std::env::var_os("HOME"),
    )
}

fn missed_user_installation(
    user_dir: Option<std::ffi::OsString>,
    data_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<std::path::PathBuf> {
    if user_dir.is_some() {
        return None;
    }
    let sober =
        |installation: &std::path::Path| installation.join("app").join(SOBER_APP_ID).is_dir();
    let data_home = std::path::PathBuf::from(data_home?);
    if sober(&data_home.join("flatpak")) {
        return None;
    }
    let own = std::path::PathBuf::from(home?).join(".local/share/flatpak");
    sober(&own).then_some(own)
}
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
            let sober_installed = command_for("flatpak")
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
        self.command_with(uri, &ClientOptions::default())
    }

    /// [`Launcher::command`] with `options` applied: Sober runs under
    /// `options.wrapper` with `options.flatpak_args` before its app id. The
    /// other launchers hand the URI to the OS and take no options.
    pub fn command_with(self, uri: &str, options: &ClientOptions) -> Command {
        match self {
            Launcher::Sober => {
                let mut cmd = match options.wrapper.split_first() {
                    Some((program, args)) => {
                        let mut cmd = command_for(program);
                        cmd.args(args).arg("flatpak");
                        cmd
                    }
                    None => command_for("flatpak"),
                };
                cmd.arg("run")
                    .args(&options.flatpak_args)
                    .args([SOBER_APP_ID, uri]);
                cmd
            }
            Launcher::XdgOpen => {
                let mut cmd = command_for("xdg-open");
                cmd.arg(uri);
                cmd
            }
            Launcher::WindowsShell => {
                let mut cmd = command_for("cmd");
                cmd.args(["/C", "start", "", uri]);
                cmd
            }
            Launcher::MacOpen => {
                let mut cmd = command_for("open");
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
        command_for(program)
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
                let mut cmd = command_for("flatpak");
                cmd.args(["kill", SOBER_APP_ID]);
                cmd
            }
            Launcher::WindowsShell => {
                let mut cmd = command_for("taskkill");
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

/// How one Play session's client is started, beyond the URI. Only Sober
/// honours these; see [`Launcher::command_with`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ClientOptions {
    /// Keys set in Sober's `config.json` for this session only.
    pub sober_config: Vec<(String, serde_json::Value)>,
    /// `flatpak run` sandbox options, such as `--device=input`.
    pub flatpak_args: Vec<String>,
    /// A program and its arguments that `flatpak` runs under, such as an
    /// `nsenter` into a shaped network namespace.
    pub wrapper: Vec<String>,
}

/// A launched client. The editor polls `exited` (e.g. every 500 ms) and
/// stops the Play session when it returns true; dropping it kills the
/// process when the launcher tracks the client.
pub struct ClientProcess {
    pub launcher: Launcher,
    pub child: std::process::Child,
    overlay: Option<SoberConfigOverlay>,
}

impl ClientProcess {
    /// Spawns `launcher.command(uri)` with stdin null and stdout/stderr null.
    pub fn spawn(launcher: Launcher, uri: &str) -> std::io::Result<Self> {
        Self::spawn_with(launcher, uri, &ClientOptions::default())
    }

    /// [`ClientProcess::spawn`] with `options`. Sober's config changes are
    /// written first and put back after the client exits; a failed spawn
    /// puts them back at once.
    pub fn spawn_with(
        launcher: Launcher,
        uri: &str,
        options: &ClientOptions,
    ) -> std::io::Result<Self> {
        let overlay = match (launcher, crate::sober_config::default_path()) {
            (Launcher::Sober, Some(path)) => {
                SoberConfigOverlay::apply(&path, &options.sober_config)?
            }
            _ => None,
        };
        let mut cmd = launcher.command_with(uri, options);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = cmd.spawn()?;
        Ok(ClientProcess {
            launcher,
            child,
            overlay,
        })
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
            let _ = command_for(program)
                .args(args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
        }
        if let Some(overlay) = self.overlay.take() {
            overlay.restore_after_exit();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{missed_user_installation, SOBER_APP_ID};

    #[test]
    fn an_isolated_data_home_still_finds_the_users_flatpaks() {
        let root = std::env::temp_dir().join(format!("rbx_play_flatpak_{}", std::process::id()));
        let home = root.join("home");
        let isolated = root.join("isolated");
        std::fs::create_dir_all(home.join(".local/share/flatpak/app").join(SOBER_APP_ID)).unwrap();
        // What any `flatpak` run with this data home leaves: an empty
        // installation, which still has no Sober.
        std::fs::create_dir_all(isolated.join("flatpak/repo")).unwrap();
        let find = |user_dir: Option<&str>, data_home: &std::path::Path| {
            missed_user_installation(
                user_dir.map(Into::into),
                Some(data_home.into()),
                Some(home.clone().into()),
            )
        };
        assert_eq!(
            find(None, &isolated),
            Some(home.join(".local/share/flatpak"))
        );
        // Flatpak already finds it, or was told where to look.
        assert_eq!(find(None, &home.join(".local/share")), None);
        assert_eq!(find(Some("/elsewhere"), &isolated), None);
        // No data home set: flatpak's own default is the home one.
        assert_eq!(
            missed_user_installation(None, None, Some(home.clone().into())),
            None
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
