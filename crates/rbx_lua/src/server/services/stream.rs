//! `HttpService:CreateWebStreamClient`'s connections, each on a thread of
//! its own: Server-Sent Events (one message per event's `data`), a raw
//! stream (one message per chunk read) or a WebSocket. What happens comes
//! back as [`Event`]s, which the services hand the script once a frame.

use std::io::Read;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::plugin::http::HttpRequest;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Event {
    Opened(u16, String),
    Message(String),
    Error(u16, String),
    Closed,
}

pub(crate) enum Out {
    Send(String),
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Sse,
    Raw,
    WebSocket,
}

/// Events of every client, by the client's instance id.
pub(crate) type Inbox = Arc<Mutex<Vec<(u32, Event)>>>;

/// Opens `request` as a stream of `kind`; the sender takes what the script
/// sends (a WebSocket's) and the close.
pub(crate) fn open(id: u32, kind: Kind, request: HttpRequest, inbox: Inbox) -> Sender<Out> {
    let (out, commands) = mpsc::channel();
    std::thread::spawn(move || {
        let push = |event| {
            inbox
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push((id, event))
        };
        let result = match kind {
            Kind::WebSocket => websocket(&request, &commands, &push),
            _ => http_stream(kind, &request, &commands, &push),
        };
        if let Err(message) = result {
            push(Event::Error(0, message));
        }
        push(Event::Closed);
    });
    out
}

fn closed(commands: &Receiver<Out>) -> bool {
    matches!(
        commands.try_recv(),
        Ok(Out::Close) | Err(TryRecvError::Disconnected)
    )
}

fn http_stream(
    kind: Kind,
    request: &HttpRequest,
    commands: &Receiver<Out>,
    push: &dyn Fn(Event),
) -> Result<(), String> {
    let agent: ureq::Agent = rbx_net::config().http_status_as_error(false).build().into();
    let mut builder = ureq::http::Request::builder()
        .method(request.method.as_str())
        .uri(request.url.as_str());
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    if kind == Kind::Sse {
        builder = builder.header("Accept", "text/event-stream");
    }
    let prepared = builder
        .body(request.body.clone().unwrap_or_default())
        .map_err(|e| e.to_string())?;
    let mut response = agent.run(prepared).map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let headers: serde_json::Map<String, serde_json::Value> = response
        .headers()
        .iter()
        .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.into())))
        .collect();
    if !response.status().is_success() {
        let body = response.body_mut().read_to_string().unwrap_or_default();
        push(Event::Error(status, body));
        return Ok(());
    }
    push(Event::Opened(
        status,
        serde_json::Value::Object(headers).to_string(),
    ));
    let mut reader = response.into_body().into_reader();
    let mut buf = [0u8; 8192];
    let mut pending = String::new();
    let mut data: Vec<String> = Vec::new();
    loop {
        if closed(commands) {
            return Ok(());
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        let chunk = String::from_utf8_lossy(&buf[..n]).into_owned();
        if kind == Kind::Raw {
            push(Event::Message(chunk));
            continue;
        }
        pending.push_str(&chunk);
        while let Some(end) = pending.find('\n') {
            let line: String = pending.drain(..=end).collect();
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !data.is_empty() {
                    push(Event::Message(data.join("\n")));
                    data.clear();
                }
            } else if let Some(rest) = line.strip_prefix("data:") {
                data.push(rest.strip_prefix(' ').unwrap_or(rest).to_owned());
            }
        }
    }
}

fn websocket(
    request: &HttpRequest,
    commands: &Receiver<Out>,
    push: &dyn Fn(Event),
) -> Result<(), String> {
    use tungstenite::client::IntoClientRequest;
    use tungstenite::stream::MaybeTlsStream;
    use tungstenite::Message;
    if rbx_net::is_offline() {
        return Err("HttpError: ConnectFail".into());
    }
    let url = request
        .url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let mut handshake = url.into_client_request().map_err(|e| e.to_string())?;
    for (name, value) in &request.headers {
        if let (Ok(name), Ok(value)) = (
            tungstenite::http::HeaderName::from_bytes(name.as_bytes()),
            tungstenite::http::HeaderValue::from_str(value),
        ) {
            handshake.headers_mut().insert(name, value);
        }
    }
    let (mut socket, response) = tungstenite::connect(handshake).map_err(|e| e.to_string())?;
    let tcp = match socket.get_ref() {
        MaybeTlsStream::Plain(s) => Some(s),
        MaybeTlsStream::Rustls(s) => Some(s.get_ref()),
        _ => None,
    };
    if let Some(tcp) = tcp {
        let _ = tcp.set_read_timeout(Some(Duration::from_millis(50)));
    }
    let headers: serde_json::Map<String, serde_json::Value> = response
        .headers()
        .iter()
        .filter_map(|(k, v)| Some((k.to_string(), v.to_str().ok()?.into())))
        .collect();
    push(Event::Opened(
        response.status().as_u16(),
        serde_json::Value::Object(headers).to_string(),
    ));
    loop {
        match commands.try_recv() {
            Ok(Out::Send(text)) => socket
                .send(Message::text(text))
                .map_err(|e| e.to_string())?,
            Ok(Out::Close) | Err(TryRecvError::Disconnected) => {
                let _ = socket.close(None);
                return Ok(());
            }
            Err(TryRecvError::Empty) => {}
        }
        match socket.read() {
            Ok(Message::Text(text)) => push(Event::Message(text.to_string())),
            Ok(Message::Binary(bytes)) => {
                push(Event::Message(String::from_utf8_lossy(&bytes).into_owned()))
            }
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(tungstenite::Error::ConnectionClosed) => return Ok(()),
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn server_sent_events_arrive_one_message_per_event() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            let body = "data: one\n\ndata: two\ndata: lines\n\n";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
        });
        let inbox: Inbox = Arc::default();
        let request = HttpRequest {
            url: format!("http://127.0.0.1:{port}/events"),
            method: "GET".into(),
            headers: Vec::new(),
            body: None,
        };
        let _out = open(7, Kind::Sse, request, inbox.clone());
        let started = std::time::Instant::now();
        while !inbox
            .lock()
            .unwrap()
            .iter()
            .any(|(_, e)| *e == Event::Closed)
        {
            assert!(started.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(10));
        }
        let events: Vec<Event> = inbox
            .lock()
            .unwrap()
            .iter()
            .map(|(_, e)| e.clone())
            .collect();
        assert!(matches!(events[0], Event::Opened(200, _)));
        assert_eq!(
            events[1..],
            [
                Event::Message("one".into()),
                Event::Message("two\nlines".into()),
                Event::Closed
            ]
        );
    }
}
