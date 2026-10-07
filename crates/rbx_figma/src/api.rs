//! Figma's REST API through a signed-in session: refreshes the token near
//! expiry or when Figma calls it expired, and backs off on 429.

use std::time::Duration;

use serde_json::Value;

use crate::oauth::{self, Tokens};

const API: &str = "https://api.figma.com";
const ATTEMPTS: u32 = 3;
const LONGEST_WAIT: Duration = Duration::from_secs(30);

pub struct Session {
    tokens: Tokens,
    agent: ureq::Agent,
    /// Whether `tokens` changed since the session began: they must be saved,
    /// the old access token no longer works.
    pub refreshed: bool,
}

impl Session {
    pub fn new(tokens: Tokens) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(60)))
            .build()
            .into();
        Session {
            tokens,
            agent,
            refreshed: false,
        }
    }

    pub fn tokens(&self) -> &Tokens {
        &self.tokens
    }

    fn refresh(&mut self) -> Result<(), String> {
        self.tokens = oauth::refresh(&self.tokens).map_err(|err| {
            format!("Figma sign-in expired and couldn\u{2019}t be renewed; connect again. ({err})")
        })?;
        self.refreshed = true;
        Ok(())
    }

    /// `GET` an API path (`/v1/files/…`) as JSON.
    pub fn get_json(&mut self, path: &str) -> Result<Value, String> {
        if self.tokens.stale(oauth::now()) {
            self.refresh()?;
        }
        let mut renewed = false;
        let mut attempt = 0;
        loop {
            attempt += 1;
            let mut response = self
                .agent
                .get(&format!("{API}{path}"))
                .header(
                    "Authorization",
                    &format!("Bearer {}", self.tokens.access_token),
                )
                .call()
                .map_err(|err| format!("Couldn\u{2019}t reach Figma: {err}"))?;
            let status = response.status().as_u16();
            let wait = retry_after(
                response
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok()),
                attempt,
            );
            let body = response
                .body_mut()
                .with_config()
                .limit(256 * 1024 * 1024)
                .read_to_vec()
                .map_err(|err| err.to_string())?;
            match status {
                200..=299 => return serde_json::from_slice(&body).map_err(|err| err.to_string()),
                401 | 403 if !renewed && expired(&body) => {
                    self.refresh()?;
                    renewed = true;
                }
                429 if attempt < ATTEMPTS => std::thread::sleep(wait),
                403 => {
                    return Err(format!(
                        "Figma refused access to this file (403): {}",
                        said(&body)
                    ))
                }
                404 => return Err("Figma has no such file or frame (404); check the link".into()),
                429 => {
                    return Err(
                        "Figma is rate-limiting this account (429); wait a minute and import again"
                            .into(),
                    )
                }
                _ => return Err(format!("Figma answered {status}: {}", said(&body))),
            }
        }
    }

    /// Downloads a rendered or uploaded image from the URL the API handed
    /// out (signed, so no token).
    pub fn download(&self, url: &str) -> Result<Vec<u8>, String> {
        let mut response = self
            .agent
            .get(url)
            .call()
            .map_err(|err| format!("Couldn\u{2019}t download a Figma image: {err}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "Figma image download answered {}",
                response.status()
            ));
        }
        response
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .map_err(|err| err.to_string())
    }
}

/// Whether a 401/403 is about the token rather than the file.
fn expired(body: &[u8]) -> bool {
    let text = said(body).to_ascii_lowercase();
    text.contains("expired") || text.contains("invalid token") || text.contains("token")
}

fn said(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|v| {
            v.get("err")
                .or_else(|| v.get("message"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(200).collect())
}

/// How long to wait before attempt `attempt + 1`: Figma's `Retry-After` when
/// it gives one, capped, else a doubling second.
fn retry_after(header: Option<&str>, attempt: u32) -> Duration {
    header
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(1 << attempt))
        .min(LONGEST_WAIT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_limits_wait_as_told_within_a_cap() {
        assert_eq!(retry_after(Some("3"), 1), Duration::from_secs(3));
        assert_eq!(retry_after(Some("3600"), 1), LONGEST_WAIT);
        assert_eq!(retry_after(None, 2), Duration::from_secs(4));
        assert!(expired(br#"{"status":403,"err":"Token expired"}"#));
        assert!(!expired(br#"{"status":403,"err":"File not shared"}"#));
    }

    /// Reads a real frame through a token, without touching the keyring:
    /// `RBX_FIGMA_TOKEN=<access token> RBX_FIGMA_TEST_URL=<frame link>
    /// cargo test -p rbx_figma live_frame -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn live_frame() {
        let token = std::env::var("RBX_FIGMA_TOKEN").expect("RBX_FIGMA_TOKEN");
        let link =
            crate::link::parse(&std::env::var("RBX_FIGMA_TEST_URL").expect("RBX_FIGMA_TEST_URL"))
                .unwrap();
        let mut session = Session::new(Tokens {
            access_token: token,
            refresh_token: String::new(),
            expires_at: u64::MAX / 2,
        });
        let root = crate::import::fetch_root(&mut session, &link).unwrap();
        let node = crate::infer::infer(&root).unwrap();
        eprintln!("{}", node.outline());
    }
}
