//! Calls to develop.roblox.com that Open Cloud has no equivalent for. They
//! run as the user's web sign-in (the `.ROBLOSECURITY` cookie), not an API
//! key, with the usual CSRF dance: a request refused with `403` and a fresh
//! `x-csrf-token` is sent once more with that token.

use crate::error::CloudError;
use crate::heartbeats::SessionCookie;

pub(crate) const DEVELOP: &str = "https://develop.roblox.com";

pub(crate) enum Method<'a> {
    Get,
    Patch(&'a serde_json::Value),
    Post(&'a serde_json::Value),
    /// A POST with no body.
    Touch,
    Delete,
    /// A multipart POST of one image, as the website's upload forms send it.
    Image(&'a Image<'a>),
}

/// One file part of a multipart upload.
pub(crate) struct Image<'a> {
    /// The form field Roblox reads the file from.
    pub(crate) field: &'a str,
    pub(crate) bytes: &'a [u8],
}

impl Image<'_> {
    /// PNG or JPEG by the bytes' signature; anything else goes as PNG and
    /// Roblox's answer says what is wrong with it.
    fn content_type(&self) -> &'static str {
        if self.bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            "image/jpeg"
        } else {
            "image/png"
        }
    }

    pub(crate) fn multipart(&self, boundary: &str) -> Vec<u8> {
        let (kind, ext) = match self.content_type() {
            "image/jpeg" => ("image/jpeg", "jpg"),
            _ => ("image/png", "png"),
        };
        let mut body = Vec::with_capacity(self.bytes.len() + 256);
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"image.{ext}\"\r\n\
                 Content-Type: {kind}\r\n\r\n",
                self.field
            )
            .as_bytes(),
        );
        body.extend_from_slice(self.bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        body
    }
}

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("rbx-native/", env!("CARGO_PKG_VERSION")))
        .http_status_as_error(false)
        .timeout_global(Some(std::time::Duration::from_secs(15)))
        .build()
        .into()
}

/// Sends `method` to `url` (plus `query`, encoded) signed with `cookie` and
/// hands back a 2xx reply's body. Anything else is `Refused`, with Roblox's
/// own message when the body has one.
pub(crate) fn send(
    cookie: &SessionCookie,
    method: Method,
    url: &str,
    query: &[(&str, &str)],
) -> Result<Vec<u8>, CloudError> {
    let agent = agent();
    let cookie = format!(".ROBLOSECURITY={}", cookie.value());
    let mut csrf = String::new();
    loop {
        let mut response = match method {
            Method::Get => signed(agent.get(url), &cookie, &csrf, query).call(),
            Method::Patch(body) => signed(agent.patch(url), &cookie, &csrf, query).send_json(body),
            Method::Post(body) => signed(agent.post(url), &cookie, &csrf, query).send_json(body),
            Method::Touch => signed(agent.post(url), &cookie, &csrf, query).send_empty(),
            Method::Delete => signed(agent.delete(url), &cookie, &csrf, query).call(),
            Method::Image(image) => {
                let boundary = crate::create_asset::boundary(image.bytes);
                signed(agent.post(url), &cookie, &csrf, query)
                    .header(
                        "Content-Type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .send(&image.multipart(&boundary)[..])
            }
        }
        .map_err(|err| CloudError::Transport(err.to_string()))?;
        let status = response.status().as_u16();
        let fresh = response
            .headers()
            .get("x-csrf-token")
            .and_then(|value| value.to_str().ok())
            .filter(|token| *token != csrf)
            .map(str::to_owned);
        let body = response.body_mut().read_to_vec().unwrap_or_default();
        if (200..300).contains(&status) {
            return Ok(body);
        }
        if let Some(token) = fresh.filter(|_| status == 403 && csrf.is_empty()) {
            csrf = token;
            continue;
        }
        let message = serde_json::from_slice(&body)
            .ok()
            .as_ref()
            .and_then(crate::assets::complaint)
            .unwrap_or_else(|| format!("HTTP {status} from {url}"));
        return Err(CloudError::Refused { status, message });
    }
}

fn signed<B>(
    request: ureq::RequestBuilder<B>,
    cookie: &str,
    csrf: &str,
    query: &[(&str, &str)],
) -> ureq::RequestBuilder<B> {
    let mut request = request
        .header("Cookie", cookie)
        .query_pairs(query.iter().copied());
    if !csrf.is_empty() {
        request = request.header("x-csrf-token", csrf);
    }
    request
}

/// A loopback server for the web calls' tests.
#[cfg(test)]
pub(crate) mod serve {
    use std::io::{Read, Write};

    /// Answers one request per reply (`status`, extra headers, body), each
    /// on its own connection, handing back every request it was sent.
    pub(crate) fn serve(
        replies: Vec<(u16, &'static str, &'static str)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let root = format!("http://{}", listener.local_addr().unwrap());
        let served = std::thread::spawn(move || {
            let mut requests = Vec::new();
            for (status, headers, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut seen = Vec::new();
                let mut buf = [0u8; 4096];
                while !request_done(&seen) {
                    let read = stream.read(&mut buf).unwrap();
                    seen.extend_from_slice(&buf[..read]);
                }
                let reply = format!(
                    "HTTP/1.1 {status} X\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(reply.as_bytes()).unwrap();
                requests.push(String::from_utf8_lossy(&seen).into_owned());
            }
            requests
        });
        (root, served)
    }

    /// Whether `seen` holds the whole head and as much body as it announced.
    fn request_done(seen: &[u8]) -> bool {
        let text = String::from_utf8_lossy(seen);
        let Some((head, body)) = text.split_once("\r\n\r\n") else {
            return false;
        };
        let length = head
            .lines()
            .find_map(|line| {
                line.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|n| n.trim().parse().unwrap_or(0))
            })
            .unwrap_or(0);
        body.len() >= length
    }
}
