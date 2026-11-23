//! A collaborator playing a Local Run through the co-edit room's relay, as
//! two editors do: a hosted relay on loopback, the session's host and the
//! player each on their own sealed play connection. The player asks, is
//! let in, walks on their own keys, gets their client's frames, leaves;
//! a second player is told when the host ends the session. The bytes on
//! the wire (seals and addresses included) are measured at 60 Hz.

use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use rbx_dom::{Variant, Vector3Data, WeakDom};
use rbx_presence::play::{Config, Event, Link};
use rbx_sim::remote::{self, Down, Frame, Host, Up, Viewer};
use rbx_sim::replay::Action;
use rbx_sim::{Input, Session};

const SECRET: &str = "place secret";
const PLACE: u64 = 42;

fn link(url: &str, pid: u64) -> (Link, Receiver<Event>) {
    let key = rbx_presence::auth::derive_key(SECRET, PLACE);
    let (link, events) = Link::start(Config {
        relay_url: url.into(),
        room: rbx_presence::auth::room_id(&key),
        key,
        enc_key: rbx_presence::crypt::enc_key(SECRET, PLACE),
        user_id: 1,
        pid,
    });
    assert_eq!(
        events.recv_timeout(Duration::from_secs(10)),
        Ok(Event::Connected)
    );
    (link, events)
}

/// The next message, waiting for it.
fn next(events: &Receiver<Event>) -> (u64, Vec<u8>) {
    loop {
        match events.recv_timeout(Duration::from_secs(10)) {
            Ok(Event::Message { from, bytes }) => return (from, bytes),
            Ok(other) => panic!("{other:?}"),
            Err(e) => panic!("no message: {e}"),
        }
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

#[test]
fn a_collaborator_plays_through_the_relay() {
    let key = rbx_presence::auth::derive_key(SECRET, PLACE);
    let relay = rbx_presence::host::Host::start(&key).unwrap();
    let url = relay.local_url();
    const HOST: u64 = 0x11;
    const GUEST: u64 = 0x22;
    let (host_link, host_events) = link(&url, HOST);
    let (guest_link, guest_events) = link(&url, GUEST);

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/smoke/obby.rbxlx");
    let mut session = Session::new(rbx_sim::load::read_place(&path).unwrap()).unwrap();
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
    // frame down.
    let mut viewer = Viewer::default();
    let start = Instant::now();
    let mut start_at = None;
    let mut frame_trips = Duration::ZERO;
    let (base_up, base_down) = (
        guest_link.stats().sent.load(Ordering::Relaxed),
        host_link.stats().sent.load(Ordering::Relaxed),
    );
    const FRAMES: u32 = 180;
    for frame in 0..FRAMES {
        let mut actions = vec![Action::Input(Input::Move {
            position: [400.0, 300.0],
            delta: [1.0, 0.0],
        })];
        if frame == 20 {
            actions.push(Action::Input(Input::Key {
                code: "W".into(),
                down: true,
            }));
        }
        guest_link.send(HOST, remote::encode_up(&Up::Actions(actions)));
        let (from, bytes) = next(&host_events);
        host.receive(&mut session, from, remote::decode_up(&bytes).unwrap());
        session.step();
        let i = session
            .clients
            .iter()
            .position(|c| c.player == player)
            .unwrap();
        if frame == 21 {
            start_at = root_part(&session.server.dom(), "Guest");
        }
        let changes = session.clients[i].sim.take_view_changes();
        let sim = &mut session.clients[i].sim;
        let view = Frame {
            camera: sim.camera(),
            core: Some(sim.core_gui()),
            ..Frame::default()
        };
        let (to, down) = host.frame(player, &sim.dom(), &changes, view).unwrap();
        let sent = Instant::now();
        host_link.send(to, remote::encode_down(&down));
        let Down::Frame(frame) = remote::decode_down(&next(&guest_events).1).unwrap() else {
            panic!("a frame");
        };
        frame_trips += sent.elapsed();
        viewer.apply(&frame);
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
    assert_eq!(
        seen,
        root_part(&session.clients[i].sim.dom(), "Guest").unwrap(),
        "their copy shows their character where their client has it"
    );

    // Measured at 60 Hz of game time; wall time ran faster, in lockstep.
    let seconds = f64::from(FRAMES) / rbx_sim::FRAME_HZ;
    let up = guest_link.stats().sent.load(Ordering::Relaxed) - base_up;
    let down = host_link.stats().sent.load(Ordering::Relaxed) - base_down;
    eprintln!(
        "through the relay at 60 Hz: {:.1} KB/s down (first frame included), {:.2} KB/s up; \
         a frame host to player in {:.2} ms on loopback; {:.1} s wall",
        down as f64 / seconds / 1000.0,
        up as f64 / seconds / 1000.0,
        frame_trips.as_secs_f64() * 1000.0 / f64::from(FRAMES),
        start.elapsed().as_secs_f64()
    );
    assert!((up as f64 / seconds) < 8000.0);

    // They leave; then another asks and is told when the host ends.
    guest_link.send(HOST, remote::encode_up(&Up::Leave));
    let (from, bytes) = next(&host_events);
    assert_eq!(
        host.receive(&mut session, from, remote::decode_up(&bytes).unwrap()),
        Some(remote::Event::Left(player))
    );
    assert!(session.clients.iter().all(|c| c.player != player));
    guest_link.send(
        HOST,
        remote::encode_up(&Up::Ask {
            name: "Guest".into(),
            user_id: 4242,
            template: None,
        }),
    );
    let (from, bytes) = next(&host_events);
    host.receive(&mut session, from, remote::decode_up(&bytes).unwrap());
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
}
