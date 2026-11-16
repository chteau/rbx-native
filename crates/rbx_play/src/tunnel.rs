//! Public HTTPS tunnel to the loopback channel. cloudflared quick tunnels by
//! default, or any command that prints its public URL.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use regex::Regex;

/// A running tunnel; dropping it kills the child process.
pub struct Tunnel {
    pub public_url: String,
    pub child: Child,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        end(&mut self.child);
    }
}

/// Stops the tunnel and reaps it. On macOS that is the watching shell's
/// whole process group (see [`command`]): killing the shell alone would
/// leave cloudflared running under launchd.
fn end(child: &mut Child) {
    #[cfg(target_os = "macos")]
    if let Ok(group) = i32::try_from(child.id()) {
        // SAFETY: a plain signal to the process group the child leads.
        unsafe { libc::kill(-group, libc::SIGTERM) };
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// macOS has no `PR_SET_PDEATHSIG`, and quitting from the menu exits
/// without running `Tunnel`'s `Drop`: a seen-live cloudflared outlived the
/// editor under launchd. So a shell runs it, in a process group of its own,
/// and ends it as soon as the editor (`$0`) is gone. The tunnel inherits the
/// piped stdout and stderr, so its URL is still read as before.
#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
const WATCH: &str = r#"parent=$0; "$@" & tunnel=$!; trap 'kill $tunnel 2>/dev/null' EXIT HUP INT TERM; while kill -0 "$parent" 2>/dev/null && kill -0 $tunnel 2>/dev/null; do sleep 1; done"#;

/// The command that runs `program args`, watched on macOS (see [`WATCH`]).
fn command(program: PathBuf, args: Vec<String>) -> Command {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(WATCH)
            .arg(std::process::id().to_string())
            .arg(program)
            .args(args)
            .process_group(0);
        command
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut command = Command::new(program);
        command.args(args);
        command
    }
}

#[derive(Debug)]
pub enum TunnelError {
    /// The program could not be started (not installed, not executable).
    Spawn(std::io::Error),
    /// It exited before printing a URL; carries its last stderr/stdout lines.
    Exited(String),
    /// No URL within the timeout; the child is killed.
    Timeout,
}

impl std::fmt::Display for TunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TunnelError::Spawn(e) => write!(f, "the tunnel program could not start: {e}"),
            TunnelError::Exited(lines) => {
                write!(f, "the tunnel exited before giving a URL:\n{lines}")
            }
            TunnelError::Timeout => f.write_str("the tunnel gave no URL in time"),
        }
    }
}

impl std::error::Error for TunnelError {}

/// How to start a tunnel to `http://127.0.0.1:<port>`.
#[derive(Debug, Clone)]
pub enum TunnelProvider {
    /// `<binary> tunnel --no-autoupdate --url http://127.0.0.1:<port>`;
    /// URL matches `https://[a-z0-9-]+\.trycloudflare\.com`.
    Cloudflared { binary: PathBuf },
    /// `program args…` with every `{port}` in an arg replaced; the first
    /// match of `url_pattern` on stdout or stderr is the URL.
    Custom {
        program: PathBuf,
        args: Vec<String>,
        url_pattern: String,
    },
}

/// Linux: the kernel sends the tunnel `SIGTERM` when the editor dies, so a
/// crash or a kill (which skip `Tunnel`'s `Drop`) does not leave it running
/// (`PR_SET_PDEATHSIG`; seen live: a killed editor left cloudflared behind).
/// The signal follows the *thread* that spawned the child, so `start` must
/// run on a thread that lives as long as the tunnel (the UI thread or a
/// long-lived worker pool, as the editor's background executor is).
#[cfg(target_os = "linux")]
pub(crate) fn end_with_parent(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    let parent = std::process::id();
    // SAFETY: `prctl` and `getppid` are async-signal-safe, as `pre_exec`
    // requires; nothing is allocated between fork and exec.
    unsafe {
        command.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            // The editor may have died before the prctl took effect (its
            // orphans go to init or a subreaper such as `systemd --user`).
            if u32::try_from(libc::getppid()).ok() != Some(parent) {
                return Err(std::io::Error::other("parent exited"));
            }
            Ok(())
        });
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn end_with_parent(_command: &mut Command) {}

impl TunnelProvider {
    /// Starts the child with stdout+stderr piped, reads both on threads,
    /// and returns once a line matches or `timeout` passes.
    pub fn start(&self, port: u16, timeout: Duration) -> Result<Tunnel, TunnelError> {
        let (program, args) = self.command_line(port);
        let pattern = self.url_pattern();
        let mut command = command(program, args);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        end_with_parent(&mut command);
        // No console window of its own next to the (GUI) editor.
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000);
        let mut child = command.spawn().map_err(TunnelError::Spawn)?;

