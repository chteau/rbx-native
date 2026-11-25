//! Waiting for a fresh cloudflared tunnel to answer, for the recorders.

use std::time::{Duration, Instant};

use rbx_play::protocol::{Token, TOKEN_HEADER};

pub fn wait_for_tunnel(url: &str, token: &Token, say: &dyn Fn(&str)) {
    // trycloudflare names take a few seconds to exist (see live_probe).
    std::thread::sleep(Duration::from_secs(10));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    let health = format!("{url}/healthz");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let status = agent
            .get(&health)
            .header(TOKEN_HEADER, token.as_str())
            .call()
            .map(|r| r.status().as_u16());
        if matches!(status, Ok(200)) {
            say("tunnel answers");
            return;
        }
        if Instant::now() > deadline {
            say(&format!("tunnel never answered (last: {status:?})"));
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}
