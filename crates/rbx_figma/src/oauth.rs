//! Figma sign-in: OAuth 2 authorization code with PKCE (RFC 7636), the
//! browser sending its answer back to a one-shot listener on the loopback
//! interface.
//!
//! The client secret is never in the source: it is read when the crate is
//! built, from `RBX_FIGMA_CLIENT_SECRET`. A build without it can still parse
//! links and import with a token from elsewhere, but cannot sign in.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// This app's Figma OAuth client: public by design.
pub const CLIENT_ID: &str = "1hoFla9Afel8DR7jhxqFvb";
/// Baked in at build time; `None` in a build made without it.
pub const CLIENT_SECRET: Option<&str> = option_env!("RBX_FIGMA_CLIENT_SECRET");
pub const PORT: u16 = 47823;
pub const REDIRECT_URI: &str = "http://127.0.0.1:47823/figma/callback";
const SCOPE: &str = "file_content:read";
const TOKEN_URL: &str = "https://api.figma.com/v1/oauth/token";
const REFRESH_URL: &str = "https://api.figma.com/v1/oauth/refresh";

/// What a build without the secret says instead of signing in.
pub const NO_SECRET: &str =
    "This build has no Figma client secret, so it can\u{2019}t sign in to Figma. \
     Rebuild with RBX_FIGMA_CLIENT_SECRET set to the app\u{2019}s secret \
     (RBX_FIGMA_CLIENT_SECRET=\u{2026} cargo build -p rbx_studio).";

/// A signed-in session's tokens. `expires_at` is in Unix seconds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
}

impl Tokens {
    /// Whether the access token runs out within five minutes.
    pub fn stale(&self, now: u64) -> bool {
        self.expires_at <= now + 300
    }
}

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// 32 random bytes, base64url: a PKCE verifier (43 characters) or a `state`.
pub fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS has a random source");
    URL_SAFE_NO_PAD.encode(bytes)
}

/// The S256 challenge for `verifier`.
pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The page the system browser opens to ask the user.
pub fn authorize_url(state: &str, challenge: &str) -> String {
    let query = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", SCOPE)
        .append_pair("state", state)
        .append_pair("response_type", "code")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .finish();
    format!("https://www.figma.com/oauth?{query}")
}

/// What the browser's request says, from its request line
/// (`GET /figma/callback?code=…&state=… HTTP/1.1`). `Ok(None)` for a request
/// that is not the callback (a favicon), so the caller keeps listening.
pub fn parse_callback(request_line: &str, state: &str) -> Result<Option<String>, String> {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let Some(query) = target.strip_prefix("/figma/callback") else {
        return Ok(None);
    };
    let query = query.strip_prefix('?').unwrap_or("");
    let (mut code, mut got_state, mut error) = (None, None, None);
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match &*key {
            "code" => code = Some(value.into_owned()),
            "state" => got_state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    if got_state.as_deref() != Some(state) {
        return Err(
            "Figma\u{2019}s answer doesn\u{2019}t match this sign-in (state mismatch); try again"
                .into(),
        );
    }
    if let Some(error) = error {
        return Err(format!("Figma refused the sign-in: {error}"));
    }
    code.filter(|c| !c.is_empty())
        .map(Some)
        .ok_or_else(|| "Figma\u{2019}s answer carried no code".to_string())
}

/// The loopback listener the browser is sent back to. Bind it before
/// opening the browser, so a taken port is reported up front.
pub struct Callback(TcpListener);

impl Callback {
    pub fn bind() -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", PORT)).map_err(|err| {
            if err.kind() == std::io::ErrorKind::AddrInUse {
                format!("Port {PORT} is in use by another program, so Figma can\u{2019}t send the sign-in back. Close it and try again.")
            } else {
                format!("Couldn\u{2019}t listen on 127.0.0.1:{PORT}: {err}")
            }
        })?;
        listener
            .set_nonblocking(true)
            .map_err(|err| err.to_string())?;
        Ok(Callback(listener))
    }

    /// Waits for the browser's callback, answers it with a page saying the
    /// tab can be closed, and returns the code.
    pub fn wait(self, state: &str, timeout: Duration) -> Result<String, String> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.0.accept() {
                Ok((stream, _)) => {
                    if let Some(answer) = answer(stream, state) {
                        return answer;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if Instant::now() > deadline {
                        return Err("Figma sign-in timed out: the browser never came back".into());
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(err) => return Err(err.to_string()),
            }
        }
    }
}

