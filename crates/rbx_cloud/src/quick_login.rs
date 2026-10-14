//! Signing in to Roblox with a code, no browser embedded: Roblox's "Quick
//! Log In" (cross-device login), the flow Studio's own binary uses.
//!
//! 1. `create` returns a 6-character code (lives ~2 minutes) and a QR image.
//! 2. The user enters the code at [`CONFIRM_PAGE`] (or scans the QR in the
//!    Roblox app) and approves: `status` goes Created → UserLinked → Validated.
//! 3. `finish` exchanges the validated code for a `.ROBLOSECURITY` session.
//!
//! Verified live 2026-10-14: Created → UserLinked → Validated, then
//! `POST /v2/login {"ctype":"AuthToken","cvalue":code,"password":privateKey}`
//! answered 200 with the cookie and `{"user":{"id":…}}`; no captcha. An
//! expired code's status answers 400 `"CodeInvalid"`.
//!
//! Every POST uses the web CSRF dance (403 + `x-csrf-token` → retry once).

use crate::heartbeats::SessionCookie;

pub const CREATE_URL: &str = "https://apis.roblox.com/auth-token-service/v1/login/create";
pub const STATUS_URL: &str = "https://apis.roblox.com/auth-token-service/v1/login/status";
pub const LOGIN_URL: &str = "https://auth.roblox.com/v2/login";
/// Where the user types the code.
pub const CONFIRM_PAGE: &str = "https://www.roblox.com/crossdevicelogin/ConfirmCode";
/// `LoginCode::qr_path` is relative to this.
pub const QR_BASE: &str = "https://apis.roblox.com/auth-token-service";

/// One code. `private_key` never leaves this module except to Roblox.
#[derive(Clone, PartialEq, Eq)]
pub struct LoginCode {
    pub code: String,
    private_key: String,
    /// `expirationTime` as Roblox sent it (UTC, no zone suffix).
    pub expires: String,
    /// `imagePath`, e.g. `/v1/login/qr-code-image?key=…&code=…`.
    pub qr_path: String,
}

impl std::fmt::Debug for LoginCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoginCode")
            .field("code", &self.code)
            .field("expires", &self.expires)
            .finish_non_exhaustive()
    }
}

