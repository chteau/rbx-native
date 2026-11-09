//! Keeps a Flatpak client current. Sober stops running once Roblox moves on
//! and ships refreshes under an unchanged version number, so what is compared
//! is the installed commit against the one on the client's remote.

use std::io::Read;
use std::process::{Child, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use super::{command_for, Launcher};

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const POLL: Duration = Duration::from_millis(50);

impl Launcher {
    /// Whether this client's remote has a newer build. False for a client
    /// that isn't a Flatpak and when the answer can't be had (offline, a
    /// remote that is gone). Blocking.
    pub fn update_available(self) -> bool {
        let Some(app) = self.app_id() else {
            return false;
        };
        let Some(scope) = installation(app) else {
            return false;
        };
        let read = |args: &[&str]| flatpak(args, CHECK_TIMEOUT).ok();
        let origin = read(&["info", scope, "--show-origin", app]);
        let installed = read(&["info", scope, "--show-commit", app]);
        let (Some(origin), Some(installed)) = (origin, installed) else {
            return false;
        };
        read(&["remote-info", scope, "--show-commit", origin.trim(), app])
            .is_some_and(|latest| is_newer(&installed, &latest))
    }

    /// Installs the newest build of this client. Blocking, for as long as
    /// the download takes.
    pub fn update(self) -> Result<(), String> {
        let app = self.app_id().ok_or("it isn't a Flatpak app")?;
        let scope = installation(app).ok_or("it isn't installed")?;
        let args = ["update", scope, "--noninteractive", "-y", app];
        flatpak(&args, INSTALL_TIMEOUT).map(drop)
    }

    /// The step Play shows during [`Launcher::update`].
    pub fn updating_step(self) -> &'static str {
        match self {
            Launcher::Roseline => "Updating Roséline\u{2026}",
            _ => "Updating Sober\u{2026}",
        }
    }
}

/// `--user` or `--system`, whichever holds `app`. Without one, `flatpak` asks
/// which to use when a remote exists in both.
fn installation(app: &str) -> Option<&'static str> {
    ["--user", "--system"]
        .into_iter()
        .find(|scope| flatpak(&["info", scope, app], CHECK_TIMEOUT).is_ok())
}

fn is_newer(installed: &str, latest: &str) -> bool {
    let (installed, latest) = (installed.trim(), latest.trim());
    !installed.is_empty() && !latest.is_empty() && installed != latest
}

/// Runs `flatpak` with `args`, returning what it printed or why it failed.
fn flatpak(args: &[&str], timeout: Duration) -> Result<String, String> {
    let mut child = command_for("flatpak")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| err.to_string())?;
    // Drained on their own threads so a full pipe can't stall the child.
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let succeeded = wait(&mut child, timeout)?;
    let output = stdout.join().unwrap_or_default();
    match succeeded {
        true => Ok(output),
        false => Err(failure(&stderr.join().unwrap_or_default())),
    }
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    })
}

fn wait(child: &mut Child, timeout: Duration) -> Result<bool, String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().map_err(|err| err.to_string())? {
            return Ok(status.success());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("it took too long".to_owned());
        }
        thread::sleep(POLL);
    }
}

/// The last line `flatpak` wrote, without its `error:` prefix.
fn failure(stderr: &str) -> String {
    let line = stderr.lines().rev().find(|line| !line.trim().is_empty());
    match line {
        Some(line) => line.trim().trim_start_matches("error:").trim().to_owned(),
        None => "flatpak gave no reason".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_different_commit_is_an_update_even_under_the_same_version() {
        assert!(is_newer("7b6430c7\n", "a611b863\n"));
    }

    #[test]
    fn the_same_commit_is_not_an_update() {
        assert!(!is_newer("a611b863\n", "a611b863"));
    }

    #[test]
    fn a_missing_answer_is_not_an_update() {
        assert!(!is_newer("", "a611b863"));
        assert!(!is_newer("7b6430c7", "  \n"));
    }

    #[test]
    fn a_failure_is_flatpaks_last_line_without_its_prefix() {
        let stderr = "Looking for updates\nerror: Unable to load summary\n\n";
        assert_eq!(failure(stderr), "Unable to load summary");
        assert_eq!(failure(""), "flatpak gave no reason");
    }

    #[test]
    fn only_flatpak_clients_can_update() {
        assert!(!Launcher::XdgOpen.update_available());
        assert!(Launcher::XdgOpen.update().is_err());
    }
}