fn answer(mut stream: TcpStream, state: &str) -> Option<Result<String, String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut line = String::new();
    BufReader::new(&stream).read_line(&mut line).ok()?;
    let parsed = parse_callback(&line, state).transpose();
    let (status, text) = match &parsed {
        None => ("404 Not Found", "Not found."),
        Some(Ok(_)) => (
            "200 OK",
            "Signed in to Figma. You can close this tab and go back to rbx-native.",
        ),
        Some(Err(_)) => (
            "400 Bad Request",
            "Figma sign-in failed. Go back to rbx-native for details.",
        ),
    };
    let page = format!("<!doctype html><meta charset=utf-8><title>rbx-native</title><p style=\"font:16px sans-serif\">{text}</p>");
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    parsed
}

/// A form POST to Figma's token endpoints.
#[derive(Debug, PartialEq, Eq)]
pub struct Form {
    pub url: &'static str,
    pub authorization: String,
    pub body: String,
}

fn basic(secret: &str) -> String {
    format!("Basic {}", STANDARD.encode(format!("{CLIENT_ID}:{secret}")))
}

pub fn token_request(secret: &str, code: &str, verifier: &str) -> Form {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("code", code)
        .append_pair("grant_type", "authorization_code")
        .append_pair("code_verifier", verifier)
        .finish();
    Form {
        url: TOKEN_URL,
        authorization: basic(secret),
        body,
    }
}

pub fn refresh_request(secret: &str, refresh_token: &str) -> Form {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("refresh_token", refresh_token)
        .finish();
    Form {
        url: REFRESH_URL,
        authorization: basic(secret),
        body,
    }
}

#[derive(Deserialize)]
struct Answer {
    access_token: String,
    expires_in: u64,
    refresh_token: Option<String>,
}

/// A token or refresh answer as [`Tokens`]; a refresh answer has no new
/// refresh token, so `previous` keeps going.
pub fn parse_tokens(body: &[u8], now: u64, previous: Option<&str>) -> Result<Tokens, String> {
    let answer: Answer = serde_json::from_slice(body)
        .map_err(|err| format!("Figma\u{2019}s token answer is not what was expected: {err}"))?;
    let refresh_token = answer
        .refresh_token
        .or_else(|| previous.map(str::to_string))
        .ok_or("Figma\u{2019}s token answer carried no refresh token")?;
    Ok(Tokens {
        access_token: answer.access_token,
        refresh_token,
        expires_at: now + answer.expires_in,
    })
}

fn send(form: &Form) -> Result<Vec<u8>, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut response = agent
        .post(form.url)
        .header("Authorization", &form.authorization)
        .content_type("application/x-www-form-urlencoded")
        .send(form.body.as_bytes())
        .map_err(|err| format!("Couldn\u{2019}t reach Figma: {err}"))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|err| err.to_string())?;
    if !(200..300).contains(&status) {
        return Err(format!(
            "Figma refused the sign-in ({status}): {}",
            String::from_utf8_lossy(&body)
                .chars()
                .take(200)
                .collect::<String>()
        ));
    }
    Ok(body)
}

/// Blocking: trades the callback's code for tokens. The code is only good
/// for 30 seconds, so call this as soon as [`Callback::wait`] returns.
pub fn exchange(code: &str, verifier: &str) -> Result<Tokens, String> {
    let secret = CLIENT_SECRET.ok_or(NO_SECRET)?;
    parse_tokens(&send(&token_request(secret, code, verifier))?, now(), None)
}

/// Blocking: a new access token. Figma invalidates the old one.
pub fn refresh(tokens: &Tokens) -> Result<Tokens, String> {
    let secret = CLIENT_SECRET.ok_or(NO_SECRET)?;
    let body = send(&refresh_request(secret, &tokens.refresh_token))?;
    parse_tokens(&body, now(), Some(&tokens.refresh_token))
}

