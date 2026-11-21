//! Records the streaming probe (`rbx_sim::conformance::streaming`) on a
//! real Roblox server: each variant's place gets the Play probe, is
//! published over the sandbox place, and the Roblox client joins it; the
//! server's sections are written to
//! `conformance/goldens/streaming/<variant>.json`.
//!
//! Publishes only to the place given, which must be the sandbox place. The
//! API key is read from the keyring (or `RBX_API_KEY`) and never printed.
//!
//! ```text
//! cargo run -p rbx_sim --example record_streaming -- \
//!     <universe_id> <place_id> <cloudflared> [Variant,...]
//! ```

use std::time::{Duration, Instant};

use rbx_play::channel::{ChannelServer, DEFAULT_HOLD};
use rbx_play::inject::{self, ProbeConfig};
use rbx_play::launch::{join_uri, ClientProcess, Launcher};
use rbx_play::protocol::{Side, Token, Uplink};
use rbx_play::tunnel::TunnelProvider;
use rbx_sim::conformance::streaming::{self, PREFIX, VARIANTS};

#[path = "support/tunnel.rs"]
mod tunnel;
use tunnel::wait_for_tunnel;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [universe, place, cloudflared] = &args[..3.min(args.len())] else {
        eprintln!("usage: record_streaming <universe_id> <place_id> <cloudflared> [Variant,...]");
        std::process::exit(2);
    };
    let only: Vec<&str> = args
        .get(3)
        .map(|list| list.split(',').collect())
        .unwrap_or_else(|| VARIANTS.iter().map(|(n, _)| *n).collect());
    let universe: u64 = universe.parse().expect("universe id");
    let place: u64 = place.parse().expect("place id");
    let started = Instant::now();
    let say = |line: &str| println!("[{:6.1}s] {line}", started.elapsed().as_secs_f64());

    let token = Token::random();
    let channel = ChannelServer::start(token.clone(), DEFAULT_HOLD).expect("channel");
    let tunnel = TunnelProvider::Cloudflared {
        binary: cloudflared.into(),
    }
    .start(channel.port(), Duration::from_secs(30))
    .expect("tunnel");
    wait_for_tunnel(&tunnel.public_url, &token, &say);
    let key = rbx_cloud::ApiKey::from_env_or_config()
        .or_else(rbx_cloud::keyring::stored_api_key)
        .expect("no API key");
    let client = rbx_cloud::Client::new(Some(key));
    let database = rbx_reflection::ReflectionDatabase::shared();
    let mut failed = false;

    for variant in only {
        let session = format!("streaming-{}-{variant}", std::process::id());
        let mut dom = streaming::place(variant).expect("variant");
        inject::inject(
            &mut dom,
            &ProbeConfig {
                base_url: tunnel.public_url.clone(),
                token: token.as_str().to_owned(),
                session: session.clone(),
                ..Default::default()
            },
        )
        .expect("inject");
        let bytes = rbx_binary::serialize_with_defaults(&dom, |class, key| {
            database.stored_default(class, key).cloned()
        })
        .expect("serialize");
        let version = client
            .publish_place(universe, place, &bytes, rbx_cloud::PublishMode::Published)
            .expect("publish");
        say(&format!("{variant}: published version {version}"));
        if let Err(err) = client.restart_servers(universe, true) {
            say(&format!("restart servers: {err}"));
        }
        let launcher = Launcher::detect();
        let game = ClientProcess::spawn(launcher, &join_uri(place, &session)).expect("launch");
        let mut lines = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(600);
        let sections = loop {
            match channel.events().recv_timeout(Duration::from_millis(500)) {
                Ok(Uplink::Log {
                    side: Side::Server,
                    message,
                    ..
                }) => {
                    if message.starts_with(PREFIX) || message == streaming::DONE {
                        say(&message.chars().take(160).collect::<String>());
                    }
                    lines.push(message);
                    if let Some(sections) = streaming::parse(&lines) {
                        break Some(sections);
                    }
                }
                Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => break None,
            }
            if game.exited() {
                say("the client exited");
                break None;
            }
            if Instant::now() > deadline {
                say("10 minutes without the probe finishing");
                break None;
            }
        };
        drop(game);
        let Some(sections) = sections else {
            failed = true;
            continue;
        };
        let golden = serde_json::json!({
            "source": format!("real Roblox server, place {place} version {version}, joined with {launcher:?}"),
            "sections": sections,
        });
        let path = streaming::golden_path(variant);
        std::fs::create_dir_all(path.parent().expect("dir")).expect("goldens dir");
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&golden).expect("json") + "\n",
        )
        .expect("write golden");
        say(&format!("wrote {}", path.display()));
        // Let the server see the player leave before the next publish.
        std::thread::sleep(Duration::from_secs(5));
    }
    drop(tunnel);
    if failed {
        std::process::exit(1);
    }
}
