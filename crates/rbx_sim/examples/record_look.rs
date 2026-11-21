//! Puts a place file on the sandbox place and joins it in the real client,
//! for side-by-side captures against Play (local) of the same file: the
//! place is published as it is, servers restart, and the client launches on
//! `$DISPLAY` (an Xephyr), its window fitted to the size given (the size of
//! Play (local)'s viewport) at the top left. It runs until the client
//! exits or `seconds` pass; driving and capturing are left to the caller.
//!
//! Publishes only to the place given, which must be the sandbox place from
//! the editor's settings. The API key is read from the keyring (or
//! `RBX_API_KEY`) and never printed. A place file of `-` publishes nothing:
//! the client only joins the place as it is live, for comparing against a
//! game that must stay untouched.
//!
//! ```text
//! cargo run -p rbx_sim --example record_look -- \
//!     <universe_id> <place_id> <place file> <width> <height> <seconds>
//! ```

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("record_look drives an X display: Linux only");
}

#[cfg(target_os = "linux")]
#[path = "support/gestures.rs"]
#[allow(dead_code)]
mod gestures;

#[cfg(target_os = "linux")]
fn main() {
    use std::time::{Duration, Instant};

    use rbx_play::launch::{join_uri, ClientOptions, ClientProcess, Launcher};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt};

    let args: Vec<String> = std::env::args().skip(1).collect();
    let [universe, place, file, width, height, seconds] = args.as_slice() else {
        eprintln!(
            "usage: record_look <universe_id> <place_id> <place file> <width> <height> <seconds>"
        );
        std::process::exit(2);
    };
    let universe: u64 = universe.parse().expect("universe id");
    let place: u64 = place.parse().expect("place id");
    let (width, height): (u32, u32) = (
        width.parse().expect("width"),
        height.parse().expect("height"),
    );
    let seconds: u64 = seconds.parse().expect("seconds");

    if file != "-" {
        let database = rbx_reflection::ReflectionDatabase::shared();
        let dom = rbx_sim::load::read_place(std::path::Path::new(file)).expect("place file");
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
        println!("published version {version}");
        if let Err(err) = client.restart_servers(universe, true) {
            println!("restart servers: {err}");
        }
    }
    let launcher = Launcher::detect();
    let game = ClientProcess::spawn_with(launcher, &join_uri(place, ""), &ClientOptions::default())
        .expect("launch");
    println!("launched {launcher:?}");

    let fit = || -> Result<(), String> {
        let window = gestures::client_window()?;
        let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let geometry = conn
            .get_geometry(window)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        if (geometry.x, geometry.y, geometry.width, geometry.height)
            != (0, 0, width as u16, height as u16)
        {
            conn.configure_window(
                window,
                &ConfigureWindowAux::new()
                    .x(0)
                    .y(0)
                    .width(width)
                    .height(height),
            )
            .map_err(|e| e.to_string())?;
            conn.flush().map_err(|e| e.to_string())?;
        }
        Ok(())
    };
    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline && !game.exited() {
        let _ = fit();
        std::thread::sleep(Duration::from_millis(500));
    }
    drop(game);
}
