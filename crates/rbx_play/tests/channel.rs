use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::{Duration, Instant};

use rbx_play::channel::{ChannelServer, MAX_BODY};
use rbx_play::protocol::{Command, CommandBody, LogLevel, Side, Token, Uplink, TOKEN_HEADER};

const HOLD: Duration = Duration::from_millis(300);

/// Sends one HTTP/1.1 request on a fresh connection and returns the status
/// code and the body. Write errors are ignored so that a server which
/// answers and closes early (e.g. `413`) still yields its reply.
fn request(port: u16, method: &str, path: &str, token: Option<&str>, body: &[u8]) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to channel");
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .expect("set read timeout");

    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    if let Some(t) = token {
        head.push_str(&format!("{TOKEN_HEADER}: {t}\r\n"));
    }
    head.push_str("\r\n");

    let _ = stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(body));
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw);

    let text = String::from_utf8_lossy(&raw).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
    let status = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .expect("status line");
    (status, body.to_owned())
}

fn start() -> (ChannelServer, String) {
    let token = Token::random();
    let secret = token.as_str().to_owned();
    let server = ChannelServer::start(token, HOLD).expect("start channel");
    (server, secret)
}

#[test]
fn healthz_with_token_answers_ok() {
    let (server, token) = start();
    let (status, body) = request(server.port(), "GET", "/healthz", Some(&token), &[]);
    assert_eq!(status, 200);
    assert_eq!(body, "ok");
}

#[test]
fn missing_or_wrong_token_answers_404_on_every_route() {
    let (server, token) = start();
    let wrong = "f".repeat(64);
    let routes: [(&str, &str, &[u8]); 3] = [
        ("GET", "/healthz", &[]),
        ("POST", "/v1/ingest", b"[]"),
        ("GET", "/v1/poll?since=0", &[]),
    ];
    for (method, path, body) in routes {
        let (status, _) = request(server.port(), method, path, None, body);
        assert_eq!(status, 404, "missing token on {method} {path}");
        let (status, _) = request(server.port(), method, path, Some(&wrong), body);
        assert_eq!(status, 404, "wrong token on {method} {path}");
        assert_ne!(wrong, token);
    }
}

#[test]
fn ingest_two_uplinks_returns_204_and_events_arrive_in_order() {
    let (server, token) = start();
    let batch = vec![
        Uplink::Log {
            side: Side::Server,
            level: LogLevel::Output,
            message: "hello".to_owned(),
            time: 1.5,
        },
        Uplink::Hello {
            session: "s1".to_owned(),
            place_version: 3,
            http_enabled: true,
            loadstring_enabled: false,
            is_studio: true,
            job_id: String::new(),
        },
    ];
    let body = serde_json::to_vec(&batch).expect("encode batch");

    let (status, _) = request(server.port(), "POST", "/v1/ingest", Some(&token), &body);
    assert_eq!(status, 204);

    let events = server.events();
    let first = events
        .recv_timeout(Duration::from_secs(2))
        .expect("first event");
    let second = events
        .recv_timeout(Duration::from_secs(2))
        .expect("second event");
    assert_eq!(first, batch[0]);
    assert_eq!(second, batch[1]);
}

#[test]
fn malformed_json_answers_400() {
    let (server, token) = start();
    let (status, _) = request(server.port(), "POST", "/v1/ingest", Some(&token), b"[{");
    assert_eq!(status, 400);
}

#[test]
fn poll_with_nothing_queued_answers_204_after_hold() {
    let (server, token) = start();
    let started = Instant::now();
    let (status, _) = request(server.port(), "GET", "/v1/poll?since=0", Some(&token), &[]);
    let elapsed = started.elapsed();
    assert_eq!(status, 204);
    assert!(
        elapsed >= Duration::from_millis(250),
        "poll returned after {elapsed:?}, expected roughly the hold"
    );
}

#[test]
fn send_then_poll_returns_the_command_as_json() {
    let (server, token) = start();
    let id = server.send(CommandBody::Eval {
        source: "return 1".to_owned(),
    });
    assert_eq!(id, 1);

    let (status, body) = request(server.port(), "GET", "/v1/poll?since=0", Some(&token), &[]);
    assert_eq!(status, 200);
    let commands: Vec<Command> = serde_json::from_str(&body).expect("poll body is a command array");
    assert_eq!(commands.len(), 1);
    assert_eq!(commands[0].id, 1);
    assert_eq!(
        commands[0].body,
        CommandBody::Eval {
            source: "return 1".to_owned()
        }
    );

    let raw: serde_json::Value = serde_json::from_str(&body).expect("valid json");
    assert_eq!(raw[0]["id"], 1);
    assert_eq!(raw[0]["type"], "eval");
}

