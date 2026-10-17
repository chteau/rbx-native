//! The Linux shaper, unprivileged: `unshare` makes a user + network
//! namespace whose first process (the holder, `holder.sh`) owns its
//! capabilities and runs `ip`/`tc` there; `slirp4netns` gives it internet
//! through a `tap0`; the client is started inside with `nsenter` (see
//! [`Shaper::wrapper`]). Entering as the same non-root uid drops the
//! inherited capabilities on exec, which `flatpak`'s `bwrap` insists on.

use std::io::{BufRead as _, BufReader, Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};

use super::NetConditions;
use crate::tunnel::end_with_parent;

const HOLDER: &str = include_str!("holder.sh");
/// slirp4netns creates `tap0` and configures it in well under a second;
/// the holder itself gives up after 10 s.
const READY_TIMEOUT: Duration = Duration::from_secs(15);
const ACK_TIMEOUT: Duration = Duration::from_secs(5);

/// Why the shaper is unavailable or failed, worded for the Output panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShaperError(pub String);

impl std::fmt::Display for ShaperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ShaperError {}

fn fail<T>(reason: impl Into<String>) -> Result<T, ShaperError> {
    Err(ShaperError(reason.into()))
}

fn on_path(program: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(program))
        .find(|path| path.is_file())
}

fn sysctl(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|v| v.trim().to_owned())
}

/// Whether this machine can run a [`Shaper`] with `slirp4netns`, or why
/// not. Cheap (no process started); [`Shaper::start`] still reports the
/// step that failed when a kernel feature turns out to be missing.
pub fn probe(slirp4netns: &Path) -> Result<(), ShaperError> {
    for (program, package) in [
        ("unshare", "util-linux"),
        ("nsenter", "util-linux"),
        ("ip", "iproute2"),
        ("tc", "iproute2"),
    ] {
        if on_path(program).is_none() {
            return fail(format!(
                "`{program}` was not found on PATH (it comes with {package})"
            ));
        }
    }
    if !slirp4netns.is_file() && on_path(&slirp4netns.to_string_lossy()).is_none() {
        return fail(format!("{} was not found", slirp4netns.display()));
    }
    if sysctl("/proc/sys/user/max_user_namespaces").as_deref() == Some("0") {
        return fail("user namespaces are turned off (user.max_user_namespaces = 0)");
    }
    if sysctl("/proc/sys/kernel/unprivileged_userns_clone").as_deref() == Some("0") {
        return fail(
            "unprivileged user namespaces are turned off (kernel.unprivileged_userns_clone = 0)",
        );
    }
    if sysctl("/proc/sys/kernel/apparmor_restrict_unprivileged_userns").as_deref() == Some("1") {
        return fail(
            "AppArmor keeps unprivileged user namespaces from configuring their network \
             (kernel.apparmor_restrict_unprivileged_userns = 1)",
        );
    }
    Ok(())
}

/// A shaped network namespace, alive until dropped. Both helper processes
/// get `SIGTERM` if the editor dies (so start it on a long-lived thread, see
/// `tunnel::end_with_parent`).
pub struct Shaper {
    holder: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    slirp: Child,
}

fn forward(stream: impl Read + Send + 'static, prefix: &'static str, tx: Sender<String>) {
    std::thread::spawn(move || {
        for line in BufReader::new(stream).lines().map_while(Result::ok) {
            if tx.send(format!("{prefix}{line}")).is_err() {
                break;
            }
        }
    });
}

