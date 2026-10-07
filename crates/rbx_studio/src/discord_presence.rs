//! Discord Rich Presence over the local IPC pipe. Runs on a background
//! thread; the editor sends activity updates and the thread connects,
//! reconnects and retries silently when Discord is not running.

use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CLIENT_ID: &str = "1557174796435980338";
const RETRY_DELAY: Duration = Duration::from_secs(15);
/// Discord accepts five `SET_ACTIVITY` per 20 seconds and drops the rest,
/// which could leave a stale activity showing; updates closer together
/// than this are coalesced into the newest one instead.
const MIN_SPACING: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Activity {
    pub(crate) place: String,
    pub(crate) detail: String,
    pub(crate) started: u64,
    pub(crate) kind: Kind,
}

/// What the user is doing, which picks the large image. Each variant's
/// asset key is uploaded to the Discord application; `instudio` is the
/// small badge on every one of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Building,
    Scripting,
    UiDesigning,
    /// No editor drives this yet; the asset is uploaded for when an
    /// animation editor lands.
    #[allow(dead_code)]
    Animating,
    Idling,
}

impl Kind {
    fn asset(self) -> (&'static str, &'static str) {
        match self {
            Kind::Building => ("building", "Building"),
            Kind::Scripting => ("scripting", "Scripting"),
            Kind::UiDesigning => ("uidesigning", "Designing UI"),
            Kind::Animating => ("animating", "Animating"),
            Kind::Idling => ("idling", "Idle"),
        }
    }
}

enum Message {
    Update(Activity),
    Stop,
}

pub(crate) struct Presence {
    sender: mpsc::Sender<Message>,
}

impl Presence {
    pub(crate) fn start(activity: Activity) -> Self {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || run(receiver, activity))
            .expect("spawning the Discord presence thread");
        Presence { sender }
    }

    pub(crate) fn update(&self, activity: Activity) {
        let _ = self.sender.send(Message::Update(activity));
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Stop);
    }
}

