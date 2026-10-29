//! The little HTTP/1.1 the host speaks: one request per connection,
//! `Connection: close` on every reply.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

use super::room::Refusal;

const MAX_HEAD: usize = 16 * 1024;

pub(super) struct Request {
    pub(super) method: String,
    pub(super) target: String,
    headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
}

impl Request {
    pub(super) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub(super) fn path(&self) -> &str {
        self.target.split('?').next().unwrap_or("")
    }

    pub(super) fn query(&self, name: &str) -> Option<&str> {
        let query = self.target.split_once('?')?.1;
        query.split('&').find_map(|pair| {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            (k == name).then_some(v)
        })
    }
}

pub(super) enum Incoming {
    Request(Request),
    TooLarge,
    Bad,
}

/// Reads one request; `limit` gives the largest body allowed for a target.
pub(super) fn read_request(
    reader: &mut BufReader<TcpStream>,
    limit: impl Fn(&str) -> usize,
) -> Incoming {
    let mut line = String::new();
    if reader.read_line(&mut line).unwrap_or(0) == 0 {
        return Incoming::Bad;
    }
    let mut parts = line.split_whitespace();
    let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
        return Incoming::Bad;
    };
    let (method, target) = (method.to_owned(), target.to_owned());
    let mut headers = Vec::new();
    let mut head = line.len();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return Incoming::Bad;
        }
        head += line.len();
        if head > MAX_HEAD {
            return Incoming::TooLarge;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((k, v)) = line.split_once(':') {
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    let max_body = limit(&target);
    let mut request = Request {
        method,
        target,
        headers,
        body: Vec::new(),
    };
    if request
        .header("transfer-encoding")
        .is_some_and(|v| v.eq_ignore_ascii_case("chunked"))
    {
        loop {
            let mut size = String::new();
            if reader.read_line(&mut size).unwrap_or(0) == 0 {
                return Incoming::Bad;
            }
            let Ok(n) = usize::from_str_radix(size.trim().split(';').next().unwrap_or(""), 16)
            else {
                return Incoming::Bad;
            };
            if request.body.len() + n > max_body {
                return Incoming::TooLarge;
            }
            let mut chunk = vec![0; n + 2];
            if reader.read_exact(&mut chunk).is_err() {
                return Incoming::Bad;
            }
            if n == 0 {
                break;
            }
            request.body.extend_from_slice(&chunk[..n]);
        }
    } else if let Some(len) = request.header("content-length") {
        let Ok(len) = len.parse::<usize>() else {
            return Incoming::Bad;
        };
        if len > max_body {
            return Incoming::TooLarge;
        }
        request.body = vec![0; len];
        if reader.read_exact(&mut request.body).is_err() {
            return Incoming::Bad;
        }
    }
    Incoming::Request(request)
}

pub(super) fn respond(stream: &mut TcpStream, status: u16, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        426 => "Upgrade Required",
        429 => "Too Many Requests",
        _ => "Error",
    };
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
}

pub(super) fn refuse(stream: &mut TcpStream, refusal: Refusal) {
    respond(
        stream,
        refusal.status,
        &serde_json::json!({ "error": refusal.error }).to_string(),
    );
}

pub(super) fn number_query(request: &Request, name: &str) -> Option<u64> {
    match request.query(name) {
        None => Some(0),
        Some(v) => v
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite() && *n >= 0.0)
            .map(|n| n as u64),
    }
}

pub(super) fn constant_eq(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0u8, |acc, (x, y)| acc | (x ^ y))
            == 0
}