impl Shaper {
    /// Starts the namespace with no added delay and waits until it is
    /// shaped. `host_loopback` lets it reach the host's 127.0.0.1 at
    /// 10.0.2.2, which tests use; Play passes `false`.
    pub fn start(slirp4netns: &Path, host_loopback: bool) -> Result<Shaper, ShaperError> {
        probe(slirp4netns)?;
        let (tx, lines) = mpsc::channel();
        let mut command = Command::new("unshare");
        command
            .args(["-U", "-n", "--map-current-user", "--keep-caps"])
            .args(["sh", "-c", HOLDER, "rbx-netsim"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        end_with_parent(&mut command);
        let mut holder = command
            .spawn()
            .map_err(|err| ShaperError(format!("`unshare` could not be started: {err}")))?;
        forward(holder.stdout.take().expect("piped"), "", tx.clone());
        forward(holder.stderr.take().expect("piped"), "", tx.clone());
        let stdin = holder.stdin.take().expect("piped");

        let mut slirp = Command::new(slirp4netns);
        slirp.args(["--configure", "--mtu=65520"]);
        if !host_loopback {
            slirp.arg("--disable-host-loopback");
        }
        slirp
            .args([holder.id().to_string().as_str(), "tap0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        end_with_parent(&mut slirp);

        // Until `slirp` exists, a failure only has the holder to clean up.
        let mut partial = PartialShaper {
            holder: Some(holder),
        };
        let deadline = Instant::now() + READY_TIMEOUT;
        wait_for(&lines, "ns", deadline).map_err(|why| {
            ShaperError(format!(
                "the network namespace could not be created ({why})"
            ))
        })?;
        let mut slirp = slirp
            .spawn()
            .map_err(|err| ShaperError(format!("slirp4netns could not be started: {err}")))?;
        forward(slirp.stderr.take().expect("piped"), "slirp4netns: ", tx);
        let shaper = Shaper {
            holder: partial.holder.take().expect("set above"),
            stdin,
            lines,
            slirp,
        };
        wait_for(&shaper.lines, "ready", deadline).map_err(|why| {
            ShaperError(format!(
                "the network could not be shaped ({why}){}",
                module_hint(&why)
            ))
        })?;
        Ok(shaper)
    }

    /// The program and arguments that run a command inside the namespace,
    /// for `ClientOptions::wrapper`.
    pub fn wrapper(&self) -> Vec<String> {
        [
            "nsenter",
            "-t",
            &self.holder.id().to_string(),
            "-U",
            "-n",
            "--preserve-credentials",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    /// Applies `conditions` (clamped) at once, to running traffic too.
    pub fn apply(&mut self, conditions: &NetConditions) -> Result<(), ShaperError> {
        let lines = conditions.clamped().holder_lines();
        self.stdin
            .write_all(lines.as_bytes())
            .and_then(|()| self.stdin.flush())
            .map_err(|err| ShaperError(format!("the shaper stopped ({err})")))?;
        let deadline = Instant::now() + ACK_TIMEOUT;
        for _ in 0..2 {
            wait_for(&self.lines, "ok", deadline).map_err(|why| {
                ShaperError(format!("the new conditions were not applied ({why})"))
            })?;
        }
        Ok(())
    }

    /// Both helper processes are still running.
    pub fn alive(&mut self) -> bool {
        matches!(self.holder.try_wait(), Ok(None)) && matches!(self.slirp.try_wait(), Ok(None))
    }
}

impl Drop for Shaper {
    fn drop(&mut self) {
        for child in [&mut self.slirp, &mut self.holder] {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// The holder before `slirp4netns` has started, killed if start gives up.
struct PartialShaper {
    holder: Option<Child>,
}

impl Drop for PartialShaper {
    fn drop(&mut self) {
        if let Some(mut holder) = self.holder.take() {
            let _ = holder.kill();
            let _ = holder.wait();
        }
    }
}

/// Reads lines until `want`; an `error …` line, the holder ending or the
/// deadline fail with what was printed meanwhile.
fn wait_for(lines: &Receiver<String>, want: &str, deadline: Instant) -> Result<(), String> {
    let mut seen = Vec::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match lines.recv_timeout(left) {
            Ok(line) if line == want => return Ok(()),
            Ok(line) if line.starts_with("error") => {
                seen.push(line);
                return Err(seen.join("; "));
            }
            Ok(line) => seen.push(line),
            Err(RecvTimeoutError::Timeout) => {
                seen.push("timed out".to_owned());
                return Err(seen.join("; "));
            }
            Err(RecvTimeoutError::Disconnected) => {
                seen.push("it exited".to_owned());
                return Err(seen.join("; "));
            }
        }
    }
}

/// A user namespace cannot load kernel modules; name the one to load.
fn module_hint(why: &str) -> &'static str {
    let missing = |module: &str| !Path::new("/sys/module").join(module).exists();
    if why.contains("error ifb") && missing("ifb") {
        "; load the ifb module once with `sudo modprobe ifb`"
    } else if why.contains("error netem") && missing("sch_netem") {
        "; load the netem module once with `sudo modprobe sch_netem`"
    } else {
        ""
    }
}