        let (tx, rx) = mpsc::channel::<String>();
        if let Some(out) = child.stdout.take() {
            spawn_reader(out, tx.clone());
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(err, tx.clone());
        }
        drop(tx);

        let deadline = Instant::now() + timeout;
        let mut last: VecDeque<String> = VecDeque::new();
        loop {
            let now = Instant::now();
            if now >= deadline {
                end(&mut child);
                return Err(TunnelError::Timeout);
            }
            let slice = (deadline - now).min(Duration::from_millis(50));
            match rx.recv_timeout(slice) {
                Ok(line) => {
                    if let Some(public_url) = scrape_url(&line, &pattern) {
                        return Ok(Tunnel { public_url, child });
                    }
                    remember(&mut last, line);
                }
                Err(RecvTimeoutError::Disconnected) => {
                    let _ = child.wait();
                    return Err(exited(last));
                }
                Err(RecvTimeoutError::Timeout) => {
                    if matches!(child.try_wait(), Ok(Some(_))) {
                        // Lines written just before exit may still be in flight.
                        while let Ok(line) = rx.recv_timeout(Duration::from_millis(100)) {
                            if let Some(public_url) = scrape_url(&line, &pattern) {
                                return Ok(Tunnel { public_url, child });
                            }
                            remember(&mut last, line);
                        }
                        return Err(exited(last));
                    }
                }
            }
        }
    }

    /// The program and arguments `start` runs, for display and tests.
    pub fn command_line(&self, port: u16) -> (PathBuf, Vec<String>) {
        match self {
            TunnelProvider::Cloudflared { binary } => (
                binary.clone(),
                vec![
                    "tunnel".to_owned(),
                    "--no-autoupdate".to_owned(),
                    "--url".to_owned(),
                    format!("http://127.0.0.1:{port}"),
                ],
            ),
            TunnelProvider::Custom { program, args, .. } => {
                let port = port.to_string();
                (
                    program.clone(),
                    args.iter().map(|a| a.replace("{port}", &port)).collect(),
                )
            }
        }
    }

    /// The regex `start` scrapes with.
    pub fn url_pattern(&self) -> String {
        match self {
            TunnelProvider::Cloudflared { .. } => {
                r"https://[a-z0-9-]+\.trycloudflare\.com".to_owned()
            }
            TunnelProvider::Custom { url_pattern, .. } => url_pattern.clone(),
        }
    }
}

const KEPT_LINES: usize = 20;

fn remember(last: &mut VecDeque<String>, line: String) {
    if last.len() == KEPT_LINES {
        last.pop_front();
    }
    last.push_back(line);
}

fn exited(last: VecDeque<String>) -> TunnelError {
    TunnelError::Exited(Vec::from(last).join("\n"))
}

fn spawn_reader<R: Read + Send + 'static>(pipe: R, tx: mpsc::Sender<String>) {
    thread::spawn(move || {
        for line in BufReader::new(pipe).lines() {
            let Ok(line) = line else { break };
            // Keep draining after `start` has returned and dropped the
            // receiver: a closed pipe kills the tunnel on its next log line.
            let _ = tx.send(line);
        }
    });
}

/// First match of `pattern` in `line`, if any. An invalid pattern never matches.
pub fn scrape_url(line: &str, pattern: &str) -> Option<String> {
    let re = Regex::new(pattern).ok()?;
    re.find(line).map(|m| m.as_str().to_owned())
}

#[cfg(all(test, any(target_os = "linux", target_os = "macos")))]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;

    fn within(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + limit;
        while Instant::now() < end {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(50));
        }
        false
    }

    #[test]
    fn the_watch_shell_ends_the_tunnel_once_the_editor_is_gone() {
        let mut editor = Command::new("sleep").arg("1").spawn().unwrap();
        let mut watch = Command::new("/bin/sh")
            .arg("-c")
            .arg(WATCH)
            .arg(editor.id().to_string())
            .args(["sleep", "60"])
            .process_group(0)
            .spawn()
            .unwrap();
        let group = i32::try_from(watch.id()).unwrap();
        editor.wait().unwrap();
        assert!(within(Duration::from_secs(5), || matches!(
            watch.try_wait(),
            Ok(Some(_))
        )));
        // SAFETY: signal 0 only asks whether anyone in the group is left.
        let gone = within(
            Duration::from_secs(5),
            || unsafe { libc::kill(-group, 0) } != 0,
        );
        if !gone {
            unsafe { libc::kill(-group, libc::SIGKILL) };
        }
        assert!(gone, "the tunnel outlived the editor");
    }
}
