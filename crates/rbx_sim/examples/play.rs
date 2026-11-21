//! Play (local) without the editor: the place's server, one noob player,
//! every Output line from both sides, then where the character stands.
//!
//! ```text
//! cargo run -p rbx_sim --example play -- place.rbxl --seconds 20 [--walk 3] [--jump]
//!     [--place-id N] [--game-id N] [--track] [--live] [--live-write]
//! ```
//!
//! `--walk N` holds W from second N for two seconds; `--jump` presses Space
//! once the walk ends; `--track` prints the character's root and health
//! every second. `--place-id`/`--game-id` are what `game.PlaceId`/`GameId`
//! read. `--live` runs the DataStores against `--game-id`'s real ones,
//! read-only (writes kept in the run) unless `--live-write` also says so. `PLAY_UID` sets the player's user id (1 by default). Assets come
//! from Roblox through `rbx_cloud`, with the Open Cloud key it finds.

use std::path::PathBuf;
use std::sync::Arc;

use rbx_dom::Variant;
use rbx_sim::{Input, LineKind, Peer, Session};

struct Fetcher(rbx_cloud::Client);

impl rbx_assets::AssetFetcher for Fetcher {
    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, rbx_assets::FetchError> {
        self.0
            .asset(id)
            .map(|content| content.bytes)
            .map_err(|err| rbx_assets::FetchError::Other {
                id,
                message: err.to_string(),
            })
    }
}

/// Player1's character as `peer` sees it: its root's position and its
/// health.
fn character(session: &Session, peer: Peer) -> String {
    let Some((player, _)) = session.clients[0].sim.local_player() else {
        return "no player".into();
    };
    let Some(sim) = session.sim(peer) else {
        return "no peer".into();
    };
    let dom = sim.dom();
    let Some(Variant::Ref(character)) = dom
        .get(player)
        .and_then(|p| p.properties().get("Character"))
    else {
        return "no character".into();
    };
    let Some(model) = dom.get(*character) else {
        return "character gone".into();
    };
    let child = |name: &str| {
        model
            .children()
            .iter()
            .filter_map(|c| dom.get(*c))
            .find(|i| i.name() == name)
    };
    let at = match child("HumanoidRootPart").and_then(|r| r.properties().get("CFrame")) {
        Some(Variant::CFrame(cf)) => format!(
            "{:.1} {:.1} {:.1}",
            cf.position.x, cf.position.y, cf.position.z
        ),
        _ => "no root".into(),
    };
    let health = match child("Humanoid").and_then(|h| h.properties().get("Health")) {
        Some(Variant::Float32(h)) => h.to_string(),
        Some(Variant::Float64(h)) => h.to_string(),
        _ => "?".into(),
    };
    format!("root {at} health {health}")
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut place, mut seconds, mut walk, mut jump) = (None, 10.0, None, false);
    let (mut live, mut live_write) = (false, false);
    let (mut place_id, mut game_id, mut track) = (0, 0, false);
    while let Some(arg) = args.next() {
        let mut number = || args.next().and_then(|s| s.parse::<f64>().ok());
        match arg.as_str() {
            "--seconds" => seconds = number().expect("--seconds N"),
            "--walk" => walk = Some(number().expect("--walk N")),
            "--jump" => jump = true,
            "--track" => track = true,
            "--live" => live = true,
            "--live-write" => live_write = true,
            "--place-id" => place_id = number().expect("--place-id N") as i64,
            "--game-id" => game_id = number().expect("--game-id N") as i64,
            _ => place = Some(PathBuf::from(arg)),
        }
    }
    let place = place.expect(
        "usage: play place.rbxl [--seconds N] [--walk N] [--jump] [--place-id N] [--game-id N]",
    );
    let dom = rbx_sim::load::read_place(&place).expect("read place");
    let mut session = Session::new(dom).expect("session");
    session.set_fetcher(Arc::new(Fetcher(rbx_cloud::Client::new(
        rbx_cloud::ApiKey::from_env_or_config().or_else(rbx_cloud::keyring::stored_api_key),
    ))));
    let mut services = rbx_lua::server::Services {
        place_id,
        game_id,
        ..Default::default()
    };
    if live {
        // Writes only ever reach the sandbox experience the suite records on.
        let live_write = live_write && game_id == 1_113_565_074;
        match rbx_sim::live::connect(game_id as u64, !live_write) {
            Ok(store) => {
                println!(
                    "[info] DataStores: live, experience {game_id} (read-only: {})",
                    !live_write
                );
                services.live = Some(store);
            }
            Err(why) => println!("[info] DataStores: local ({})", why.reason()),
        }
    }
    session.set_services(services);
    session.start();
    let uid = std::env::var("PLAY_UID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    session.join("Player1", uid, None).expect("join");
    let started = std::time::Instant::now();
    let frames = (seconds * rbx_sim::FRAME_HZ).round() as u64;
    for frame in 0..frames {
        let t = frame as f64 / rbx_sim::FRAME_HZ;
        if let Some(at) = walk {
            let key = |down| Input::Key {
                code: "W".into(),
                down,
            };
            if (t - at).abs() < 0.5 / rbx_sim::FRAME_HZ {
                session.input(0, key(true));
            }
            if (t - at - 2.0).abs() < 0.5 / rbx_sim::FRAME_HZ {
                session.input(0, key(false));
                if jump {
                    session.input(
                        0,
                        Input::Key {
                            code: "Space".into(),
                            down: true,
                        },
                    );
                }
            }
            if jump && (t - at - 2.2).abs() < 0.5 / rbx_sim::FRAME_HZ {
                session.input(
                    0,
                    Input::Key {
                        code: "Space".into(),
                        down: false,
                    },
                );
            }
        }
        session.step();
        for (name, message) in session.kicked.drain(..) {
            println!("[{t:7.3} S] kicked: {name}: {message}");
        }
        if track && frame % (rbx_sim::FRAME_HZ as u64) == 0 {
            println!(
                "[{t:7.3} C] track: {}",
                character(&session, Peer::Client(0))
            );
            println!("[{t:7.3} S] track: {}", character(&session, Peer::Server));
        }
        for (peer, output) in session.take_output() {
            let side = match peer {
                Peer::Server => "S",
                Peer::Client(_) => "C",
            };
            for (line, kind) in output.lines().iter().zip(output.kinds()) {
                let tag = match kind {
                    LineKind::Print => "",
                    LineKind::Warn => "warn: ",
                    LineKind::Error => "error: ",
                };
                println!("[{t:7.3} {side}] {tag}{line}");
            }
        }
        if session.clients.is_empty() {
            break;
        }
    }
    println!(
        "-- {seconds}s in {:.2}s wall",
        started.elapsed().as_secs_f64()
    );
}
