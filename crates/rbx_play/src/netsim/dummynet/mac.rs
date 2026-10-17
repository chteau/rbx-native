//! macOS shaper: a root helper (`helper.sh`, started through one
//! administrator prompt) drives pf/dummynet; this side talks to it over two
//! FIFOs in a private temp directory and tells it which UDP ports the Roblox
//! player uses.
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::DirBuilderExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::{osascript_args, parse_lsof_udp, HELPER};
use crate::netsim::lines;
use crate::netsim::{NetConditions, ShaperError};

/// osascript only returns once the password is in, so the helper is already
/// starting; it needs a handful of `pfctl`/`dnctl` calls.
const READY_TIMEOUT: Duration = Duration::from_secs(15);
const ACK_TIMEOUT: Duration = Duration::from_secs(5);
const PORT_POLL: Duration = Duration::from_millis(500);

/// Shapes the Roblox player's UDP traffic through pf dummynet.
pub struct Shaper {
    dir: PathBuf,
    cmd: Arc<Mutex<File>>,
    status: Receiver<String>,
    _status_file: File,
    pid: u32,
    jitter: bool,
    stop: Arc<AtomicBool>,
    watch: Option<JoinHandle<()>>,
}

impl Shaper {
    /// Asks for administrator rights once and waits for the helper to be ready.
    pub fn start() -> Result<Shaper, ShaperError> {
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

        let setup = (|| -> Result<(File, File), ShaperError> {
            fs::write(dir.join("helper.sh"), HELPER).map_err(|e| fail("helper script", &e))?;
            let made = Command::new("mkfifo")
                .args(["-m", "600"])
                .arg(dir.join("cmd"))
                .arg(dir.join("status"))
                .output()
                .map_err(|e| fail("mkfifo", &e))?;
            if !made.status.success() {
                return Err(fail(
                    "mkfifo",
                    &String::from_utf8_lossy(&made.stderr).trim(),
                ));
            }
            let open = |name: &str| {
                OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(dir.join(name))
                    .map_err(|e| fail("pipe", &e))
            };
            Ok((open("cmd")?, open("status")?))
        })();
        let (cmd_file, status_file) = match setup {
            Ok(files) => files,
            Err(e) => {
                let _ = fs::remove_dir_all(&dir);
                return Err(e);
            }
        };

        let (tx, rx) = mpsc::channel();
        let reader = match status_file.try_clone() {
            Ok(f) => f,
            Err(e) => {
                let _ = fs::remove_dir_all(&dir);
                return Err(fail("pipe", &e));
            }
        };
        thread::spawn(move || {
            for line in BufReader::new(reader).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        let mut shaper = Shaper {
            dir,
            cmd: Arc::new(Mutex::new(cmd_file)),
            status: rx,
            _status_file: status_file,
            pid: 0,
            jitter: true,
            stop: Arc::new(AtomicBool::new(false)),
            watch: None,
        };
        shaper.launch()?;
        shaper.await_ready()?;
        shaper.spawn_watch();
        Ok(shaper)
    }

    fn launch(&self) -> Result<(), ShaperError> {
        let dir = self.dir.to_string_lossy();
        let args = osascript_args(&dir, std::process::id()).ok_or_else(|| {
            ShaperError("the temporary folder path has characters the helper cannot use".into())
        })?;
        let out = Command::new("osascript")
            .args(&args)
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
        }
        let _ = fs::remove_dir_all(&self.dir);
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
