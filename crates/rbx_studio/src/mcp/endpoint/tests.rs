use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::{file_of, Endpoint, Server};
use crate::mcp::broker;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rbx-native-mcp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Answers whatever arrives, the way the shell's timer does, until `done`.
fn answer_until(server: &Server, done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() && Instant::now() < deadline {
        for request in server.drain() {
            let reply = format!("echo {}", request.line());
            request.answer(Some(reply));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_endpoint_file_round_trips_and_rejects_what_it_cannot_read() {
    let endpoint = Endpoint {
        studio_id: "a1".to_owned(),
        port: 51234,
        token: "abc".to_owned(),
    };
    assert_eq!(Endpoint::from_json(&endpoint.to_json()), Some(endpoint));
    assert_eq!(
        Endpoint::from_json(r#"{"studio_id":"a","port":70000,"token":"x"}"#),
        None
    );
    assert_eq!(
        Endpoint::from_json(r#"{"port":1,"token":"x"}"#),
        None,
        "a file without an id cannot be routed to"
    );
    assert_eq!(Endpoint::from_json("nonsense"), None);
}

#[test]
fn a_client_with_the_token_is_answered_line_by_line() {
    let dir = scratch("answered");
    let server = Server::start(&dir).unwrap();
    let endpoint = server.endpoint().clone();
    assert_eq!(
        Endpoint::load(&file_of(&dir, &endpoint.studio_id)),
        Some(endpoint.clone())
    );
    assert_eq!(endpoint.token.len(), 32);
    assert_eq!(endpoint.studio_id.len(), 16);

    let stream = broker::connect(&endpoint).unwrap();
    let client = std::thread::spawn(move || {
        let mut writer = stream.try_clone().unwrap();
        writeln!(writer, "first\n\nsecond").unwrap();
        let mut lines = BufReader::new(stream).lines();
        [
            lines.next().unwrap().unwrap(),
            lines.next().unwrap().unwrap(),
        ]
    });
    answer_until(&server, || client.is_finished());
    assert_eq!(client.join().unwrap(), ["echo first", "echo second"]);
}

#[test]
fn a_notification_sends_nothing_back() {
    let dir = scratch("silent");
    let server = Server::start(&dir).unwrap();
    let stream = broker::connect(server.endpoint()).unwrap();
    let client = std::thread::spawn(move || {
        let mut writer = stream.try_clone().unwrap();
        writeln!(writer, "quiet\nloud").unwrap();
        BufReader::new(stream).lines().next().unwrap().unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while !client.is_finished() && Instant::now() < deadline {
        for request in server.drain() {
            let reply = (request.line() == "loud").then(|| "heard".to_owned());
            request.answer(reply);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(client.join().unwrap(), "heard");
}

#[test]
fn a_client_without_the_token_is_hung_up_on_unheard() {
    let dir = scratch("refused");
    let server = Server::start(&dir).unwrap();
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", server.endpoint().port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    writeln!(stream, "not-the-token\nhello").unwrap();
    let mut rest = Vec::new();
    let read = stream.read_to_end(&mut rest);
    assert!(read.is_err() || rest.is_empty(), "{read:?} {rest:?}");
    assert!(server.drain().is_empty());
}

#[test]
fn every_editor_has_its_own_file_and_takes_only_its_own_back() {
    let dir = scratch("dropped");
    let first = Server::start(&dir).unwrap();
    let second = Server::start(&dir).unwrap();
    let (first_id, second_id) = (
        first.endpoint().studio_id.clone(),
        second.endpoint().studio_id.clone(),
    );
    assert_ne!(first_id, second_id);
    drop(first);
    assert!(!file_of(&dir, &first_id).exists());
    assert_eq!(
        Endpoint::load(&file_of(&dir, &second_id)).as_ref(),
        Some(second.endpoint())
    );
    drop(second);
    assert!(!file_of(&dir, &second_id).exists());
}

#[test]
fn an_id_that_is_not_hex_is_not_an_endpoint() {
    // The id names the file a dead editor's endpoint is removed by.
    for id in ["../x", "a1.json", "", "a 1"] {
        let text = format!(r#"{{"studio_id":"{id}","port":1,"token":"x"}}"#);
        assert_eq!(Endpoint::from_json(&text), None, "{id}");
    }
}

#[cfg(unix)]
#[test]
fn only_the_owner_can_read_the_token() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("private");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o775)).unwrap();
    let server = Server::start(&dir).unwrap();
    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&file_of(&dir, &server.endpoint().studio_id)), 0o600);
}

/// What a connection that never shows the token gets back: nothing, and
/// the connection closed.
fn hung_up(mut stream: std::net::TcpStream) -> bool {
    stream
        .set_read_timeout(Some(Duration::from_secs(20)))
        .unwrap();
    let mut rest = Vec::new();
    match stream.read_to_end(&mut rest) {
        Ok(_) => rest.is_empty(),
        // Data left unread when the editor closes makes it a reset rather
        // than an orderly end; only waiting out the timeout is not hung up.
        Err(err) => !matches!(
            err.kind(),
            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
        ),
    }
}

#[test]
fn a_first_line_longer_than_any_token_is_refused_unread() {
    let dir = scratch("long");
    let server = Server::start(&dir).unwrap();
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", server.endpoint().port)).unwrap();
    // Far past the limit and without a newline, as an endless line would be.
    let _ = stream.write_all(&[b'a'; 4096]);
    assert!(hung_up(stream));
    assert!(server.drain().is_empty());
}

#[test]
fn a_connection_that_never_shows_the_token_is_dropped() {
    let dir = scratch("idle");
    let server = Server::start(&dir).unwrap();
    let stream = std::net::TcpStream::connect(("127.0.0.1", server.endpoint().port)).unwrap();
    let started = Instant::now();
    assert!(hung_up(stream));
    assert!(started.elapsed() < Duration::from_secs(15));
}

#[test]
fn a_token_that_differs_anywhere_or_in_length_is_not_the_token() {
    assert!(super::same(b"abc", b"abc"));
    assert!(!super::same(b"abc", b"abd"));
    assert!(!super::same(b"abc", b"xbc"));
    assert!(!super::same(b"abc", b"abcd"));
    assert!(!super::same(b"", b"a"));
}

#[test]
fn a_client_counts_as_attached_while_its_connection_stays_open() {
    let dir = scratch("attached");
    let server = Server::start(&dir).unwrap();
    assert_eq!((server.clients(), server.last_call()), (0, None));
    let stream = broker::connect(server.endpoint()).unwrap();
    let mut writer = stream.try_clone().unwrap();
    writeln!(writer, "ping").unwrap();
    let mut lines = BufReader::new(stream).lines();
    let reader = std::thread::spawn(move || {
        let line = lines.next();
        (line, lines)
    });
    answer_until(&server, || reader.is_finished());
    let (line, lines) = reader.join().unwrap();
    assert_eq!(line.unwrap().unwrap(), "echo ping");
    assert_eq!(server.clients(), 1);
    assert!(server.last_call().is_some());
    drop((writer, lines));
    let deadline = Instant::now() + Duration::from_secs(5);
    while server.clients() != 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        server.clients(),
        0,
        "a closed connection is no longer counted"
    );
}
