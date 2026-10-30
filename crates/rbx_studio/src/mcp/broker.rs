//! What `rbxstudio --mcp` does with each line: answers what every editor
//! would answer alike, joins every editor's row for `list_roblox_studios`,
//! runs `subagent` over the client's sampling, and hands every other tool
//! call to the editor its `studio_id` names.
//!
//! Editors are found through their files in the endpoint directory, read
//! again on every call, so one opened after the client connected is
//! reachable at once. A file whose port refuses the connection belongs to an
//! editor that did not close cleanly, and is removed.

mod subagent;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use super::endpoint::{file_of, Endpoint};
use super::protocol::{self, Message, METHOD_NOT_FOUND};
use super::tools::{self, LIST_STUDIOS, STUDIOS_KEY};

/// How long an editor may take over one answer before the call is reported
/// as failed. Every answer waits for the editor's UI thread, so this is
/// generous; it only has to keep a frozen editor from hanging the client.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(30);

/// The answer timeout for one tool: running Luau, starting a play session
/// and rendering a frame take far longer than reading the tree.
pub(super) fn answer_timeout(tool: &str) -> Duration {
    match tool {
        "execute_luau" => Duration::from_secs(120),
        "start_stop_play" => Duration::from_secs(240),
        "screen_capture" => Duration::from_secs(60),
        // A walk first waits, like Studio's, for a character with a
        // Humanoid; the editor gives up on it after 10 minutes.
        "character_navigation" => Duration::from_secs(11 * 60),
        _ => ANSWER_TIMEOUT,
    }
}

pub(crate) const NOT_RUNNING: &str = "no editor is serving MCP. Open a place in rbxstudio with \
Settings › Beta features › MCP server switched on.";

/// Asks the MCP client something, such as `sampling/createMessage`.
pub(crate) trait Client {
    fn request(&mut self, method: &str, params: Value) -> Result<Value, String>;
}

/// One editor's connection, kept for the next call to it.
struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

pub(crate) struct Broker {
    dir: PathBuf,
    connections: HashMap<String, Connection>,
    /// The client advertised `capabilities.sampling`.
    sampling: bool,
    /// The client advertised `capabilities.sampling.tools`.
    sampling_tools: bool,
}

impl Broker {
    pub(crate) fn new(dir: PathBuf) -> Self {
        Broker {
            dir,
            connections: HashMap::new(),
            sampling: false,
            sampling_tools: false,
        }
    }

    /// The reply to one line from the client, or `None` when it needs none.
    pub(crate) fn handle(&mut self, line: &str, client: &mut dyn Client) -> Option<String> {
        let (id, method, params) = match protocol::parse(line) {
            Message::Request { id, method, params } => (id, method, params),
            Message::Silent => return None,
            Message::Invalid(reply) => return Some(reply),
        };
        if method == "initialize" {
            let sampling = params
                .get("capabilities")
                .and_then(|capabilities| capabilities.get("sampling"))
                .filter(|sampling| !sampling.is_null());
            self.sampling = sampling.is_some();
            self.sampling_tools = sampling
                .and_then(|sampling| sampling.get("tools"))
                .is_some_and(|tools| !tools.is_null());
        }
        if let Some(outcome) = protocol::answer_alone(&method, &params) {
            return Some(protocol::reply(id, outcome));
        }
        if method != "tools/call" {
            let failure = (METHOD_NOT_FOUND, format!("method not found: {method}"));
            return Some(protocol::reply(id, Err(failure)));
        }
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        if name == subagent::SUBAGENT {
            let arguments = params.get("arguments").cloned().unwrap_or(Value::Null);
            return Some(protocol::reply(id, Ok(self.subagent(&arguments, client))));
        }
        if name == LIST_STUDIOS {
            let studios = json!({ STUDIOS_KEY: self.studios() }).to_string();
            return Some(protocol::reply(
                id,
                Ok(tools::result(&Value::String(studios), false)),
            ));
        }
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let arguments = params.get("arguments").cloned().unwrap_or(json!({}));
        if let Some(answer) = tools::without_editor(name, &arguments) {
            return Some(protocol::reply(id, Ok(answer)));
        }
        let wanted = params
            .get("arguments")
            .and_then(|arguments| arguments.get("studio_id"))
            .and_then(Value::as_str);
        Some(self.call(id, wanted, name, line))
    }

    /// Hands the tools/call `line` for `tool` to the editor `wanted` routes
    /// to; the reply line, also when no editor answers.
    fn call(&mut self, id: Value, wanted: Option<&str>, tool: &str, line: &str) -> String {
        let timeout = answer_timeout(tool);
        let answer = self
            .route(wanted)
            .and_then(|studio| self.forward(&studio, line, timeout));
        answer.unwrap_or_else(|message| {
            protocol::reply(id, Ok(tools::result(&Value::String(message), true)))
        })
    }

