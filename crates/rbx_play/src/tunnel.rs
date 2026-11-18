//! Public HTTPS tunnel to the loopback channel. cloudflared quick tunnels by
//! default, or any command that prints its public URL.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// Fails like a direct spawn would when `program` can't be found, since the
/// watching shell itself always starts and would only hide that.
#[cfg(any(target_os = "macos", all(test, target_os = "linux")))]
fn find_program(program: &std::path::Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let runnable = |path: &std::path::Path| {
        std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    let found = if program.components().count() > 1 {
        runnable(program)
    } else {
        std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|dir| runnable(&dir.join(program)))
        })
    };
    if found {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("{} not found", program.display()),
        ))
    }
}

/// The command that runs `program args`, watched on macOS (see [`WATCH`]).
fn command(program: PathBuf, args: Vec<String>) -> std::io::Result<Command> {
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        find_program(&program)?;
        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(WATCH)
            .arg(std::process::id().to_string())
            .arg(program)
            .args(args)
            .process_group(0);
        Ok(command)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut command = Command::new(program);
        command.args(args);
        Ok(command)
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
    /// The tunnel service refused this network for asking too often (a
    /// trycloudflare 429), now or on a recent try, so nothing was started.
    RateLimited { retry_in: Duration },
}

impl TunnelError {
    /// Whether the tunnel exited because its service answered 429.
    fn says_rate_limited(&self) -> bool {
        let TunnelError::Exited(lines) = self else {
            return false;
        };
        let lower = lines.to_ascii_lowercase();
        lower.contains("status 429")
            || lower.contains("429 too many requests")
            || lower.contains("status code: 429")
    }
}

impl std::fmt::Display for TunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TunnelError::Spawn(e) => write!(f, "the tunnel program could not start: {e}"),
            TunnelError::Exited(lines) => {
                write!(f, "the tunnel exited before giving a URL:\n{lines}")
            }
            TunnelError::Timeout => f.write_str("the tunnel gave no URL in time"),
            TunnelError::RateLimited { retry_in } => write!(
                f,
                "Cloudflare is rate limiting quick tunnels from this network, try again in {}",
                minutes(*retry_in)
            ),
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
        let mut command = command(program, args).map_err(TunnelError::Spawn)?;
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        end_with_parent(&mut command);
        // No console window of its own next to the (GUI) editor.
        #[cfg(windows)]
        std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000);
        let mut child = command.spawn().map_err(TunnelError::Spawn)?;
        crate::child_job::kill_with_editor(&child);

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

    /// [`TunnelProvider::start`], unless `cooldown` says the service refused
    /// this network recently; a refusal now is recorded there.
    pub fn start_gated(
        &self,
        port: u16,
        timeout: Duration,
        cooldown: &Cooldown,
    ) -> Result<Tunnel, TunnelError> {
        if let Some(retry_in) = cooldown.remaining() {
            return Err(TunnelError::RateLimited { retry_in });
        }
        match self.start(port, timeout) {
            Ok(tunnel) => {
                cooldown.clear();
                Ok(tunnel)
            }
            Err(e) if e.says_rate_limited() => Err(TunnelError::RateLimited {
                retry_in: cooldown.refused(),
            }),
            Err(e) => Err(e),
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

/// "N min", rounded up, for a wait shown to the user.
pub fn minutes(wait: Duration) -> String {
    format!("{} min", wait.as_millis().div_ceil(60_000).max(1))
}

/// Remembers, in a file, how long quick tunnels are off after the service
/// refused this network, so every start (presence, Play, a relaunch) waits
/// it out instead of asking again: trycloudflare keeps refusing for over
/// an hour when it is asked on through its limit.
pub struct Cooldown {
    path: PathBuf,
}

/// The first wait after a refusal, doubling on each refusal in a row up to
/// [`COOLDOWN_MAX`]. A tunnel that starts clears it.
pub const COOLDOWN_START: Duration = Duration::from_secs(5 * 60);
pub const COOLDOWN_MAX: Duration = Duration::from_secs(60 * 60);

impl Cooldown {
    pub fn new(path: impl Into<PathBuf>) -> Cooldown {
        Cooldown { path: path.into() }
    }

    /// `(until, next wait)` in unix seconds and seconds, if a refusal is on file.
    fn read(&self) -> Option<(u64, u64)> {
        let text = std::fs::read_to_string(&self.path).ok()?;
        let mut parts = text.split_whitespace().map(str::parse::<u64>);
        Some((parts.next()?.ok()?, parts.next()?.ok()?))
    }

    /// How long until tunnels may be asked for again, if they are off.
    pub fn remaining(&self) -> Option<Duration> {
        let (until, _) = self.read()?;
        let left = until.checked_sub(unix_secs())?;
        (left > 0).then(|| Duration::from_secs(left))
    }

    /// Records a refusal and returns how long tunnels are now off.
    pub fn refused(&self) -> Duration {
        let wait = self
            .read()
            .map_or(COOLDOWN_START, |(_, next)| Duration::from_secs(next))
            .clamp(COOLDOWN_START, COOLDOWN_MAX);
        let next = (wait * 2).min(COOLDOWN_MAX);
        let until = unix_secs() + wait.as_secs();
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&self.path, format!("{until} {}\n", next.as_secs()));
        wait
    }

    /// A tunnel started: the next refusal starts from [`COOLDOWN_START`].
    pub fn clear(&self) {
        let _ = std::fs::remove_file(&self.path);
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
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

    #[test]
    fn find_program_matches_a_direct_spawn() {
        assert!(find_program(std::path::Path::new("/bin/sh")).is_ok());
        assert!(find_program(std::path::Path::new("sh")).is_ok());
        let missing = find_program(std::path::Path::new("/nonexistent/no-such-tunnel"));
        assert_eq!(missing.unwrap_err().kind(), std::io::ErrorKind::NotFound);
        assert!(find_program(std::path::Path::new("rbx-play-no-such-tunnel")).is_err());
    }

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
