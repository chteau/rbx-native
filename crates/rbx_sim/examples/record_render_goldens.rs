//! Captures the render fidelity references from the real client: every
//! `rbx_viewer::fidelity` scene goes into one place with the Play probe, is
//! published over the sandbox place, and the client joins it on `$DISPLAY`
//! (an Xephyr exactly [`CAPTURE`] in size). The client script shows each
//! scene in turn and prints when it is ready; the screen is captured then.
//!
//! Writes the full-size captures to `<out>/<scene>.png`, the downsampled
//! references to `crates/rbx_viewer/fidelity/refs/`, and the sun and moon
//! directions the client reported to `<out>/directions.json`.
//!
//! Publishes only to the place given, which must be the sandbox place from
//! the editor's settings. The API key is read from the keyring (or
//! `RBX_API_KEY`) and never printed. Sober is started with its graphics
//! quality forced to the top level for the session only.
//!
//! ```text
//! cargo run -p rbx_sim --example record_render_goldens -- \
//!     <universe_id> <place_id> <cloudflared> <out> [--only a,b]
//! ```

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("record_render_goldens captures an X display: Linux only");
}

#[cfg(target_os = "linux")]
fn main() {
    linux::main();
}

#[cfg(target_os = "linux")]
#[path = "support/gestures.rs"]
#[allow(dead_code)]
mod gestures;
#[cfg(target_os = "linux")]
#[path = "support/tunnel.rs"]
mod tunnel;

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use rbx_play::channel::{ChannelServer, DEFAULT_HOLD};
    use rbx_play::inject::{self, ProbeConfig};
    use rbx_play::launch::{join_uri, ClientOptions, ClientProcess, Launcher};
    use rbx_play::protocol::{LogLevel, Token, Uplink};
    use rbx_play::tunnel::TunnelProvider;
    use rbx_viewer::fidelity::{self, metrics::Image, roblox, CAPTURE, COMPARE};
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::{ConfigureWindowAux, ConnectionExt, ImageFormat};

    use super::gestures;
    use super::tunnel::wait_for_tunnel;

    /// The client's own setting for its graphics quality, the top of the 21
    /// levels its frame-rate manager steps through, so no frame is drawn at a
    /// level the client picked for itself.
    const QUALITY_FLAG: &str = "DFIntDebugFRMQualityLevelOverride";
    const TOP_QUALITY: u32 = 21;
    /// After the script says a scene is ready, so the frame on screen is one
    /// drawn after the print.
    const CAPTURE_DELAY: Duration = Duration::from_millis(500);

    pub fn main() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        let (Some([universe, place, cloudflared, out]), rest) = (
            args.get(..4).and_then(|a| <&[String; 4]>::try_from(a).ok()),
            args.get(4..).unwrap_or_default(),
        ) else {
            eprintln!("usage: record_render_goldens <universe_id> <place_id> <cloudflared> <out> [--only a,b]");
            std::process::exit(2);
        };
        let only: Vec<String> = rest
            .iter()
            .position(|a| a == "--only")
            .and_then(|i| rest.get(i + 1))
            .map(|list| list.split(',').map(str::to_owned).collect())
            .unwrap_or_default();
        let universe: u64 = universe.parse().expect("universe id");
        let place: u64 = place.parse().expect("place id");
        let out = PathBuf::from(out);
        std::fs::create_dir_all(&out).expect("out dir");
        let started = Instant::now();
        let say = |line: &str| println!("[{:6.1}s] {line}", started.elapsed().as_secs_f64());

        let scenes: Vec<_> = fidelity::scenes()
            .into_iter()
            .filter(|s| only.is_empty() || only.iter().any(|o| o == s.name))
            .collect();
        let token = Token::random();
        let session = format!("fidelity-{}", std::process::id());
        let channel = ChannelServer::start(token.clone(), DEFAULT_HOLD).expect("channel");
        let tunnel = TunnelProvider::Cloudflared {
            binary: cloudflared.into(),
        }
        .start(channel.port(), Duration::from_secs(30))
        .expect("tunnel");
        say(&format!("tunnel {}", tunnel.public_url));
        wait_for_tunnel(&tunnel.public_url, &token, &say);

        let database = rbx_reflection::ReflectionDatabase::shared();
        let key = rbx_cloud::ApiKey::from_env_or_config()
            .or_else(rbx_cloud::keyring::stored_api_key)
            .expect("no API key: set RBX_API_KEY or run rbxstudio's key wizard");
        let client = rbx_cloud::Client::new(Some(key));
        let launcher = Launcher::detect();
        let options = ClientOptions {
            sober_config: vec![("fflags".to_owned(), fflags())],
            ..Default::default()
        };
        let mut directions = BTreeMap::new();
        let mut finished = true;
        for group in roblox::groups(scenes) {
            let mut dom = roblox::place(&group, database);
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
            let names: Vec<&str> = group.iter().map(|s| s.name).collect();
            say(&format!(
                "published version {version}: {}",
                names.join(", ")
            ));
            // A server still running the previous version would show the
            // previous scenes.
            if let Err(err) = client.restart_servers(universe, true) {
                say(&format!("restart servers: {err}"));
            }
            say(&format!("launching {launcher:?}"));
            let game = ClientProcess::spawn_with(launcher, &join_uri(place, &session), &options)
                .expect("launch");
            let shown = show(&channel, &game, &out, &mut directions, &say);
            drop(game);
            if !shown {
                finished = false;
                break;
            }
        }
        drop(tunnel);
        let text = serde_json::to_string_pretty(&directions).expect("json") + "\n";
        std::fs::write(out.join("directions.json"), text).expect("directions");
        if !finished {
            std::process::exit(1);
        }
    }

    /// Captures each scene the client says is ready until it says it is
    /// done (`true`), fails, exits or runs out of time (`false`).
    fn show(
        channel: &ChannelServer,
        game: &ClientProcess,
        out: &Path,
        directions: &mut BTreeMap<String, serde_json::Value>,
        say: &dyn Fn(&str),
    ) -> bool {
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            let _ = fit_window();
            match channel.events().recv_timeout(Duration::from_millis(500)) {
                Ok(Uplink::Log {
                    side,
                    level,
                    message,
                    ..
                }) => {
                    if !matches!(level, LogLevel::Output) {
                        say(&format!("{side:?} {level:?}: {message}"));
                    }
                    if matches!(level, LogLevel::Error) && message.contains("Fidelity") {
                        return false;
                    }
                    if let Some(quality) = message.strip_prefix(roblox::QUALITY) {
                        say(&format!("saved quality level {quality}"));
                    } else if let Some(ready) = message.strip_prefix(roblox::READY) {
                        let mut words = ready.split_whitespace();
                        let name = words.next().unwrap_or_default().to_owned();
                        let numbers: Vec<f64> = words.filter_map(|w| w.parse().ok()).collect();
                        std::thread::sleep(CAPTURE_DELAY);
                        match capture() {
                            Ok(image) => {
                                save(out, &name, &image);
                                say(&format!("captured {name}"));
                            }
                            Err(why) => say(&format!("{name}: capture failed: {why}")),
                        }
                        let (sun, moon) = numbers.split_at(numbers.len().min(3));
                        directions.insert(name, serde_json::json!({ "sun": sun, "moon": moon }));
                    } else if message == roblox::DONE {
                        return true;
                    }
                }
                Ok(Uplink::Hello { place_version, .. }) => {
                    say(&format!("server up, place version {place_version}"))
                }
                Ok(_) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return false,
            }
            if game.exited() {
                say("the client exited");
                return false;
            }
            if Instant::now() > deadline {
                say("5 minutes without every scene shown");
                return false;
            }
        }
    }

    /// The person's own `fflags` with the quality override added.
    fn fflags() -> serde_json::Value {
        let mut flags = rbx_play::sober_config::default_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|text| {
                let (_, body) = rbx_play::sober_config::split_header(&text);
                serde_json::from_str::<serde_json::Value>(body).ok()
            })
            .and_then(|config| config.get("fflags").cloned())
            .and_then(|flags| flags.as_object().cloned())
            .unwrap_or_default();
        flags.insert(QUALITY_FLAG.to_owned(), TOP_QUALITY.into());
        serde_json::Value::Object(flags)
    }

    /// Moves the client's window to the top left and sizes it to [`CAPTURE`],
    /// unless it is there already.
    fn fit_window() -> Result<(), String> {
        let window = gestures::client_window()?;
        let (conn, _) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let (width, height) = CAPTURE;
        let geometry = conn
            .get_geometry(window)
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        let at = (geometry.x, geometry.y, geometry.width, geometry.height);
        if at == (0, 0, width as u16, height as u16) {
            return Ok(());
        }
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
        Ok(())
    }

    /// The top-left [`CAPTURE`] of the screen, as RGBA8.
    fn capture() -> Result<Image, String> {
        let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
        let root = conn.setup().roots[screen].root;
        let (width, height) = CAPTURE;
        let reply = conn
            .get_image(
                ImageFormat::Z_PIXMAP,
                root,
                0,
                0,
                width as u16,
                height as u16,
                !0,
            )
            .map_err(|e| e.to_string())?
            .reply()
            .map_err(|e| e.to_string())?;
        if reply.data.len() != (width * height * 4) as usize {
            return Err(format!(
                "{} bytes at depth {}: expected 32-bit pixels",
                reply.data.len(),
                reply.depth
            ));
        }
        let rgba = reply
            .data
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|[b, g, r, _]| [*r, *g, *b, 255])
            .collect();
        Ok(Image::new(width, height, rgba))
    }

    fn save(out: &Path, name: &str, image: &Image) {
        let full = out.join(format!("{name}.png"));
        fidelity::write_png(&full, (image.width, image.height), &image.rgba).expect("capture");
        let (w, h) = COMPARE;
        let small = image.downsample(w, h);
        fidelity::write_png(&fidelity::reference_path(name), (w, h), &small.rgba)
            .expect("reference");
    }
}
