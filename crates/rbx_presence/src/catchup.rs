//! The offline catch-up summary: a value-less journal of property edits,
//! recovered when a collaborator reconnects, aggregated per author and in
//! total. Pinned by `plugins/team-presence/fixtures/catchup.json`.
//!
//! The journal is recovered differently per transport (neither carries edit
//! *values*, and neither records instance add/remove — see the crate README):
//! - [`from_worker`] asks the relay's bounded Durable-Object journal
//!   (`GET /journal`) for the entries it kept.
//! - [`from_ntfy`] re-reads the topic's retained messages and lifts the
//!   `edits` out of each one, dropping the value on the way in.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::client::Config;
use crate::relay::RelayKind;
use crate::{auth, ntfy, relay};

/// One property edit, without its value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub user_id: u64,
    pub name: String,
    pub path: String,
    pub property: String,
    /// Unix milliseconds.
    pub at: u64,
}

/// What one author changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorChange {
    pub user_id: u64,
    pub name: String,
    /// Distinct paths.
    pub instances: usize,
    /// Distinct (path, property) pairs.
    pub properties: usize,
    pub edits: usize,
}

/// Everything that changed, per author and in total.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub authors: Vec<AuthorChange>,
    pub instances: usize,
    pub properties: usize,
    pub edits: usize,
}

#[derive(Default)]
struct Tally<'a> {
    name: &'a str,
    latest: u64,
    paths: HashSet<&'a str>,
    props: HashSet<(&'a str, &'a str)>,
    edits: usize,
}

/// Aggregates `entries`. Authors are ordered by edits descending, then name
/// case-insensitively; an author's name is the one on their newest entry.
pub fn summarize(entries: &[JournalEntry]) -> Summary {
    let mut by_user: HashMap<u64, Tally> = HashMap::new();
    let mut paths = HashSet::new();
    let mut props = HashSet::new();
    for entry in entries {
        paths.insert(entry.path.as_str());
        props.insert((entry.path.as_str(), entry.property.as_str()));
        let tally = by_user.entry(entry.user_id).or_default();
        if tally.edits == 0 || entry.at >= tally.latest {
            tally.name = &entry.name;
            tally.latest = entry.at;
        }
        tally.paths.insert(&entry.path);
        tally.props.insert((&entry.path, &entry.property));
        tally.edits += 1;
    }
    let mut authors: Vec<AuthorChange> = by_user
        .into_iter()
        .map(|(user_id, t)| AuthorChange {
            user_id,
            name: t.name.to_string(),
            instances: t.paths.len(),
            properties: t.props.len(),
            edits: t.edits,
        })
        .collect();
    authors.sort_by(|a, b| {
        b.edits
            .cmp(&a.edits)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.user_id.cmp(&b.user_id))
    });
    Summary {
        authors,
        instances: paths.len(),
        properties: props.len(),
        edits: entries.len(),
    }
}

/// The relay's reply to `GET /journal`.
#[derive(Debug, Deserialize)]
struct JournalReply {
    entries: Vec<crate::schema::SealedJournalEntry>,
}

/// How far back a reconnecting client asks a dumb pub/sub server to replay.
/// ntfy keeps messages ~12 h; this matches the Worker journal's TTL.
const NTFY_LOOKBACK: &str = "12h";

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("rbx-native/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(20)))
        .build()
        .into()
}

/// The summary a reconnecting client builds from the right transport, for
/// edits made after `since_ms`. [`None`] when `config.kind` has no catch-up
/// (there is none to miss).
pub fn fetch(config: &Config, since_ms: u64) -> Result<Summary, String> {
    match config.kind {
        RelayKind::Hosted => {
            // The announced host's journal; nobody hosting means nothing
            // was journaled since.
            let found = crate::announce::find(&config.relay_url, &config.key, &config.room)?;
            match found {
                Some(host) => from_worker(
                    &Config {
                        relay_url: host.url,
                        ..config.clone()
                    },
                    since_ms,
                ),
                None => Ok(summarize(&[])),
            }
        }
        RelayKind::Worker => from_worker(config, since_ms),
        RelayKind::Ntfy => from_ntfy(config, since_ms),
    }
}

