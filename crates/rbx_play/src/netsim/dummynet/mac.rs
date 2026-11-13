//! macOS shaper: a root helper (`helper.sh`, installed once as a launchd
//! daemon) drives pf/dummynet; this side connects to its socket for the
//! length of a Play session and tells it which UDP ports the Roblox player
//! uses. The connection closing is what ends the shaping.
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use super::{
    helper_version, install_command, osascript_args, parse_lsof_udp, plist, uninstall_command,
    HELPER, PLIST_PATH, SOCKET,
};
use crate::netsim::lines;
use crate::netsim::{NetConditions, ShaperError};

/// A handful of `pfctl`/`dnctl` calls between connecting and `ready`.
const READY_TIMEOUT: Duration = Duration::from_secs(15);
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// launchd opens the socket a moment after `bootstrap` returns.
const SOCKET_WAIT: Duration = Duration::from_secs(5);
const ACK_TIMEOUT: Duration = Duration::from_secs(5);
const PORT_POLL: Duration = Duration::from_millis(500);

/// Whether the network helper daemon is installed (Settings offers removal).
pub fn helper_installed() -> bool {
    Path::new(PLIST_PATH).exists()
}

/// Stops and deletes the helper daemon, behind one administrator prompt.
pub fn remove_helper() -> Result<(), ShaperError> {
    admin(
        &uninstall_command(),
        "RbxNative wants to remove its network helper.",
    )
}

