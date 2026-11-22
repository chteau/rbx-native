//! The editor's side of the bridge: a loopback listener, and the file that
//! tells [`super::broker`] its port, token and `studio_id`.
//!
//! Each connection gets a thread that reads one line at a time and waits for
//! the UI thread's answer before reading the next, so requests from one
//! client are answered in order and never two at once.
//!
//! The token is what keeps another account on the machine out, so on unix
//! the directory is `0700` and each file `0600`; Windows' `%APPDATA%` is
//! already private to its user.

#[cfg(test)]
mod tests;

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;

/// What an editor's `<studio_id>.json` holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Endpoint {
    /// Hex digits only: it names the file, so it can never step out of
    /// the directory.
    pub(crate) studio_id: String,
    pub(crate) port: u16,
    pub(crate) token: String,
}

impl Endpoint {
    pub(crate) fn to_json(&self) -> String {
        json!({ "studio_id": self.studio_id, "port": self.port, "token": self.token }).to_string()
    }

    pub(crate) fn from_json(text: &str) -> Option<Endpoint> {
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        let studio_id = value.get("studio_id")?.as_str()?;
        if studio_id.is_empty() || !studio_id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        Some(Endpoint {
            studio_id: studio_id.to_owned(),
            port: u16::try_from(value.get("port")?.as_u64()?).ok()?,
            token: value.get("token")?.as_str()?.to_owned(),
        })
    }

    pub(crate) fn load(path: &Path) -> Option<Endpoint> {
        Endpoint::from_json(&std::fs::read_to_string(path).ok()?)
    }
}

/// Where the editor `studio_id` names says how to reach it.
pub(crate) fn file_of(dir: &Path, studio_id: &str) -> PathBuf {
    dir.join(format!("{studio_id}.json"))
}

/// One line from a client, waiting for the UI thread's answer.
pub(crate) struct Request {
    line: String,
    reply: Sender<Option<String>>,
}

impl Request {
    pub(crate) fn line(&self) -> &str {
        &self.line
    }

    /// `None` sends nothing back, for a notification.
    pub(crate) fn answer(self, reply: Option<String>) {
        // A client that hung up meanwhile has nobody left to tell.
        let _ = self.reply.send(reply);
    }
}

/// The listener, for as long as it is kept. Dropping it stops accepting,
/// ends every connection at its next line and removes its file.
pub(crate) struct Server {
    endpoint: Endpoint,
    file: PathBuf,
    requests: Receiver<Request>,
    stopped: Arc<AtomicBool>,
    activity: Arc<Activity>,
}

/// What the listener has seen of clients, for Settings to show whether one
/// is attached.
#[derive(Default)]
struct Activity {
    /// Connections that showed the token and are still open.
    connected: AtomicUsize,
    last_call: Mutex<Option<Instant>>,
}

/// Counts a connection as attached for as long as it is kept.
struct Attached(Arc<Activity>);

impl Attached {
    fn new(activity: Arc<Activity>) -> Self {
        activity.connected.fetch_add(1, Ordering::SeqCst);
        Attached(activity)
    }
}