/// Reads the Worker relay's bounded journal (signed `GET /journal?since=N`)
/// and summarises it.
pub fn from_worker(config: &Config, since_ms: u64) -> Result<Summary, String> {
    let base = config.relay_url.trim_end_matches('/');
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("worker relay url must be http(s)".into());
    }
    let path = format!("/v1/rooms/{}/journal?since={since_ms}", config.room);
    let ts = crate::now_ms();
    let agent = agent();
    let mut req = agent.get(format!("{base}{path}"));
    for (name, value) in auth::headers(&config.key, "GET", &path, config.user_id, ts, b"", true) {
        req = req.header(name, &value);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("journal: HTTP {status}"));
    }
    let reply: JournalReply =
        serde_json::from_str(&text).map_err(|e| format!("journal reply: {e}"))?;
    let entries: Vec<JournalEntry> = reply
        .entries
        .iter()
        .filter_map(|entry| crate::seal::open_journal(&config.enc_key, &config.room, entry))
        .collect();
    Ok(summarize(&entries))
}

/// Re-reads the ntfy topic's retained messages and summarises the value-less
/// edits lifted out of them (duplicates across retained messages collapsed).
pub fn from_ntfy(config: &Config, since_ms: u64) -> Result<Summary, String> {
    let base = config.relay_url.trim_end_matches('/');
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("ntfy relay url must be http(s)".into());
    }
    let topic = relay::ntfy_topic(&config.room);
    let agent = agent();
    let mut resp = agent
        .get(format!("{base}/{topic}/json"))
        .query("poll", "1")
        .query("since", NTFY_LOOKBACK)
        .call()
        .map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("ntfy journal: HTTP {status}"));
    }
    let now = crate::now_ms();
    let mut seen: HashSet<(u64, String, String, u64)> = HashSet::new();
    let mut entries: Vec<JournalEntry> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("event").and_then(|e| e.as_str()) != Some("message") {
            continue;
        }
        let Some(body) = message.get("message").and_then(|m| m.as_str()) else {
            continue;
        };
        // ntfy keeps ~12 h; its stored timestamps predate `now` by more than
        // the signing skew, so verify with the message's own time.
        let signed = message.get("time").and_then(|t| t.as_u64());
        let check_at = signed.map_or(now, |secs| secs * 1000);
        let Some(state) = ntfy::decode(
            &config.key,
            &config.enc_key,
            &config.room,
            &topic,
            check_at,
            body,
        ) else {
            continue;
        };
        for edit in &state.edits {
            if edit.at <= since_ms {
                continue;
            }
            let dedupe = (
                state.user_id,
                edit.path.clone(),
                edit.property.clone(),
                edit.at,
            );
            if seen.insert(dedupe) {
                entries.push(JournalEntry {
                    user_id: state.user_id,
                    name: state.name.clone(),
                    path: edit.path.clone(),
                    property: edit.property.clone(),
                    at: edit.at,
                });
            }
        }
    }
    Ok(summarize(&entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Fixture {
        entries: Vec<JournalEntry>,
        summary: Summary,
    }

    #[test]
    fn matches_golden_fixture() {
        let fixture: Fixture = serde_json::from_str(include_str!(
            "../../../plugins/team-presence/fixtures/catchup.json"
        ))
        .unwrap();
        assert_eq!(summarize(&fixture.entries), fixture.summary);
    }

    #[test]
    fn empty_journal_is_empty_summary() {
        let summary = summarize(&[]);
        assert!(summary.authors.is_empty());
        assert_eq!(
            (summary.instances, summary.properties, summary.edits),
            (0, 0, 0)
        );
    }

    #[test]
    fn name_comes_from_newest_entry() {
        let entry = |name: &str, at| JournalEntry {
            user_id: 1,
            name: name.into(),
            path: "A".into(),
            property: "P".into(),
            at,
        };
        let summary = summarize(&[entry("New", 20), entry("Old", 10)]);
        assert_eq!(summary.authors[0].name, "New");
    }

    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// Serves one HTTP request with a fixed body, then closes.
    fn serve_once(body: String, content_type: &str) -> (String, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let content_type = content_type.to_owned();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
            let _ = stream.flush();
        });
        (url, handle)
    }

    fn config(url: String, kind: RelayKind) -> Config {
        Config {
            kind,
            relay_url: url,
            room: "room1".into(),
            key: "k".repeat(64),
            enc_key: crate::crypt::enc_key("s", 1),
            user_id: 42,
            hosting: None,
        }
    }

    #[test]
    fn from_worker_summarises_journal() {
        use crate::schema::{Client, Edit, SealedJournalEntry, UserState, SCHEMA_VERSION};
        let enc_key = crate::crypt::enc_key("s", 1);
        let edit = |property: &str, at| Edit {
            path: "Workspace.Part".into(),
            property: property.into(),
            value: "1".into(),
            at,
        };
        let state = UserState {
            v: SCHEMA_VERSION,
            user_id: 7,
            name: "Ada".into(),
            color: "#1c7ed6".into(),
            client: Client::Studio,
            camera: None,
            selection: vec![],
            script: None,
            edits: vec![edit("Size", 5), edit("Pos", 6)],
            seen: 0,
        };
        let sealed = crate::seal::seal_state(&enc_key, "room1", &state);
        let mut entries: Vec<SealedJournalEntry> = sealed
            .j
            .iter()
            .map(|e| SealedJournalEntry {
                user_id: 7,
                at: e.at,
                ct: e.ct.clone(),
            })
            .collect();
        entries.push(SealedJournalEntry {
            user_id: 7,
            at: 99,
            ct: sealed.j[0].ct.clone(),
        });
        let body = serde_json::json!({ "entries": entries }).to_string();
        let (url, handle) = serve_once(body, "application/json");
        let summary = from_worker(&config(url, RelayKind::Worker), 0).unwrap();
        handle.join().unwrap();
        assert_eq!(summary.edits, 2);
        assert_eq!(summary.authors.len(), 1);
        assert_eq!(summary.authors[0].properties, 2);
    }

    #[test]
    fn from_ntfy_lifts_value_less_edits() {
        use crate::schema::{Client, Edit, UserState, SCHEMA_VERSION};
        let key = "k".repeat(64);
        let topic = relay::ntfy_topic("room1");
        let state = UserState {
            v: SCHEMA_VERSION,
            user_id: 9,
            name: "Lin".into(),
            color: "#1c7ed6".into(),
            client: Client::Studio,
            camera: None,
            selection: vec![],
            script: None,
            edits: vec![Edit {
                path: "Lighting.Sky".into(),
                property: "Brightness".into(),
                value: "SECRET".into(),
                at: 1000,
            }],
            seen: 0,
        };
        let now = crate::now_ms();
        let enc_key = crate::crypt::enc_key("s", 1);
        let envelope = ntfy::encode(&key, &enc_key, "room1", &topic, 9, now, &state).unwrap();
        // ntfy's own ndjson: event/message/time wrapping our envelope.
        let line = serde_json::json!({
            "id": "abc", "event": "message", "time": now / 1000, "message": envelope
        })
        .to_string();
        let (url, handle) = serve_once(line, "application/x-ndjson");
        let summary = from_ntfy(&config(url, RelayKind::Ntfy), 0).unwrap();
        handle.join().unwrap();
        assert_eq!(summary.edits, 1);
        assert_eq!(summary.authors[0].user_id, 9);
        assert_eq!(summary.authors[0].name, "Lin");
        // The value never leaves the edit stream: a JournalEntry has no value
        // field, so the summary cannot carry "SECRET".
        assert!(!serde_json::to_string(&summary).unwrap().contains("SECRET"));
    }
}
