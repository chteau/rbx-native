//! A collaborator playing a Local Run through the co-edit room's relay, as
//! two editors do: a hosted relay on loopback, the session's host and the
//! player each on their own sealed play connection. The player asks, is
//! let in, walks on their own keys (their character moving on their screen
//! before the frames say so, and settling where the host has it), hears
//! the game's sound from where their camera is, is carried through a
//! teleport on the same connection, leaves; a second player is told when
//! the host ends the session. The bytes on the wire (seals and addresses
//! included) are measured at 60 Hz.
//!
//! Its own test binary: it points the asset cache at a folder of its own,
//! holding the sound the game plays.

use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rbx_assets::{AssetFetcher, FetchError};
use rbx_dom::{Ref, Variant, Vector3Data, WeakDom};
use rbx_presence::play::{Config, Event, Link};
use rbx_sim::remote::{self, Down, Frame, Host, Shot, Speaker, Up, Viewer};
use rbx_sim::replay::Action;
use rbx_sim::{Input, Session};

const SECRET: &str = "place secret";
const PLACE: u64 = 42;
const CHIME: u64 = 990_002;

fn link(url: &str, pid: u64) -> (Link, Receiver<Event>) {
    let key = rbx_presence::auth::derive_key(SECRET, PLACE);
    let (link, events) = Link::start(Config {
        relay_url: url.into(),
        room: rbx_presence::auth::room_id(&key),
        key,
        enc_key: rbx_presence::crypt::enc_key(SECRET, PLACE),
        user_id: 1,
        pid,
        kind: rbx_presence::relay::RelayKind::Hosted,
    });
    assert_eq!(
        events.recv_timeout(Duration::from_secs(10)),
        Ok(Event::Connected)
    );
    (link, events)
}

/// The next message, waiting for it.
fn next(events: &Receiver<Event>) -> (u64, Vec<u8>) {
    match events.recv_timeout(Duration::from_secs(10)) {
        Ok(Event::Message { from, bytes }) => (from, bytes),
        Ok(other) => panic!("{other:?}"),
        Err(e) => panic!("no message: {e}"),
    }
}

fn root_part(dom: &WeakDom, name: &str) -> Option<Vector3Data> {
    let workspace = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))?;
    let child = |parent, name: &str| {
        dom.get(parent)?
            .children()
            .iter()
            .copied()
            .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
    };
    let root = child(child(workspace, name)?, "HumanoidRootPart")?;
    match dom.get(root)?.properties().get("CFrame")? {
        Variant::CFrame(c) => Some(c.position),
        _ => None,
    }
}

fn apart(a: Vector3Data, b: Vector3Data) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2) + (a.z - b.z).powi(2)).sqrt()
}

/// A second of 440 Hz, mono 16-bit at 48 kHz.
fn tone() -> Vec<u8> {
    let samples: Vec<i16> = (0..48_000)
        .map(|i| ((i as f32 / 48_000.0 * 440.0 * std::f32::consts::TAU).sin() * 16000.0) as i16)
        .collect();
    let data = (samples.len() * 2) as u32;
    let mut wav = b"RIFF".to_vec();
    wav.extend((36 + data).to_le_bytes());
    wav.extend(b"WAVEfmt ");
    wav.extend(16u32.to_le_bytes());
    wav.extend([1u16, 1].iter().flat_map(|f| f.to_le_bytes()));
    wav.extend([48_000u32, 96_000].iter().flat_map(|f| f.to_le_bytes()));
    wav.extend([2u16, 16].iter().flat_map(|f| f.to_le_bytes()));
    wav.extend(b"data");
    wav.extend(data.to_le_bytes());
    wav.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
    wav
}

/// The collaborator's own computer, where the chime comes from.
struct Chime;

impl AssetFetcher for Chime {
    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, FetchError> {
        if id == CHIME {
            Ok(tone())
        } else {
            Err(FetchError::NotFound(id))
        }
    }
}

/// Obby, with a server script ringing a looped chime on a block to the
/// spawn's right for a second, a second in.
fn place() -> WeakDom {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/smoke/obby.rbxlx");
    let mut dom = rbx_sim::load::read_place(&path).unwrap();
    let sss = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| {
            dom.get(*r)
                .is_some_and(|i| i.class() == "ServerScriptService")
        })
        .unwrap();
    let script = dom.new_instance("Script", "Chime", Some(sss));
    let source = format!(
        "local part = Instance.new('Part') part.Anchored = true \
         part.Position = Vector3.new(40, 5, 0) part.Parent = workspace \
         local s = Instance.new('Sound') s.Name = 'Chime' s.Looped = true \
         s.SoundId = 'rbxassetid://{CHIME}' s.Parent = part \
         task.wait(1) s:Play() task.wait(1) s:Stop()"
    );
    let _ = dom.set_property(script, "Source", Variant::String(source));
    dom.take_changes();
    dom
}

/// One lockstep frame: the guest's actions up, a step, their frame down
/// (and its trip time).
struct Trip<'a> {
    host_link: &'a Link,
    host_events: &'a Receiver<Event>,
    guest_link: &'a Link,
    guest_events: &'a Receiver<Event>,
}

