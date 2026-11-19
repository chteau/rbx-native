//! Records the conformance goldens from a real Roblox server: the suite
//! goes into a copy of `base.rbxl` with the Play probe, is published over
//! the sandbox place, and the Roblox client joins it so a server starts.
//! The runner's lines come back through the probe; each test's result is
//! written to `conformance/goldens/<Test>.json`.
//!
//! Publishes only to the place given, which must be the sandbox place from
//! the editor's settings (`play_sandbox`). The API key is read from the
//! keyring (or `RBX_API_KEY`) and never printed.
//!
//! ```text
//! cargo run -p rbx_sim --example record_goldens -- \
//!     <universe_id> <place_id> <base.rbxl> <cloudflared>
//! ```

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use rbx_play::channel::{ChannelServer, DEFAULT_HOLD};
use rbx_play::inject::{self, ProbeConfig};
use rbx_play::launch::{join_uri, ClientProcess, Launcher};
use rbx_play::protocol::{Side, Token, Uplink, TOKEN_HEADER};
use rbx_play::tunnel::TunnelProvider;
use rbx_sim::conformance::{self, DONE, PASSES, PREFIX};


fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (universe, place, base, cloudflared, passes) = match args.as_slice() {
        [u, p, b, c] => (u, p, b, c, PASSES),
        [u, p, b, c, flag, n] if flag == "--passes" => (u, p, b, c, n.parse().expect("passes")),
        _ => {
            eprintln!("usage: record_goldens <universe_id> <place_id> <base.rbxl> <cloudflared> [--passes N]");
            std::process::exit(2);
        }
    };
    let universe: u64 = universe.parse().expect("universe id");
    let place: u64 = place.parse().expect("place id");
    let started = Instant::now();
    let say = |line: &str| println!("[{:6.1}s] {line}", started.elapsed().as_secs_f64());

    let token = Token::random();
    let session = format!("conformance-{}", std::process::id());
    let channel = ChannelServer::start(token.clone(), DEFAULT_HOLD).expect("channel");
    let tunnel = TunnelProvider::Cloudflared {
        binary: cloudflared.into(),
    }
    .start(channel.port(), Duration::from_secs(30))
    .expect("tunnel");
    say(&format!("tunnel {}", tunnel.public_url));
    wait_for_tunnel(&tunnel.public_url, &token, &say);

    let bytes = std::fs::read(base).expect("base place");
    let mut dom = rbx_binary::deserialize(&bytes).expect("parse base place");
    conformance::add_to(&mut dom, passes);
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
    let database = rbx_reflection::ReflectionDatabase::shared();
    let bytes = rbx_binary::serialize_with_defaults(&dom, |class, key| {
        database.stored_default(class, key).cloned()
    })
    .expect("serialize");
    let key = rbx_cloud::ApiKey::from_env_or_config()
        .or_else(rbx_cloud::keyring::stored_api_key)
        .expect("no API key: set RBX_API_KEY or run rbxstudio's key wizard");
    let client = rbx_cloud::Client::new(Some(key));
    let version = client
        .publish_place(universe, place, &bytes, rbx_cloud::PublishMode::Published)
        .expect("publish");
    say(&format!("published version {version}"));
    // A server still running the previous version would never run the suite.
    if let Err(err) = client.restart_servers(universe, true) {
        say(&format!("restart servers: {err}"));
    }

    let launcher = Launcher::detect();
    say(&format!("launching {launcher:?}"));
    let game = ClientProcess::spawn(launcher, &join_uri(place, &session)).expect("launch");

    let mut lines = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(600);
    let finished = loop {
        match channel.events().recv_timeout(Duration::from_millis(500)) {
            Ok(Uplink::Log {
                side: Side::Server,
                message,
                ..
            }) => {
                if message.starts_with(PREFIX) || message == DONE {
                    say(&message);
                }
                let done = message == DONE;
                lines.push(message);
                if done {
                    break true;
                }
            }
            Ok(Uplink::Hello { place_version, .. }) => {
                say(&format!("server up, place version {place_version}"))
            }
            Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break false,
        }
        if game.exited() {
            say("the client exited");
            break false;
        }
        if Instant::now() > deadline {
            say("10 minutes without the suite finishing");
            break false;
        }
    };
    drop(game);
    drop(tunnel);

    let results: BTreeMap<String, Vec<serde_json::Value>> = conformance::parse(&lines);
    let recorded = format!(
        "real Roblox server, place {place} version {version}, joined with {launcher:?}"
    );
    for test in conformance::TESTS {
        let Some(passes) = results.get(test.name) else {
            continue;
        };
        let (result, spread) = conformance::aggregate(passes);
        let mut golden = serde_json::json!({
            "status": "recorded",
            "source": recorded,
            "passes": passes.len(),
            "result": result,
            "spread": spread,
        });
        // Why a test is expected to differ is ours to keep across recordings.
        if let Some(gap) = conformance::golden(test.name).and_then(|g| g.get("known_gap").cloned()) {
            golden["known_gap"] = gap;
        }
        let path = conformance::goldens_dir().join(format!("{}.json", test.name));
        let text = serde_json::to_string_pretty(&golden).expect("json") + "\n";
        std::fs::write(&path, text).expect("write golden");
        say(&format!("wrote {}", path.display()));
    }
    if !finished {
        std::process::exit(1);
    }
}

fn wait_for_tunnel(url: &str, token: &Token, say: &dyn Fn(&str)) {
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
