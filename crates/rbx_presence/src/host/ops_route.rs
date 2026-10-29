//! The op log's routes: `POST …/ops` (a sender's chunks), `POST …/ops/fill`
//! (a log handed over by a peer) and `GET …/ops?since=N` (a long-poll that
//! answers at once when the reader is ahead of this log, so a peer holding
//! a log the relay lacks finds out without waiting).

use std::net::TcpStream;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;

use super::http::{number_query, refuse, respond, Request};
use super::validate::{parse_fill, parse_sealed_ops};
use super::{lock, Refusal, Shared, OPS_REPLY_BYTES, POLL_TIMEOUT};
use crate::schema::OpsPosted;

pub(super) fn serve(
    shared: &Arc<Shared>,
    route: &str,
    request: &Request,
    user_id: u64,
    sig: &str,
    now: u64,
    stream: &mut TcpStream,
) {
    lock(&shared.ops).opened(now);
    if request.method == "GET" {
        return poll(shared, request, stream);
    }
    let body = String::from_utf8_lossy(&request.body);
    let result = if route == "ops/fill" {
        parse_fill(&body).and_then(|fill| {
            let mut ops = lock(&shared.ops);
            ops.fresh(sig, now)?;
            ops.fill(user_id, &fill.log, fill.entries, now)
        })
    } else {
        parse_sealed_ops(&body, user_id).and_then(|sealed| {
            let mut ops = lock(&shared.ops);
            ops.fresh(sig, now)?;
            ops.append(user_id, sealed.client, sealed.cts, now)
        })
    };
    match result {
        Ok(seq) => {
            let log = lock(&shared.ops).log().to_owned();
            shared.ops_changed.notify_all();
            let posted = OpsPosted { seq, log };
            respond(
                stream,
                200,
                &serde_json::to_string(&posted).unwrap_or_default(),
            );
        }
        Err(fault) => refuse(stream, fault),
    }
}

fn poll(shared: &Arc<Shared>, request: &Request, stream: &mut TcpStream) {
    let Some(since) = number_query(request, "since") else {
        return refuse(stream, Refusal::new(400, "schema"));
    };
    let deadline = Instant::now() + POLL_TIMEOUT;
    let mut log = lock(&shared.ops);
    while log.head() == since && !shared.stop.load(Ordering::SeqCst) {
        let Some(left) = deadline.checked_duration_since(Instant::now()) else {
            break;
        };
        log = shared
            .ops_changed
            .wait_timeout(log, left)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
    let body = serde_json::to_string(&log.since(since, OPS_REPLY_BYTES)).unwrap_or_default();
    drop(log);
    respond(stream, 200, &body);
}