impl Drop for Attached {
    fn drop(&mut self) {
        self.0.connected.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Server {
    /// Listens on a port of the system's choosing, under a new `studio_id`,
    /// and writes `<dir>/<studio_id>.json`.
    pub(crate) fn start(dir: &Path) -> std::io::Result<Server> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let endpoint = Endpoint {
            studio_id: random_hex(8)?,
            port: listener.local_addr()?.port(),
            token: random_hex(16)?,
        };
        private_dir(dir)?;
        let file = file_of(dir, &endpoint.studio_id);
        // Written whole and renamed, so a broker scanning the directory never
        // reads half a file.
        let tmp = file.with_extension("json.tmp");
        write_private(&tmp, endpoint.to_json().as_bytes())?;
        std::fs::rename(&tmp, &file)?;

        let (sender, requests) = mpsc::channel();
        let stopped = Arc::new(AtomicBool::new(false));
        let token = endpoint.token.clone();
        let flag = Arc::clone(&stopped);
        let activity = Arc::new(Activity::default());
        let seen = Arc::clone(&activity);
        std::thread::Builder::new()
            .name("mcp-listener".to_owned())
            .spawn(move || accept(&listener, &token, &sender, &flag, &seen))?;
        Ok(Server {
            endpoint,
            file,
            requests,
            stopped,
            activity,
        })
    }

    pub(crate) fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// How many clients (bridges) hold a connection right now. A bridge
    /// connects on its client's first call and keeps the connection.
    pub(crate) fn clients(&self) -> usize {
        self.activity.connected.load(Ordering::SeqCst)
    }

    /// When a client last sent anything.
    pub(crate) fn last_call(&self) -> Option<Instant> {
        *self
            .activity
            .last_call
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Everything clients sent since the last call.
    pub(crate) fn drain(&self) -> Vec<Request> {
        self.requests.try_iter().collect()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        // `accept` blocks; a connection of our own is what wakes it to see
        // the flag.
        let _ = TcpStream::connect((Ipv4Addr::LOCALHOST, self.endpoint.port));
        let _ = std::fs::remove_file(&self.file);
    }
}

fn accept(
    listener: &TcpListener,
    token: &str,
    sender: &Sender<Request>,
    stopped: &AtomicBool,
    activity: &Arc<Activity>,
) {
    for stream in listener.incoming() {
        if stopped.load(Ordering::SeqCst) {
            return;
        }
        let Ok(stream) = stream else {
            continue;
        };
        let token = token.to_owned();
        let sender = sender.clone();
        let activity = Arc::clone(activity);
        let _ = std::thread::Builder::new()
            .name("mcp-connection".to_owned())
            .spawn(move || serve(stream, &token, &sender, activity));
    }
}

fn serve(stream: TcpStream, token: &str, sender: &Sender<Request>, activity: Arc<Activity>) {
    let Ok(mut writer) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(stream);
    if !introduced(&mut reader, token) {
        let _ = writer.shutdown(Shutdown::Both);
        return;
    }
    // A client may sit idle between calls for as long as it likes once it
    // has shown the token.
    if reader.get_ref().set_read_timeout(None).is_err() {
        return;
    }
    let attached = Attached::new(activity);
    for line in reader.lines() {
        let Ok(line) = line else {
            return;
        };
        if line.trim().is_empty() {
            continue;
        }
        *attached
            .0
            .last_call
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
        let (reply, answer) = mpsc::channel();
        if sender.send(Request { line, reply }).is_err() {
            return;
        }
        // The server is gone if this fails: nothing will ever answer.
        let Ok(answer) = answer.recv() else {
            return;
        };
        if let Some(text) = answer {
            if writeln!(writer, "{text}")
                .and_then(|()| writer.flush())
                .is_err()
            {
                return;
            }
        }
    }
}

/// How long a connection has to show the token, and how much it may send
/// before it does: a token is 32 hex digits and a newline, so anything more
/// is not a client, and nobody else gets to hold a thread or fill memory.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const HANDSHAKE_LIMIT: u64 = 64;

/// Whether the connection's first line is `token`.
fn introduced(reader: &mut BufReader<TcpStream>, token: &str) -> bool {
    if reader
        .get_ref()
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .is_err()
    {
        return false;
    }
    let mut first = Vec::new();
    let read = reader
        .by_ref()
        .take(HANDSHAKE_LIMIT)
        .read_until(b'\n', &mut first);
    read.is_ok() && first.ends_with(b"\n") && same(first.trim_ascii_end(), token.as_bytes())
}

/// `a == b` in a time that does not depend on where they first differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0, |differ, (x, y)| differ | (x ^ y)) == 0
}

/// `bytes` from the OS random source, as hex.
fn random_hex(bytes: usize) -> std::io::Result<String> {
    let mut raw = vec![0u8; bytes];
    getrandom::fill(&mut raw).map_err(std::io::Error::other)?;
    Ok(raw.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// Creates `dir`, and keeps it readable by its owner alone.
fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Writes `path` readable by its owner alone from the moment it exists. A
/// file left there before is removed first, since opening one only keeps the
/// mode it already has.
fn write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
        _ => {}
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)?.write_all(contents)
}