impl Trip<'_> {
    fn frame(
        &self,
        session: &mut Session,
        host: &mut Host,
        player: Ref,
        actions: Option<Vec<Action>>,
    ) -> (Frame, Duration) {
        if let Some(actions) = actions {
            self.guest_link
                .send(HOST, remote::encode_up(&Up::Actions(actions)));
            let (from, bytes) = next(self.host_events);
            host.receive(session, from, remote::decode_up(&bytes).unwrap());
        }
        session.step();
        let i = session
            .clients
            .iter()
            .position(|c| c.player == player)
            .unwrap();
        let sim = &mut session.clients[i].sim;
        let changes = sim.take_view_changes();
        let view = Frame {
            camera: sim.camera(),
            core: Some(sim.core_gui()),
            ..Frame::default()
        };
        let dom = sim.dom();
        let shot = Shot {
            dom: &dom,
            changes: &changes,
            sounds: sim.sound_clocks(),
            time: sim.time(),
        };
        let (to, down) = host.frame(player, shot, view, false).unwrap();
        let sent = Instant::now();
        self.host_link.send(to, remote::encode_down(&down));
        let Down::Frame(frame) = remote::decode_down(&next(self.guest_events).1).unwrap() else {
            panic!("a frame");
        };
        (*frame, sent.elapsed())
    }
}

const HOST: u64 = 0x11;
const GUEST: u64 = 0x22;

