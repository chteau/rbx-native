//! Test connection: runs the bridge the way a client would, says hello,
//! lists the tools and the open editors, and reports what came back.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::mcp::tools::{LIST_STUDIOS, STUDIOS_KEY};

const TIMEOUT: Duration = Duration::from_secs(20);

/// What the bridge answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Probe {
    pub(crate) tools: usize,
    /// The `studio_id` of every editor it reached.
    pub(crate) studios: Vec<String>,
}

/// Runs `exe --mcp` with `env` and asks it what a client asks first. Blocks
/// for up to [`TIMEOUT`]; run it off the UI thread, which has to answer the
/// editor's own part of `list_roblox_studios` meanwhile.
pub(crate) fn run(exe: &Path, env: &[(String, String)]) -> Result<Probe, String> {
    let mut command = Command::new(exe);
    command
        .arg(crate::mcp::BRIDGE_FLAG)
        .envs(env.iter().map(|(key, value)| (key, value)))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(
        &mut command,
        crate::luau_lsp::CREATE_NO_WINDOW,
    );
    let mut child = command
        .spawn()
        .map_err(|error| format!("couldn't start {}: {error}", exe.display()))?;
    let outcome = converse(&mut child);
    let _ = child.kill();
    let _ = child.wait();
    outcome
}

fn converse(child: &mut std::process::Child) -> Result<Probe, String> {
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        return Err("the bridge's pipes weren't opened".to_owned());
    };
    let (sender, lines) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    let hello = json!({
        "protocolVersion": "2025-06-18",
        "capabilities": {},
        "clientInfo": { "name": "rbx-native-settings", "version": env!("CARGO_PKG_VERSION") },
    });
    let messages = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": hello }),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
        json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }),
        json!({ "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": LIST_STUDIOS, "arguments": {} } }),
    ];
    for message in &messages {
        writeln!(stdin, "{message}")
            .and_then(|()| stdin.flush())
            .map_err(|error| format!("the bridge stopped reading: {error}"))?;
    }
    let deadline = Instant::now() + TIMEOUT;
    let mut answers: [Option<Value>; 3] = [None, None, None];
    while answers.iter().any(Option::is_none) {
        let left = deadline.saturating_duration_since(Instant::now());
        let line = match lines.recv_timeout(left) {
            Ok(line) => line,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                return Err("the bridge didn't answer within 20 seconds".to_owned())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err("the bridge exited before answering".to_owned())
            }
        };
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(slot) = message["id"]
            .as_u64()
            .and_then(|id| answers.get_mut((id as usize).wrapping_sub(1)))
        {
            *slot = Some(message);
        }
    }
    let [Some(hello), Some(tools), Some(studios)] = answers else {
        unreachable!("the loop ends once every answer is in");
    };
    read(&hello, &tools, &studios)
}

/// The probe's result from the three answers.
pub(super) fn read(hello: &Value, tools: &Value, studios: &Value) -> Result<Probe, String> {
    for answer in [hello, tools, studios] {
        if let Some(error) = answer.get("error") {
            let text = error["message"].as_str().unwrap_or("unknown error");
            return Err(format!("the bridge answered with an error: {text}"));
        }
    }
    if hello["result"]["serverInfo"]["name"] != "rbx-native" {
        return Err("something other than RbxNative's bridge answered".to_owned());
    }
    let tools = tools["result"]["tools"].as_array().map_or(0, Vec::len);
    let text = studios["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    let studios = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|listed| listed.get(STUDIOS_KEY).cloned())
        .and_then(|rows| rows.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|row| row["id"].as_str().map(str::to_owned))
        .collect();
    Ok(Probe { tools, studios })
}