    /// Every editor that answers, each listing itself: an editor answers
    /// `list_roblox_studios` with its own row, so the rows are only joined
    /// here, never rebuilt.
    fn studios(&mut self) -> Vec<Value> {
        let ask = json!({
            "jsonrpc": "2.0",
            "id": 0,
            "method": "tools/call",
            "params": { "name": LIST_STUDIOS },
        })
        .to_string();
        self.reachable()
            .iter()
            .filter_map(|studio| {
                let answer: Value =
                    serde_json::from_str(&self.forward(studio, &ask, ANSWER_TIMEOUT).ok()?).ok()?;
                let text = answer["result"]["content"][0]["text"].as_str()?;
                let listed: Value = serde_json::from_str(text).ok()?;
                listed.get(STUDIOS_KEY)?.as_array().cloned()
            })
            .flatten()
            .collect()
    }

    /// The editor a call is for: the one `wanted` names, or the only one.
    fn route(&mut self, wanted: Option<&str>) -> Result<Endpoint, String> {
        if let Some(wanted) = wanted {
            return discover(&self.dir)
                .into_iter()
                .find(|studio| studio.studio_id == wanted)
                .ok_or_else(|| {
                    format!("no editor is studio {wanted}; call {LIST_STUDIOS} for the open ones")
                });
        }
        let mut studios = self.reachable();
        match studios.len() {
            0 => Err(NOT_RUNNING.to_owned()),
            1 => Ok(studios.remove(0)),
            count => Err(format!(
                "{count} editors are open; pass the studio_id of the one you mean \
                 ({LIST_STUDIOS} lists them)"
            )),
        }
    }

    /// The editors whose files are there and whose ports take a connection.
    fn reachable(&mut self) -> Vec<Endpoint> {
        let studios = discover(&self.dir);
        self.connections
            .retain(|id, _| studios.iter().any(|studio| &studio.studio_id == id));
        studios
            .into_iter()
            .filter(|studio| self.connection(studio).is_ok())
            .collect()
    }

    fn connection(&mut self, studio: &Endpoint) -> Result<&mut Connection, String> {
        if !self.connections.contains_key(&studio.studio_id) {
            let stream = match connect(studio) {
                Ok(stream) => stream,
                Err(err) => {
                    if err.kind() == ErrorKind::ConnectionRefused {
                        let _ = std::fs::remove_file(file_of(&self.dir, &studio.studio_id));
                    }
                    return Err(format!(
                        "studio {} cannot be reached: {err}",
                        studio.studio_id
                    ));
                }
            };
            let writer = stream
                .try_clone()
                .map_err(|err| format!("studio {}: {err}", studio.studio_id))?;
            let connection = Connection {
                reader: BufReader::new(stream),
                writer,
            };
            self.connections
                .insert(studio.studio_id.clone(), connection);
        }
        Ok(self
            .connections
            .get_mut(&studio.studio_id)
            .expect("inserted above"))
    }

    /// Sends one request line to `studio` and reads its one-line answer,
    /// waiting at most `timeout` for it.
    fn forward(
        &mut self,
        studio: &Endpoint,
        line: &str,
        timeout: Duration,
    ) -> Result<String, String> {
        let connection = self.connection(studio)?;
        let mut answer = String::new();
        let sent = connection
            .reader
            .get_ref()
            .set_read_timeout(Some(timeout))
            .and_then(|()| writeln!(connection.writer, "{line}"))
            .and_then(|()| connection.writer.flush())
            .and_then(|()| connection.reader.read_line(&mut answer));
        match sent {
            Ok(read) if read > 0 => Ok(answer.trim_end().to_owned()),
            // A late answer would be read as the next call's, so the
            // connection is not kept past a failure.
            failed => {
                self.connections.remove(&studio.studio_id);
                Err(unanswered(&studio.studio_id, timeout, failed.err()))
            }
        }
    }
}

/// What the client is told when an editor gave no answer within `timeout`:
/// `None` is a connection the editor closed.
fn unanswered(studio_id: &str, timeout: Duration, failure: Option<std::io::Error>) -> String {
    match failure {
        // The request is already queued in the editor and still runs once its
        // UI thread gets to it, so a client that simply retried an edit would
        // make it twice.
        Some(err) if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => format!(
            "studio {studio_id} did not answer within {} s; the call may still apply once it \
             does, so check its effect before trying it again",
            timeout.as_secs()
        ),
        failure => {
            let why = failure.map_or("it closed".to_owned(), |err| err.to_string());
            format!("studio {studio_id} stopped answering: {why}")
        }
    }
}

/// Every editor's file in `dir`, in a stable order.
pub(crate) fn discover(dir: &Path) -> Vec<Endpoint> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut studios: Vec<Endpoint> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .filter_map(|path| Endpoint::load(&path))
        .collect();
    studios.sort_by(|a, b| a.studio_id.cmp(&b.studio_id));
    studios
}

/// Connects to an editor and introduces itself with the token.
pub(crate) fn connect(endpoint: &Endpoint) -> std::io::Result<TcpStream> {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, endpoint.port))?;
    stream.set_read_timeout(Some(ANSWER_TIMEOUT))?;
    writeln!(stream, "{}", endpoint.token)?;
    stream.flush()?;
    Ok(stream)
}
