use super::*;
use crate::schema::Client;

fn post(log: &mut OpLog, user_id: u64, cts: &[&str], now: u64) -> Result<u64, Refusal> {
    log.append(
        user_id,
        Client::Studio,
        cts.iter().map(|c| (*c).to_owned()).collect(),
        now,
    )
}

fn entry(seq: u64, ct: &str) -> LoggedOp {
    LoggedOp {
        seq,
        user_id: 9,
        client: Client::RbxNative,
        ct: ct.to_owned(),
    }
}

fn seqs(log: &OpLog, since: u64) -> Vec<u64> {
    log.since(since, OPS_REPLY_BYTES)
        .entries
        .iter()
        .map(|e| e.seq)
        .collect()
}

#[test]
fn a_signature_is_taken_once() {
    let mut log = OpLog::default();
    assert!(log.fresh("abc", 1_000).is_ok());
    assert_eq!(log.fresh("abc", 2_000), Err(Refusal::new(401, "replay")));
    assert!(log
        .fresh("abc", 1_000 + 3 * super::super::room::CLOCK_SKEW_MS)
        .is_ok());
}

#[test]
fn seqs_start_at_one_and_follow_append_order() {
    let mut log = OpLog::default();
    assert_eq!(log.since(0, OPS_REPLY_BYTES).log, "");
    assert_eq!(post(&mut log, 1, &["AA"], 0), Ok(1));
    assert_eq!(post(&mut log, 2, &["BB", "CC"], 0), Ok(3));
    let reply = log.since(0, OPS_REPLY_BYTES);
    assert_eq!((reply.head, reply.floor), (3, 1));
    assert_eq!(reply.log.len(), 32);
    assert_eq!(seqs(&log, 0), [1, 2, 3]);
    assert_eq!(log.since(1, OPS_REPLY_BYTES).entries[0].ct, "BB");
    assert_eq!(log.since(2, OPS_REPLY_BYTES).entries[0].user_id, 2);
}

#[test]
fn trimming_at_the_cap_moves_the_floor() {
    let mut log = OpLog::new(8, 0);
    for _ in 0..4 {
        post(&mut log, 1, &["AAAA"], 0).unwrap();
    }
    let reply = log.since(0, OPS_REPLY_BYTES);
    assert_eq!((reply.head, reply.floor), (4, 3));
    assert_eq!(seqs(&log, 0), [3, 4]);
    assert_eq!(log.since(2, OPS_REPLY_BYTES).entries.len(), 2);
}

#[test]
fn an_entry_over_the_cap_empties_the_log() {
    let mut log = OpLog::new(2, 0);
    post(&mut log, 1, &["AAAA"], 0).unwrap();
    let reply = log.since(0, OPS_REPLY_BYTES);
    assert_eq!((reply.head, reply.floor), (1, 2));
    assert!(reply.entries.is_empty());
}

#[test]
fn since_past_head_is_empty() {
    let mut log = OpLog::default();
    post(&mut log, 1, &["AA"], 0).unwrap();
    let reply = log.since(5, OPS_REPLY_BYTES);
    assert!(reply.entries.is_empty());
    assert_eq!(reply.head, 1);
    assert!(log.since(1, OPS_REPLY_BYTES).entries.is_empty());
}

#[test]
fn a_reply_stops_at_its_byte_cap_but_never_empty() {
    let mut log = OpLog::default();
    post(&mut log, 1, &["AAAA", "AAAA", "AAAA"], 0).unwrap();
    assert_eq!(log.since(0, 8).entries.len(), 2);
    assert_eq!(log.since(0, 1).entries.len(), 1);
}

#[test]
fn the_rate_limit_counts_requests_not_chunks() {
    let mut log = OpLog::default();
    let many = vec!["AA"; 500];
    for _ in 0..64 {
        post(&mut log, 1, &many, 1000).unwrap();
    }
    assert_eq!(
        post(&mut log, 1, &["AA"], 1000),
        Err(Refusal::new(429, "rate"))
    );
    assert!(post(&mut log, 2, &["AA"], 1000).is_ok());
    assert!(post(&mut log, 1, &["AA"], 1100).is_ok());
}

#[test]
fn an_empty_host_log_waits_for_a_fill_then_takes_posts() {
    let mut log = OpLog::new(OPS_LOG_BYTES, 30_000);
    log.opened(1_000);
    assert_eq!(
        post(&mut log, 1, &["AA"], 2_000),
        Err(Refusal::new(409, "log"))
    );
    assert_eq!(post(&mut log, 1, &["AA"], 31_000), Ok(1));
}

#[test]
fn a_fill_hands_an_empty_relay_the_old_log_with_its_seqs() {
    let mut log = OpLog::new(OPS_LOG_BYTES, 30_000);
    log.opened(0);
    let old = vec![entry(5, "E5"), entry(6, "E6"), entry(7, "E7")];
    assert_eq!(log.fill(1, "abc", old, 10), Ok(7));
    let reply = log.since(0, OPS_REPLY_BYTES);
    assert_eq!((reply.head, reply.floor, reply.log.as_str()), (7, 5, "abc"));
    assert_eq!(reply.entries[0], entry(5, "E5"));
    // A peer that had read up to 6 carries on, and posts go on from 7.
    assert_eq!(seqs(&log, 6), [7]);
    assert_eq!(post(&mut log, 2, &["NEW"], 20), Ok(8));
}

#[test]
fn a_tail_fill_extends_the_same_log_and_skips_what_is_held() {
    let mut log = OpLog::default();
    log.fill(1, "abc", vec![entry(1, "E1"), entry(2, "E2")], 0)
        .unwrap();
    let tail = vec![entry(2, "E2"), entry(3, "E3"), entry(4, "E4")];
    assert_eq!(log.fill(2, "abc", tail.clone(), 0), Ok(4));
    assert_eq!(log.fill(3, "abc", tail, 0), Ok(4));
    assert_eq!(seqs(&log, 0), [1, 2, 3, 4]);
}

#[test]
fn a_fill_from_another_log_or_past_a_gap_is_refused() {
    let mut log = OpLog::default();
    log.fill(1, "abc", vec![entry(1, "E1")], 0).unwrap();
    assert_eq!(
        log.fill(1, "xyz", vec![entry(2, "E2")], 0),
        Err(Refusal::new(409, "log"))
    );
    assert_eq!(
        log.fill(1, "abc", vec![entry(3, "E3")], 0),
        Err(Refusal::new(409, "log"))
    );
    let mut claimed = OpLog::default();
    post(&mut claimed, 1, &["AA"], 0).unwrap();
    assert_eq!(
        claimed.fill(1, "abc", vec![entry(1, "E1")], 0),
        Err(Refusal::new(409, "log"))
    );
}
