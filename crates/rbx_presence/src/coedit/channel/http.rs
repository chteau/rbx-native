//! The op log's three signed requests.

use std::time::Duration;

use super::Config;
use crate::coedit::chunk::Chunk;
use crate::crypt;
use crate::schema::{LoggedOp, OpsFill, OpsReply, SealedOps, SCHEMA_VERSION};
use crate::{auth, now_ms};

pub(super) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("rbx-native/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into()
}

fn base(config: &Config) -> &str {
    config.relay_url.trim_end_matches('/')
}

fn ops_path(config: &Config) -> String {
    format!("/v1/rooms/{}/ops", config.room)
}

/// `chunk` sealed for this editor, as the log will carry it.
pub(super) fn seal(config: &Config, chunk: &Chunk) -> Result<String, String> {
    let plain = serde_json::to_vec(chunk).map_err(|e| e.to_string())?;
    let aad = crypt::ops_aad(&config.room, config.user_id, config.client);
    Ok(crypt::b64_encode(&crypt::seal(
        &config.enc_key,
        &aad,
        &plain,
    )))
}

fn post_json(agent: &ureq::Agent, config: &Config, path: &str, body: &str) -> Result<(), String> {
    let mut req = agent
        .post(format!("{}{path}", base(config)))
        .header("Content-Type", "application/json");
    for (name, value) in auth::headers(
        &config.key,
        "POST",
        path,
        config.user_id,
        now_ms(),
        body.as_bytes(),
        true,
    ) {
        req = req.header(name, &value);
    }
    let resp = req.send(body).map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(format!("ops {path}: HTTP {status}"))
    }
}

/// Appends sealed chunks to the log, in order, in one request.
pub(super) fn post(agent: &ureq::Agent, config: &Config, cts: Vec<String>) -> Result<(), String> {
    let sealed = SealedOps {
        v: SCHEMA_VERSION,
        user_id: config.user_id,
        client: config.client,
        cts,
    };
    let body = serde_json::to_string(&sealed).map_err(|e| e.to_string())?;
    post_json(agent, config, &ops_path(config), &body)
}

/// Hands the relay entries of `log` it lacks.
pub(super) fn fill(
    agent: &ureq::Agent,
    config: &Config,
    log: &str,
    entries: Vec<LoggedOp>,
) -> Result<(), String> {
    let fill = OpsFill {
        v: SCHEMA_VERSION,
        log: log.to_owned(),
        entries,
    };
    let body = serde_json::to_string(&fill).map_err(|e| e.to_string())?;
    post_json(agent, config, &format!("{}/fill", ops_path(config)), &body)
}

pub(super) fn fetch(agent: &ureq::Agent, config: &Config, since: u64) -> Result<OpsReply, String> {
    let path = format!("{}?since={since}", ops_path(config));
    let mut req = agent.get(format!("{}{path}", base(config)));
    for (name, value) in auth::headers(
        &config.key,
        "GET",
        &path,
        config.user_id,
        now_ms(),
        b"",
        true,
    ) {
        req = req.header(name, &value);
    }
    let mut resp = req.call().map_err(|e| e.to_string())?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| e.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!("ops poll: HTTP {status}"));
    }
    serde_json::from_str(&text).map_err(|e| format!("ops reply: {e}"))
}