/// Runs `command` as root through osascript's administrator prompt.
fn admin(command: &str, prompt: &str) -> Result<(), ShaperError> {
    let out = Command::new("osascript")
        .args(osascript_args(command, prompt))
        .stdin(Stdio::null())
        .output()
        .map_err(|e| ShaperError(format!("could not ask for administrator rights ({e})")))?;
    if out.status.success() {
        return Ok(());
    }
    let err = String::from_utf8_lossy(&out.stderr);
    if err.contains("-128") {
        Err(ShaperError(
            "the administrator password prompt was cancelled".into(),
        ))
    } else {
        Err(ShaperError(format!(
            "could not get administrator rights ({})",
            err.trim()
        )))
    }
}

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Installs (or replaces) the helper daemon: the one administrator prompt.
fn install() -> Result<(), ShaperError> {
    let fail = |what: &str, e: &dyn std::fmt::Display| {
        ShaperError(format!("could not set up the network helper: {what} ({e})"))
    };
    let mut raw = [0u8; 8];
    getrandom::fill(&mut raw).map_err(|e| fail("random name", &e))?;
    let hex: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let dir = std::env::temp_dir().join(format!("rbxnative-netsim-{hex}"));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| fail("temp directory", &e))?;
    let result = (|| {
        fs::write(dir.join("helper.sh"), HELPER).map_err(|e| fail("helper script", &e))?;
        // The folder was just made by this process, so it carries our uid.
        let uid = fs::metadata(&dir)
            .map_err(|e| fail("temp directory", &e))?
            .uid();
        let daemon = plist(uid);
        fs::write(dir.join("daemon.plist"), &daemon).map_err(|e| fail("daemon file", &e))?;
        let command = install_command(&dir.to_string_lossy(), &sha256(HELPER), &sha256(&daemon))
            .ok_or_else(|| {
                ShaperError("the temporary folder path has characters the helper cannot use".into())
            })?;
        admin(
            &command,
            "RbxNative wants to install a small helper so Play can simulate network conditions. You will only be asked once.",
        )
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

/// A connection to the daemon and the version its `hello` named.
fn connect() -> Option<(UnixStream, BufReader<UnixStream>, u32)> {
    let stream = UnixStream::connect(SOCKET).ok()?;
    stream.set_read_timeout(Some(HELLO_TIMEOUT)).ok()?;
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut hello = String::new();
    reader.read_line(&mut hello).ok()?;
    let version = hello.trim().strip_prefix("hello ")?.parse().ok()?;
    stream.set_read_timeout(None).ok()?;
    Some((stream, reader, version))
}

/// A connection to an up-to-date daemon, installing or replacing it first if
/// needed.
fn connect_current() -> Result<(UnixStream, BufReader<UnixStream>), ShaperError> {
    let want = helper_version();
    if let Some((stream, reader, version)) = connect() {
        if version == want {
            return Ok((stream, reader));
        }
    }
    install()?;
    let deadline = Instant::now() + SOCKET_WAIT;
    loop {
        match connect() {
            Some((stream, reader, version)) if version == want => return Ok((stream, reader)),
            Some((_, _, version)) => {
                return Err(ShaperError(format!(
                    "the network helper still runs version {version} after the update"
                )));
            }
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(200)),
            None => {
                return Err(ShaperError(
                    "the network helper was installed but doesn't answer".into(),
                ));
            }
        }
    }
}

/// Shapes the Roblox player's UDP traffic through pf dummynet.
pub struct Shaper {
    cmd: Arc<Mutex<UnixStream>>,
    status: Receiver<String>,
    pid: u32,
    jitter: bool,
    stop: Arc<AtomicBool>,
    watch: Option<JoinHandle<()>>,
}

impl Shaper {
    /// Connects to the helper (installing it the first time, the only
    /// password prompt) and waits for it to be ready.
    pub fn start() -> Result<Shaper, ShaperError> {
        let (stream, reader) = connect_current()?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            for line in reader.lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut shaper = Shaper {
            cmd: Arc::new(Mutex::new(stream)),
            status: rx,
            pid: 0,
            jitter: true,
            stop: Arc::new(AtomicBool::new(false)),
            watch: None,
        };
        shaper.await_ready()?;
        shaper.spawn_watch();
        Ok(shaper)
    }

    fn await_ready(&mut self) -> Result<(), ShaperError> {
        let deadline = Instant::now() + READY_TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.status.recv_timeout(left) {
                Ok(line) if line == "nojitter" => self.jitter = false,
                Ok(line) if line.starts_with("error") => {
                    return Err(ShaperError(format!("the network helper failed ({line})")));
                }
                Ok(line) => {
                    if let Some(pid) = line.strip_prefix("ready ") {
                        self.pid = pid.trim().parse().map_err(|_| {
                            ShaperError("the network helper sent an unreadable ready line".into())
                        })?;
                        return Ok(());
                    }
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(ShaperError(
                        "the network helper did not start in time".into(),
                    ));
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(ShaperError("the network helper exited early".into()));
                }
            }
        }
    }

    fn spawn_watch(&mut self) {
        let cmd = Arc::clone(&self.cmd);
        let stop = Arc::clone(&self.stop);
        self.watch = Some(thread::spawn(move || {
            let mut last: Option<Vec<u16>> = None;
            while !stop.load(Ordering::Relaxed) {
                let ports = player_ports();
                if last.as_ref() != Some(&ports) {
                    let line = lines::ports_line(&ports);
                    if let Ok(mut f) = cmd.lock() {
                        let _ = f.write_all(line.as_bytes());
                    }
                    last = Some(ports);
                }
                thread::sleep(PORT_POLL);
            }
        }));
    }

    /// Sends new conditions and waits for the helper to confirm both directions.
    pub fn apply(&mut self, conditions: &NetConditions) -> Result<(), ShaperError> {
        let text = lines::conditions_lines(conditions);
        self.cmd
            .lock()
            .map_err(|_| ShaperError("the network helper connection broke".into()))?
            .write_all(text.as_bytes())
            .map_err(|e| ShaperError(format!("could not reach the network helper ({e})")))?;
        let deadline = Instant::now() + ACK_TIMEOUT;
        let (mut out, mut inn) = (false, false);
        while !(out && inn) {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.status.recv_timeout(left) {
                Ok(line) => match line.as_str() {
                    "ok out" => out = true,
                    "ok in" => inn = true,
                    l if l.starts_with("error out") || l.starts_with("error in") => {
                        return Err(ShaperError(format!(
                            "the new conditions were not applied ({l})"
                        )));
                    }
                    _ => {}
                },
                Err(_) => {
                    return Err(ShaperError(
                        "the new conditions were not applied (the helper did not answer)".into(),
                    ));
                }
            }
        }
        Ok(())
    }

    /// The helper process is still running.
    pub fn alive(&mut self) -> bool {
        self.pid != 0
            && Command::new("ps")
                .args(["-p", &self.pid.to_string()])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
    }

    /// A limitation the user should hear about, if any.
    pub fn notice(&self) -> Option<&'static str> {
        if self.jitter {
            None
        } else {
            Some(
                "jitter is not available: this macOS's pf does not accept `probability` on dummynet rules, so each direction gets its mean delay",
            )
        }
    }
}

impl Drop for Shaper {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(watch) = self.watch.take() {
            let _ = watch.join();
        }
        if let Ok(mut f) = self.cmd.lock() {
            let _ = f.write_all(b"stop\n");
            let _ = f.shutdown(std::net::Shutdown::Both);
        }
    }
}

/// Local UDP ports of every running Roblox player, sorted.
fn player_ports() -> Vec<u16> {
    let Ok(out) = Command::new("pgrep").args(["-x", "RobloxPlayer"]).output() else {
        return Vec::new();
    };
    let mut ports = Vec::new();
    for pid in String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<u32>().ok())
    {
        if let Ok(o) = Command::new("lsof")
            .args(["-nP", "-a", "-p", &pid.to_string(), "-iUDP", "-F", "n"])
            .output()
        {
            ports.extend(parse_lsof_udp(&String::from_utf8_lossy(&o.stdout)));
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}