#[test]
fn a_collaborator_plays_through_the_relay() {
    let cache = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("remote-play-cache");
    let _ = std::fs::remove_dir_all(&cache);
    std::env::set_var("XDG_CACHE_HOME", &cache);
    let key = rbx_presence::auth::derive_key(SECRET, PLACE);
    let relay = rbx_presence::host::Host::start(&key).unwrap();
    let url = relay.local_url();
    let (host_link, host_events) = link(&url, HOST);
    let (guest_link, guest_events) = link(&url, GUEST);
    let trip = Trip {
        host_link: &host_link,
        host_events: &host_events,
        guest_link: &guest_link,
        guest_events: &guest_events,
    };

    let mut session = Session::new(place()).unwrap();
    session.start();
    let mut host = Host::default();

    // The host tells the room; the guest asks.
    host_link.send(
        0,
        remote::encode_down(&Down::Open {
            host: "Host".into(),
        }),
    );
    let (from, bytes) = next(&guest_events);
    assert_eq!(from, HOST);
    assert_eq!(
        remote::decode_down(&bytes).unwrap(),
        Down::Open {
            host: "Host".into()
        }
    );
    guest_link.send(
        HOST,
        remote::encode_up(&Up::Ask {
            name: "Guest".into(),
            user_id: 4242,
            template: None,
        }),
    );
    let (from, bytes) = next(&host_events);
    let ask = remote::decode_up(&bytes).unwrap();
    assert_eq!(
        host.receive(&mut session, from, ask),
        Some(remote::Event::Offered("Guest".into()))
    );
    let (_, welcome) = host.accept(&mut session, GUEST).unwrap();
    host_link.send(GUEST, remote::encode_down(&welcome));
    let Down::Welcome { player, .. } = remote::decode_down(&next(&guest_events).1).unwrap() else {
        panic!("welcomed");
    };

    // Lockstep, a frame at a time: the guest's input up, a step, the
    // frame down; their character predicted on their side.
    let mut viewer = Viewer::default();
    let mut speaker = Speaker::new(Some(Arc::new(Chime)));
    let (mut heard, mut rang, mut ahead) = (Vec::new(), false, 0.0f32);
    let start = Instant::now();
    let mut start_at = None;
    let mut frame_trips = Duration::ZERO;
    let (base_up, base_down) = (
        guest_link.stats().sent.load(Ordering::Relaxed),
        host_link.stats().sent.load(Ordering::Relaxed),
    );
    const FRAMES: u32 = 180;
    const PRESS: u32 = 20;
    const RELEASE: u32 = 120;
    for frame in 0..FRAMES {
        let mut actions = vec![Action::Input(Input::Move {
            position: [400.0, 300.0],
            delta: [1.0, 0.0],
        })];
        if frame == PRESS || frame == RELEASE {
            actions.push(Action::Input(Input::Key {
                code: "W".into(),
                down: frame == PRESS,
            }));
        }
        viewer.input(&actions);
        let (got, trip_time) = trip.frame(&mut session, &mut host, player, Some(actions));
        viewer.predictor.sent();
        frame_trips += trip_time;
        viewer.apply(&got);
        speaker.hear(&got);
        if frame == PRESS + 1 {
            start_at = root_part(&session.server.dom(), "Guest");
        }
        let (_, camera) = viewer.predict(player, got.camera, 1.0 / rbx_sim::FRAME_HZ);
        let dom = viewer.dom_rc();
        let dom = dom.borrow();
        if (PRESS..RELEASE).contains(&frame) {
            let i = session
                .clients
                .iter()
                .position(|c| c.player == player)
                .unwrap();
            let truth = root_part(&session.clients[i].sim.dom(), "Guest").unwrap();
            ahead = ahead.max(apart(root_part(&dom, "Guest").unwrap(), truth));
        }
        speaker.advance(&dom, 1.0 / rbx_sim::FRAME_HZ);
        heard.push(speaker.playing().len());
        // The chime loads on the guest's side, from their computer.
        for _ in 0..50 {
            let voices = speaker.voices(&dom, camera.map(|(c, _)| c));
            if voices.is_empty() && !speaker.playing().is_empty() {
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            if let Some(voice) = voices.first() {
                assert!(
                    voice.pan > 0.3,
                    "the chime is to their right: {}",
                    voice.pan
                );
                rang = true;
            }
            break;
        }
    }
    let i = session
        .clients
        .iter()
        .position(|c| c.player == player)
        .unwrap();
    let (from, to) = (
        start_at.unwrap(),
        root_part(&session.server.dom(), "Guest").unwrap(),
    );
    assert!(
        ((to.x - from.x).powi(2) + (to.z - from.z).powi(2)).sqrt() > 10.0,
        "the guest's W walked their character on the host"
    );
    let seen = root_part(&viewer.dom_rc().borrow(), "Guest").unwrap();
    let truth = root_part(&session.clients[i].sim.dom(), "Guest").unwrap();
    assert!(
        apart(seen, truth) < 0.05,
        "their copy shows their character where their client has it, after the \
         prediction settled: {seen:?} vs {truth:?}"
    );
    assert!(ahead < 1.0, "the prediction stayed close: {ahead} studs");
    assert!(rang, "they heard the chime");
    let on = heard
        .iter()
        .position(|n| *n == 1)
        .expect("the chime started");
    assert!(
        heard[on..].contains(&0),
        "and stopped when the server stopped it"
    );

    // Measured at 60 Hz of game time; wall time ran faster, in lockstep.
    let seconds = f64::from(FRAMES) / rbx_sim::FRAME_HZ;
    let up = guest_link.stats().sent.load(Ordering::Relaxed) - base_up;
    let down = host_link.stats().sent.load(Ordering::Relaxed) - base_down;
    eprintln!(
        "through the relay at 60 Hz: {:.1} KB/s down (first frame included), {:.2} KB/s up; \
         a frame host to player in {:.2} ms on loopback; prediction at most {ahead:.2} studs \
         off; {:.1} s wall",
        down as f64 / seconds / 1000.0,
        up as f64 / seconds / 1000.0,
        frame_trips.as_secs_f64() * 1000.0 / f64::from(FRAMES),
        start.elapsed().as_secs_f64()
    );
    assert!((up as f64 / seconds) < 8000.0);

    // A teleport: the session restarts, the guest comes along on the same
    // connection, welcomed again into the new one and sent the whole
    // tree.
    let carried = host.carry();
    assert!(host.end().is_empty(), "nobody is told the session ended");
    let mut next_session = Session::new(place()).unwrap();
    next_session.start();
    for (to, down) in host.arrive(&mut next_session, carried) {
        host_link.send(to, remote::encode_down(&down));
    }
    let Down::Welcome {
        player: arrived, ..
    } = remote::decode_down(&next(&guest_events).1).unwrap()
    else {
        panic!("welcomed again");
    };
    let (first, _) = trip.frame(&mut next_session, &mut host, arrived, None);
    assert!(first.reset, "the new place's tree, whole");
    viewer.apply(&first);
    // Their character spawns in the new place a few frames in.
    for _ in 0..30 {
        let (frame, _) = trip.frame(&mut next_session, &mut host, arrived, None);
        viewer.apply(&frame);
    }
    assert!(root_part(&viewer.dom_rc().borrow(), "Guest").is_some());
    assert!(
        guest_events.try_recv().is_err() && host_events.try_recv().is_err(),
        "no connection dropped"
    );

    // They leave; then another asks and is told when the host ends.
    guest_link.send(HOST, remote::encode_up(&Up::Leave));
    let (from, bytes) = next(&host_events);
    assert_eq!(
        host.receive(&mut next_session, from, remote::decode_up(&bytes).unwrap()),
        Some(remote::Event::Left(arrived))
    );
    assert!(next_session.clients.iter().all(|c| c.player != arrived));
    guest_link.send(
        HOST,
        remote::encode_up(&Up::Ask {
            name: "Guest".into(),
            user_id: 4242,
            template: None,
        }),
    );
    let (from, bytes) = next(&host_events);
    host.receive(&mut next_session, from, remote::decode_up(&bytes).unwrap());
    for to in host.end() {
        host_link.send(
            to,
            remote::encode_down(&Down::Ended("Host ended the session.".into())),
        );
    }
    assert_eq!(
        remote::decode_down(&next(&guest_events).1).unwrap(),
        Down::Ended("Host ended the session.".into())
    );
    let _ = std::fs::remove_dir_all(&cache);
}