impl LoginCode {
    pub(crate) fn private_key(&self) -> &str {
        &self.private_key
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginStatus {
    Created,
    /// Someone entered the code; waiting for their approval.
    UserLinked,
    Validated,
    Cancelled,
    /// The code expired (400 `CodeInvalid`); make a new one.
    Expired,
}

/// Parses `create`'s 200 body: `{"code","status","privateKey","expirationTime","imagePath"}`.
pub fn parse_create(body: &str) -> Result<LoginCode, String> {
    let value: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let field = |name: &str| -> Result<String, String> {
        value
            .get(name)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| format!("create reply is missing \"{name}\""))
    };
    Ok(LoginCode {
        code: field("code")?,
        private_key: field("privateKey")?,
        expires: field("expirationTime")?,
        qr_path: field("imagePath")?,
    })
}

/// Maps a status reply: 200 + `{"status":"Created"|"UserLinked"|"Validated"|"Cancelled"}`;
/// 400 (any body) → `Expired`; anything else → `Err` with the status.
pub fn parse_status(status: u16, body: &str) -> Result<LoginStatus, String> {
    match status {
        200 => {
            let value: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
            match value.get("status").and_then(|v| v.as_str()) {
                Some("Created") => Ok(LoginStatus::Created),
                Some("UserLinked") => Ok(LoginStatus::UserLinked),
                Some("Validated") => Ok(LoginStatus::Validated),
                Some("Cancelled") => Ok(LoginStatus::Cancelled),
                Some(other) => Err(format!("unknown login status \"{other}\"")),
                None => Err("status reply has no \"status\"".to_owned()),
            }
        }
        400 => Ok(LoginStatus::Expired),
        other => Err(format!("login status check failed (HTTP {other})")),
    }
}

/// The `.ROBLOSECURITY` value among `Set-Cookie` header values, if any.
pub fn cookie_from_set_cookie(headers: &[String]) -> Option<SessionCookie> {
    const NAME: &str = ".ROBLOSECURITY=";
    headers.iter().find_map(|header| {
        let pair = header.split(';').next()?.trim();
        let value = pair.strip_prefix(NAME)?.trim();
        (!value.is_empty()).then(|| SessionCookie::new(value))
    })
}

struct Reply {
    status: u16,
    set_cookies: Vec<String>,
    body: String,
}

/// A blocking client; one per sign-in attempt. Uses ureq with
/// `http_status_as_error(false)`, 15 s timeout, user agent `rbx-native/<ver>`.
pub struct QuickLogin {
    agent: ureq::Agent,
    csrf: String,
}

impl QuickLogin {
    pub fn new() -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(concat!("rbx-native/", env!("CARGO_PKG_VERSION")))
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(15)))
            .build()
            .into();
        QuickLogin {
            agent,
            csrf: String::new(),
        }
    }

    fn post_json(&mut self, url: &str, body: &serde_json::Value) -> Result<Reply, String> {
        let mut retried = false;
        loop {
            let mut req = self.agent.post(url);
            if !self.csrf.is_empty() {
                req = req.header("x-csrf-token", &self.csrf);
            }
            let mut resp = req.send_json(body).map_err(|e| e.to_string())?;
            let status = resp.status().as_u16();
            let fresh = resp
                .headers()
                .get("x-csrf-token")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            if status == 403 && !retried {
                if let Some(token) = fresh.filter(|t| *t != self.csrf) {
                    self.csrf = token;
                    retried = true;
                    continue;
                }
            }
            let set_cookies = resp
                .headers()
                .get_all("set-cookie")
                .iter()
                .filter_map(|v| v.to_str().ok().map(str::to_owned))
                .collect();
            let body = resp.body_mut().read_to_string().unwrap_or_default();
            return Ok(Reply {
                status,
                set_cookies,
                body,
            });
        }
    }

    /// `POST CREATE_URL {}` → a fresh code.
    pub fn create(&mut self) -> Result<LoginCode, String> {
        let reply = self.post_json(CREATE_URL, &serde_json::json!({}))?;
        if reply.status != 200 {
            return Err(format!(
                "could not create a login code (HTTP {})",
                reply.status
            ));
        }
        parse_create(&reply.body)
    }

    /// `POST STATUS_URL {"code","privateKey"}`.
    pub fn status(&mut self, code: &LoginCode) -> Result<LoginStatus, String> {
        let body = serde_json::json!({"code": code.code, "privateKey": code.private_key()});
        let reply = self.post_json(STATUS_URL, &body)?;
        parse_status(reply.status, &reply.body)
    }

    /// `GET QR_BASE + qr_path` → PNG bytes.
    pub fn qr_png(&mut self, code: &LoginCode) -> Result<Vec<u8>, String> {
        let url = format!("{QR_BASE}{}", code.qr_path);
        let mut resp = self.agent.get(&url).call().map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(format!("could not fetch the QR image (HTTP {status})"));
        }
        resp.body_mut()
            .with_config()
            .limit(4 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| e.to_string())
    }

    /// `POST LOGIN_URL {"ctype":"AuthToken","cvalue":code,"password":privateKey}`
    /// on a Validated code → the session cookie (from `Set-Cookie`) and the
    /// user id (`user.id` in the body). Errors never contain the cookie.
    pub fn finish(&mut self, code: &LoginCode) -> Result<(SessionCookie, u64), String> {
        let body = serde_json::json!({
            "ctype": "AuthToken",
            "cvalue": code.code,
            "password": code.private_key(),
        });
        let reply = self.post_json(LOGIN_URL, &body)?;
        if reply.status != 200 {
            let snippet: String = reply.body.chars().take(200).collect();
            return Err(format!(
                "Roblox refused the sign-in (HTTP {}) {snippet}",
                reply.status
            ));
        }
        let cookie = cookie_from_set_cookie(&reply.set_cookies)
            .ok_or_else(|| "Roblox did not send a session cookie".to_owned())?;
        let value: serde_json::Value =
            serde_json::from_str(&reply.body).map_err(|e| e.to_string())?;
        let user_id = value
            .pointer("/user/id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| "sign-in reply has no user id".to_owned())?;
        Ok((cookie, user_id))
    }
}

impl Default for QuickLogin {
    fn default() -> Self {
        Self::new()
    }
}