#[test]
fn poll_started_before_send_returns_early() {
    let (server, token) = start();
    let port = server.port();
    let poller = thread::spawn(move || {
        let started = Instant::now();
        let (status, body) = request(port, "GET", "/v1/poll?since=0", Some(&token), &[]);
        (status, body, started.elapsed())
    });

    thread::sleep(Duration::from_millis(100));
    server.send(CommandBody::Shutdown);

    let (status, body, elapsed) = poller.join().expect("poller thread");
    assert_eq!(status, 200);
    assert!(body.contains("\"id\":1"), "body was {body}");
    assert!(
        elapsed < Duration::from_millis(250),
        "poll took {elapsed:?}, expected an early return well before the hold"
    );
}

#[test]
fn poll_since_latest_id_after_one_send_answers_204() {
    let (server, token) = start();
    server.send(CommandBody::Shutdown);
    let (status, _) = request(server.port(), "GET", "/v1/poll?since=1", Some(&token), &[]);
    assert_eq!(status, 204);
}

#[test]
fn ack_removes_command_from_queue() {
    let (server, token) = start();
    server.send(CommandBody::Eval {
        source: "return 2".to_owned(),
    });

    let ack = serde_json::to_vec(&vec![Uplink::Ack {
        id: 1,
        ok: true,
        result: "2".to_owned(),
    }])
    .expect("encode ack");
    let (status, _) = request(server.port(), "POST", "/v1/ingest", Some(&token), &ack);
    assert_eq!(status, 204);

    let (status, _) = request(server.port(), "GET", "/v1/poll?since=0", Some(&token), &[]);
    assert_eq!(status, 204);
}

#[test]
fn three_sends_get_ids_one_two_three() {
    let (server, _token) = start();
    let ids = [
        server.send(CommandBody::Shutdown),
        server.send(CommandBody::Shutdown),
        server.send(CommandBody::Shutdown),
    ];
    assert_eq!(ids, [1, 2, 3]);
}

#[test]
fn rotate_token_swaps_the_accepted_token() {
    let (server, old) = start();
    let new = Token::random();
    let new_secret = new.as_str().to_owned();
    server.rotate_token(new);
    let (status, _) = request(server.port(), "GET", "/healthz", Some(&old), &[]);
    assert_eq!(status, 404);
    let (status, _) = request(server.port(), "GET", "/healthz", Some(&new_secret), &[]);
    assert_eq!(status, 200);
}

#[test]
fn rotate_token_drops_queued_commands_and_keeps_ids_increasing() {
    let (server, _old) = start();
    assert_eq!(server.send(CommandBody::Shutdown), 1);
    let new = Token::random();
    let new_secret = new.as_str().to_owned();
    server.rotate_token(new);
    let (status, _) = request(
        server.port(),
        "GET",
        "/v1/poll?since=0",
        Some(&new_secret),
        &[],
    );
    assert_eq!(status, 204);
    assert_eq!(server.send(CommandBody::Shutdown), 2);
}

#[test]
fn rotate_token_wakes_held_poll_with_204() {
    let (server, old) = start();
    let port = server.port();
    let poller = thread::spawn(move || {
        let started = Instant::now();
        let (status, _) = request(port, "GET", "/v1/poll?since=0", Some(&old), &[]);
        (status, started.elapsed())
    });
    thread::sleep(Duration::from_millis(100));
    server.rotate_token(Token::random());
    let (status, elapsed) = poller.join().expect("poller thread");
    assert_eq!(status, 204);
    assert!(
        elapsed < Duration::from_millis(250),
        "poll took {elapsed:?}, expected an early return well before the hold"
    );
}

#[test]
fn body_over_max_answers_413() {
    let (server, token) = start();
    let oversized = vec![b' '; MAX_BODY + 1];
    let (status, _) = request(
        server.port(),
        "POST",
        "/v1/ingest",
        Some(&token),
        &oversized,
    );
    assert_eq!(status, 413);
}