fn run(receiver: mpsc::Receiver<Message>, mut activity: Activity) {
    loop {
        match try_session(&receiver, &mut activity) {
            SessionExit::Stop => return,
            SessionExit::Disconnected => match receiver.recv_timeout(RETRY_DELAY) {
                Ok(Message::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
                Ok(Message::Update(new)) => activity = new,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            },
        }
    }
}

enum SessionExit {
    Stop,
    Disconnected,
}

fn try_session(receiver: &mpsc::Receiver<Message>, activity: &mut Activity) -> SessionExit {
    let Some(mut pipe) = ipc::connect() else {
        return SessionExit::Disconnected;
    };

    let handshake = format!(r#"{{"v":1,"client_id":"{CLIENT_ID}"}}"#);
    if ipc::write_frame(&mut pipe, 0, handshake.as_bytes()).is_err() {
        return SessionExit::Disconnected;
    }
    // A rejected client id answers with a close frame instead of READY.
    match read_reply(&mut pipe) {
        Ok(reply) if reply.contains(r#""evt":"READY""#) => {}
        _ => return SessionExit::Disconnected,
    }

    if send_activity(&mut pipe, activity).is_err() {
        return SessionExit::Disconnected;
    }
    let mut sent = Instant::now();

    loop {
        match receiver.recv() {
            Ok(Message::Update(new)) => *activity = new,
            Ok(Message::Stop) | Err(_) => return SessionExit::Stop,
        }
        loop {
            let wait = MIN_SPACING.saturating_sub(sent.elapsed());
            if wait.is_zero() {
                break;
            }
            match receiver.recv_timeout(wait) {
                Ok(Message::Update(new)) => *activity = new,
                Ok(Message::Stop) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return SessionExit::Stop;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => break,
            }
        }
        if send_activity(&mut pipe, activity).is_err() {
            return SessionExit::Disconnected;
        }
        sent = Instant::now();
    }
}

/// Sends one activity and reads Discord's answer to it, so replies never
/// pile up unread in the socket and a rejection is at least reported.
fn send_activity(pipe: &mut ipc::Pipe, activity: &Activity) -> Result<(), ()> {
    let payload = activity_json(activity);
    ipc::write_frame(pipe, 1, payload.as_bytes())?;
    let reply = read_reply(pipe)?;
    if reply.contains(r#""evt":"ERROR""#) {
        eprintln!("Discord rejected the presence update: {reply}");
    }
    Ok(())
}

/// The next data frame, answering any ping on the way. Opcodes: 0
/// handshake, 1 frame, 2 close, 3 ping, 4 pong; a close (or anything
/// unknown) ends the session.
fn read_reply(pipe: &mut ipc::Pipe) -> Result<String, ()> {
    loop {
        let (opcode, payload) = ipc::read_frame(pipe)?;
        match opcode {
            1 => return Ok(String::from_utf8_lossy(&payload).into_owned()),
            3 => ipc::write_frame(pipe, 4, &payload)?,
            _ => return Err(()),
        }
    }
}

fn activity_json(activity: &Activity) -> String {
    let details = if activity.place.is_empty() {
        "Editing in RbxNative".to_owned()
    } else {
        format!("Editing {}", activity.place)
    };
    let state = if activity.detail.is_empty() {
        None
    } else {
        Some(format!(r#","state":"{}""#, json_escape(&activity.detail)))
    };
    format!(
        concat!(
            r#"{{"cmd":"SET_ACTIVITY","args":{{"pid":{pid},"activity":{{"#,
            r#""details":"{details}""#,
            r#"{state}"#,
            r#","timestamps":{{"start":{start}}}"#,
            r#","assets":{{"large_image":"{image}","large_text":"{image_text}""#,
            r#","small_image":"instudio","small_text":"RbxNative"}}"#,
            r#"}}}},"nonce":"{nonce}"}}"#,
        ),
        pid = std::process::id(),
        details = json_escape(&details),
        state = state.as_deref().unwrap_or(""),
        start = activity.started,
        image = activity.kind.asset().0,
        image_text = activity.kind.asset().1,
        nonce = nonce(),
    )
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\x20' => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out
}

fn nonce() -> String {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{}", t.as_secs(), t.subsec_nanos())
}

pub(crate) fn now_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── Platform IPC ────────────────────────────────────────────────────────

mod ipc {
    use std::io::{Read, Write};

    #[cfg(windows)]
    pub(super) type Pipe = std::fs::File;

    #[cfg(windows)]
    pub(super) fn connect() -> Option<Pipe> {
        for i in 0..10 {
            let path = format!(r"\\.\pipe\discord-ipc-{i}");
            if let Ok(pipe) = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
            {
                return Some(pipe);
            }
        }
        None
    }

    #[cfg(not(windows))]
    pub(super) type Pipe = std::os::unix::net::UnixStream;

    #[cfg(not(windows))]
    pub(super) fn connect() -> Option<Pipe> {
        let candidates: Vec<std::path::PathBuf> = [
            std::env::var("XDG_RUNTIME_DIR").ok(),
            std::env::var("TMPDIR").ok(),
            std::env::var("TMP").ok(),
            std::env::var("TEMP").ok(),
            Some("/tmp".into()),
        ]
        .into_iter()
        .flatten()
        .filter(|s| !s.is_empty())
        .flat_map(|base| sandbox_dirs(std::path::PathBuf::from(base)))
        .collect();

        for dir in &candidates {
            for i in 0..10 {
                let path = dir.join(format!("discord-ipc-{i}"));
                if let Ok(stream) = std::os::unix::net::UnixStream::connect(&path) {
                    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                    let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
                    return Some(stream);
                }
            }
        }
        None
    }

    /// `base` itself plus every sandboxed client's view of it, found by
    /// listing rather than by app id so any Discord build or modded client
    /// is found: `app/<id>` (the official Flatpak), `.flatpak/<id>/xdg-run`
    /// (Vesktop and other Flatpaks running arRPC) and `snap.<name>` (Snap).
    #[cfg(not(windows))]
    pub(super) fn sandbox_dirs(base: std::path::PathBuf) -> Vec<std::path::PathBuf> {
        let children = |dir: std::path::PathBuf| {
            std::fs::read_dir(dir)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.path())
        };
        let mut dirs = vec![base.clone()];
        dirs.extend(children(base.join("app")));
        dirs.extend(children(base.join(".flatpak")).map(|dir| dir.join("xdg-run")));
        dirs.extend(children(base).filter(|dir| {
            dir.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("snap."))
        }));
        dirs
    }

    pub(super) fn write_frame(pipe: &mut Pipe, opcode: u32, payload: &[u8]) -> Result<(), ()> {
        let mut header = [0u8; 8];
        header[..4].copy_from_slice(&opcode.to_le_bytes());
        header[4..].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        pipe.write_all(&header).map_err(|_| ())?;
        pipe.write_all(payload).map_err(|_| ())?;
        pipe.flush().map_err(|_| ())
    }

    pub(super) fn read_frame(pipe: &mut Pipe) -> Result<(u32, Vec<u8>), ()> {
        let mut header = [0u8; 8];
        pipe.read_exact(&mut header).map_err(|_| ())?;
        let len = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
        if len > 1 << 20 {
            return Err(());
        }
        let mut payload = vec![0u8; len];
        pipe.read_exact(&mut payload).map_err(|_| ())?;
        let opcode = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
        Ok((opcode, payload))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_json_with_place_and_detail() {
        let activity = Activity {
            place: "Baseplate".into(),
            detail: "ServerScript".into(),
            started: 1700000000,
            kind: Kind::Building,
        };
        let json = activity_json(&activity);
        assert!(json.contains(r#""cmd":"SET_ACTIVITY""#));
        assert!(json.contains(r#""details":"Editing Baseplate""#));
        assert!(json.contains(r#""state":"ServerScript""#));
        assert!(json.contains(r#""start":1700000000"#));
        assert!(json.contains(r#""large_image":"building""#));
        assert!(json.contains(r#""small_image":"instudio""#));
    }

    #[test]
    fn activity_json_hidden_names() {
        let activity = Activity {
            place: String::new(),
            detail: String::new(),
            started: 1700000000,
            kind: Kind::Building,
        };
        let json = activity_json(&activity);
        assert!(json.contains(r#""details":"Editing in RbxNative""#));
        assert!(!json.contains(r#""state""#));
    }

    #[test]
    fn json_escape_special_characters() {
        assert_eq!(json_escape(r#"hello "world""#), r#"hello \"world\""#);
        assert_eq!(json_escape("back\\slash"), "back\\\\slash");
        assert_eq!(json_escape("new\nline"), "new\\nline");
        assert_eq!(json_escape("tab\there"), "tab\\there");
    }

    #[test]
    fn json_escape_control_characters() {
        assert_eq!(json_escape("\x01"), "\\u0001");
        assert_eq!(json_escape("\x1f"), "\\u001f");
    }

    #[test]
    fn json_escape_leaves_normal_text() {
        assert_eq!(json_escape("Editing MyPlace"), "Editing MyPlace");
    }

    #[test]
    fn nonce_is_nonempty() {
        let n = nonce();
        assert!(!n.is_empty());
        assert!(n.contains('.'));
    }

    #[test]
    fn now_timestamp_is_recent() {
        assert!(now_timestamp() > 1_577_836_800);
    }

    #[test]
    fn frame_header_layout() {
        let payload = b"hello";
        let mut buf = Vec::new();
        buf.extend_from_slice(&42u32.to_le_bytes());
        buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(payload);

        assert_eq!(u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]), 42);
        assert_eq!(
            u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            payload.len() as u32
        );
        assert_eq!(&buf[8..], payload);
    }

    #[test]
    fn activity_json_escapes_quotes_in_names() {
        let activity = Activity {
            place: r#"My "Cool" Place"#.into(),
            detail: r#"Script"With"Quotes"#.into(),
            started: 0,
            kind: Kind::Building,
        };
        let json = activity_json(&activity);
        assert!(!json.contains(r#"My "Cool" Place"#));
        assert!(json.contains(r#"My \"Cool\" Place"#));
    }

    #[test]
    fn start_and_drop_does_not_panic() {
        let presence = Presence::start(Activity {
            place: "Test".into(),
            detail: String::new(),
            started: now_timestamp(),
            kind: Kind::Building,
        });
        presence.update(Activity {
            place: "Test2".into(),
            detail: "Viewport".into(),
            started: now_timestamp(),
            kind: Kind::Building,
        });
        drop(presence);
    }

    #[cfg(not(windows))]
    #[test]
    fn sandbox_dirs_finds_flatpak_and_snap_clients() {
        let base = std::env::temp_dir().join(format!("rbx-discord-{}", std::process::id()));
        for dir in [
            "app/com.discordapp.Discord",
            ".flatpak/dev.vencord.Vesktop/xdg-run",
            "snap.discord",
            "unrelated",
        ] {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        }
        let dirs = ipc::sandbox_dirs(base.clone());
        std::fs::remove_dir_all(&base).unwrap();
        for dir in [
            "",
            "app/com.discordapp.Discord",
            ".flatpak/dev.vencord.Vesktop/xdg-run",
            "snap.discord",
        ] {
            assert!(
                dirs.contains(&base.join(dir)),
                "{dir} missing from {dirs:?}"
            );
        }
        assert!(!dirs.contains(&base.join("unrelated")));
    }
}
