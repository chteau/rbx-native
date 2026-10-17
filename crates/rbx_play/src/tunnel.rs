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
        let _ = self.child.kill();
        let _ = self.child.wait();
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
        let mut command = Command::new(program);
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        end_with_parent(&mut command);
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
                let _ = child.kill();
                let _ = child.wait();
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
