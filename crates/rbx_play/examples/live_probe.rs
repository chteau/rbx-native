//! Live tests 1–3 of `docs/research/play-sandbox/README.md` in one run:
//! channel + cloudflared tunnel, probe injected into a test place, publish
//! (`Published`) over a sandbox place, join it with the Roblox client, then
//! print everything the probe sends until the player leaves.
//!
//! ```text
//! RBX_API_KEY=… cargo run -p rbx_play --example live_probe -- \
//!     <universe_id> <place_id> <base.rbxl> <cloudflared> [log file]
//! ```

use std::io::Write;
use std::time::{Duration, Instant};

use rbx_play::channel::{ChannelServer, DEFAULT_HOLD};
use rbx_play::inject::{self, ProbeConfig};
use rbx_play::launch::{join_uri, ClientProcess, Launcher};
use rbx_play::protocol::{CommandBody, Token, Uplink, TOKEN_HEADER};
use rbx_play::tunnel::TunnelProvider;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [universe, place, base, cloudflared, ..] = args.as_slice() else {
        eprintln!("usage: live_probe <universe_id> <place_id> <base.rbxl> <cloudflared> [log]");
        std::process::exit(2);
    };
    let universe: u64 = universe.parse().expect("universe id");
    let place: u64 = place.parse().expect("place id");
    let mut log = args
        .get(4)
        .map(|p| std::fs::File::create(p).expect("log file"));
    let started = Instant::now();
    let mut say = |line: String| {
        let line = format!("[{:6.1}s] {line}", started.elapsed().as_secs_f64());
        println!("{line}");
        if let Some(f) = log.as_mut() {
            let _ = writeln!(f, "{line}");
        }
    };

    let token = Token::random();
    let session = format!("live-{}", std::process::id());
    let channel = ChannelServer::start(token.clone(), DEFAULT_HOLD).expect("channel");
    say(format!("channel on 127.0.0.1:{}", channel.port()));

    let provider = TunnelProvider::Cloudflared {
        binary: cloudflared.into(),
    };
    let tunnel = provider
        .start(channel.port(), Duration::from_secs(30))
        .expect("tunnel");
    say(format!("tunnel {}", tunnel.public_url));

    // trycloudflare names take a few seconds to exist; asking earlier gets a
    // "not found" the resolver then caches for a minute.
    std::thread::sleep(Duration::from_secs(10));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(10)))
        .build()
        .into();
    let health = format!("{}/healthz", tunnel.public_url);
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let status = agent
            .get(&health)
            .header(TOKEN_HEADER, token.as_str())
            .call()
            .map(|r| r.status().as_u16());
        if matches!(status, Ok(200)) {
            say("healthz through the tunnel: 200".into());
            break;
        }
        if Instant::now() > deadline {
            say(format!("healthz never answered 200 (last: {status:?})"));
            std::process::exit(1);
        }
        std::thread::sleep(Duration::from_secs(2));
    }

    let mut dom =
        rbx_binary::deserialize(&std::fs::read(base).expect("base place")).expect("parse");
    inject::inject(
        &mut dom,
        &ProbeConfig {
            base_url: tunnel.public_url.clone(),
            token: token.as_str().to_owned(),
            session: session.clone(),
        },
    )
    .expect("inject");
    let database = rbx_reflection::ReflectionDatabase::shared();
    let bytes = rbx_binary::serialize_with_defaults(&dom, |class, key| {
        database.stored_default(class, key).cloned()
    })
    .expect("serialize");
    for warning in inject::preflight(&dom, bytes.len()) {
        say(format!("preflight: {warning:?}"));
    }

    let key = rbx_cloud::ApiKey::from_env_or_config()
        .or_else(keyring_key)
        .expect("no API key: set RBX_API_KEY or run rbxstudio's key wizard");
    let client = rbx_cloud::Client::new(Some(key));
    let version = client
        .publish_place(universe, place, &bytes, rbx_cloud::PublishMode::Published)
        .expect("publish");
    say(format!(
        "published version {version} ({} bytes)",
        bytes.len()
    ));

    let launcher = Launcher::detect();
    let uri = join_uri(place, &session);
    say(format!("launching {launcher:?}: {uri}"));
    let mut game = ClientProcess::spawn(launcher, &uri).expect("launch");

    let mut evaluated = false;
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        match channel.events().recv_timeout(Duration::from_millis(500)) {
            Ok(event) => {
                say(format!("<- {event:?}"));
                match event {
                    Uplink::Hello { .. } if !evaluated => {
                        evaluated = true;
                        let id = channel.send(CommandBody::Eval {
                            source: "print('eval from the editor') return 1 + 1".into(),
                        });
                        say(format!("-> eval #{id}"));
                    }
                    Uplink::Left { .. } | Uplink::Bye => {
                        say("session over: player left / server closing".into());
                        break;
                    }
                    _ => {}
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
        if game.exited() {
            say("session over: client process exited".into());
            break;
        }
        if Instant::now() > deadline {
            say("session over: 10 minute limit".into());
            break;
        }
    }
    drop(game);
    drop(tunnel);
}

/// The key `rbxstudio` stored in the OS keyring (same lookup as `rbxcloud`).
#[cfg(target_os = "linux")]
fn keyring_key() -> Option<rbx_cloud::ApiKey> {
    futures_lite::future::block_on(async {
        let keyring = oo7::Keyring::new().await.ok()?;
        keyring.unlock().await.ok()?;
        let items = keyring
            .search_items(&[("url", "https://apis.roblox.com/rbx-native")])
            .await
            .ok()?;
        let item = items.into_iter().next()?;
        item.unlock().await.ok()?;
        let secret = item.secret().await.ok()?;
        let key = std::str::from_utf8(&secret).ok()?.trim();
        (!key.is_empty()).then(|| rbx_cloud::ApiKey::new(key))
    })
}

#[cfg(not(target_os = "linux"))]
fn keyring_key() -> Option<rbx_cloud::ApiKey> {
    None
}