/// A sign-in between opening the browser and its answer: the listener is
/// already bound, so the browser can't come back to a closed port.
pub struct Pending {
    callback: Callback,
    verifier: String,
    state: String,
    /// The page to open in the system browser.
    pub url: String,
}

/// Binds the listener and builds the authorize link. Quick: fine on the UI
/// thread, where the browser is opened from.
pub fn begin() -> Result<Pending, String> {
    CLIENT_SECRET.ok_or(NO_SECRET)?;
    let callback = Callback::bind()?;
    let (verifier, state) = (random_token(), random_token());
    let url = authorize_url(&state, &challenge(&verifier));
    Ok(Pending {
        callback,
        verifier,
        state,
        url,
    })
}

impl Pending {
    /// Blocking: waits for the browser and trades its code for tokens.
    pub fn finish(self, timeout: Duration) -> Result<Tokens, String> {
        let code = self.callback.wait(&self.state, timeout)?;
        exchange(&code, &self.verifier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_matches_rfc_7636_appendix_b() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let verifier = random_token();
        assert_eq!(verifier.len(), 43);
        assert_ne!(verifier, random_token());
    }

    #[test]
    fn the_authorize_url_carries_every_parameter() {
        let url = authorize_url("st", "ch");
        assert_eq!(
            url,
            "https://www.figma.com/oauth?client_id=1hoFla9Afel8DR7jhxqFvb\
             &redirect_uri=http%3A%2F%2F127.0.0.1%3A47823%2Ffigma%2Fcallback\
             &scope=file_content%3Aread&state=st&response_type=code\
             &code_challenge=ch&code_challenge_method=S256"
        );
    }

    #[test]
    fn the_callback_gives_its_code_only_for_this_state() {
        let line = "GET /figma/callback?code=abc%2B1&state=s1 HTTP/1.1\r\n";
        assert_eq!(parse_callback(line, "s1"), Ok(Some("abc+1".into())));
        assert!(parse_callback(line, "s2")
            .unwrap_err()
            .contains("state mismatch"));
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1", "s1"), Ok(None));
        let denied = "GET /figma/callback?error=access_denied&state=s1 HTTP/1.1";
        assert!(parse_callback(denied, "s1")
            .unwrap_err()
            .contains("access_denied"));
    }

    #[test]
    fn token_and_refresh_requests_are_basic_authed_forms() {
        let form = token_request("sec", "c0de", "ver");
        assert_eq!(form.url, "https://api.figma.com/v1/oauth/token");
        // base64("1hoFla9Afel8DR7jhxqFvb:sec")
        assert_eq!(
            form.authorization,
            format!("Basic {}", STANDARD.encode("1hoFla9Afel8DR7jhxqFvb:sec"))
        );
        assert_eq!(
            form.body,
            "redirect_uri=http%3A%2F%2F127.0.0.1%3A47823%2Ffigma%2Fcallback\
             &code=c0de&grant_type=authorization_code&code_verifier=ver"
        );
        let form = refresh_request("sec", "r/t");
        assert_eq!(form.url, "https://api.figma.com/v1/oauth/refresh");
        assert_eq!(form.body, "refresh_token=r%2Ft");
    }

    #[test]
    fn token_answers_become_tokens() {
        let token = br#"{"user_id":1,"user_id_string":"1","access_token":"a","token_type":"bearer","expires_in":7776000,"refresh_token":"r"}"#;
        let tokens = parse_tokens(token, 100, None).unwrap();
        assert_eq!(
            tokens,
            Tokens {
                access_token: "a".into(),
                refresh_token: "r".into(),
                expires_at: 7776100
            }
        );
        let refreshed = br#"{"access_token":"b","token_type":"bearer","expires_in":10}"#;
        let tokens = parse_tokens(refreshed, 5, Some("r")).unwrap();
        assert_eq!(
            (
                tokens.access_token.as_str(),
                tokens.refresh_token.as_str(),
                tokens.expires_at
            ),
            ("b", "r", 15)
        );
        assert!(tokens.stale(0));
        assert!(parse_tokens(refreshed, 5, None).is_err());
    }
}
