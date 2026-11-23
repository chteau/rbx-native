use rbx_dom::{
    Axes, Color3Data, ColorSequence, ColorSequenceKeypoint, Faces, Font, NumberRange,
    NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Rect, UDim, UDim2, UniqueId,
    Vector2Data, Vector3Data,
};

use super::*;
use crate::gui::{HotbarSlot, NameLabel, PlayerRow};
use crate::Input;

fn every_value() -> Vec<Variant> {
    let c = CFrameData {
        position: Vector3Data {
            x: 1.5,
            y: -2.0,
            z: 3.25,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0],
    };
    vec![
        Variant::String("héllo".into()),
        Variant::Bool(true),
        Variant::Int32(-7),
        Variant::Int64(1 << 40),
        Variant::Float32(0.1),
        Variant::Float64(-1e300),
        Variant::BrickColor(194),
        Variant::Color3(Color3Data {
            r: 0.25,
            g: 0.5,
            b: 1.0,
        }),
        Variant::Color3uint8 { r: 1, g: 2, b: 255 },
        Variant::Vector2(Vector2Data { x: 1.0, y: 2.0 }),
        Variant::Vector3(c.position),
        Variant::Vector3int16 {
            x: -32768,
            y: 0,
            z: 32767,
        },
        Variant::Ray {
            origin: c.position,
            direction: c.position,
        },
        Variant::Faces(Faces::from_bits(0b101101)),
        Variant::Axes(Axes::from_bits(0b110)),
        Variant::CFrame(c),
        Variant::OptionalCFrame(None),
        Variant::OptionalCFrame(Some(c)),
        Variant::Enum(3),
        Variant::Ref(Ref::new(77)),
        Variant::NumberSequence(NumberSequence {
            keypoints: vec![NumberSequenceKeypoint {
                time: 0.0,
                value: 1.0,
                envelope: 0.5,
            }],
        }),
        Variant::ColorSequence(ColorSequence {
            keypoints: vec![ColorSequenceKeypoint {
                time: 1.0,
                color: Color3Data {
                    r: 1.0,
                    g: 0.0,
                    b: 0.5,
                },
                envelope: 0.0,
            }],
        }),
        Variant::NumberRange(NumberRange { min: 1.0, max: 2.0 }),
        Variant::Rect(Rect {
            min: Vector2Data { x: 0.0, y: 1.0 },
            max: Vector2Data { x: 2.0, y: 3.0 },
        }),
        Variant::PhysicalProperties(PhysicalProperties::Default),
        Variant::PhysicalProperties(PhysicalProperties::Custom {
            density: 1.0,
            friction: 0.3,
            elasticity: 0.5,
            friction_weight: 1.0,
            elasticity_weight: 2.0,
        }),
        Variant::SharedString(4),
        Variant::UDim(UDim {
            scale: 0.5,
            offset: -10,
        }),
        Variant::UDim2(UDim2 {
            x: UDim {
                scale: 1.0,
                offset: 2,
            },
            y: UDim {
                scale: 0.0,
                offset: -3,
            },
        }),
        Variant::UniqueId(UniqueId {
            index: 1,
            time: 2,
            random: -3,
        }),
        Variant::Font(Font {
            family: "rbxasset://fonts/families/SourceSansPro.json".into(),
            weight: 700,
            style: rbx_dom::FontStyle::Italic,
            cached_face_id: None,
        }),
        Variant::SecurityCapabilities(u64::MAX),
        Variant::Content(Content::None),
        Variant::Content(Content::Uri("rbxassetid://1".into())),
        Variant::Unknown {
            type_id: 99,
            raw: vec![1, 2, 3],
        },
    ]
}

#[test]
fn every_message_and_value_crosses_unchanged() {
    let props: Vec<(String, Variant)> = every_value()
        .into_iter()
        .enumerate()
        .map(|(i, v)| (format!("P{i}"), v))
        .collect();
    let frame = Frame {
        reset: true,
        ops: vec![
            Op::Add {
                at: Ref::new(5),
                parent: Parent::Root,
                class: "Part".into(),
                name: "Part".into(),
                props,
            },
            Op::Prop {
                at: Ref::new(5),
                name: "Name".into(),
                value: Some(Variant::String("Other".into())),
            },
            Op::Prop {
                at: Ref::new(5),
                name: "Gone".into(),
                value: None,
            },
            Op::Move {
                at: Ref::new(6),
                parent: Parent::Under(Ref::new(5)),
            },
            Op::Move {
                at: Ref::new(7),
                parent: Parent::Nil,
            },
            Op::Remove(Ref::new(8)),
            Op::Class {
                at: Ref::new(9),
                class: "Model".into(),
            },
        ],
        camera: Some((
            every_value()
                .into_iter()
                .find_map(|v| match v {
                    Variant::CFrame(c) => Some(c),
                    _ => None,
                })
                .unwrap(),
            70.0,
        )),
        mouse: MouseBehavior::LockCenter,
        player_gui: Some(Ref::new(12)),
        core: Some(CoreGui {
            health: Some((50.0, 100.0)),
            hotbar: Some(vec![HotbarSlot {
                name: "Sword".into(),
                icon: String::new(),
                held: true,
            }]),
            players: Some(vec![PlayerRow {
                user_id: 1,
                name: "a".into(),
                display_name: "A".into(),
                team: Some([1, 2, 3]),
                stats: vec!["5".into()],
            }]),
            stats: vec!["Coins".into()],
            notifications: vec![("t".into(), "x".into())],
            can_reset: true,
            heads: vec![(1, [1.0, 2.0, 3.0])],
            labels: vec![NameLabel {
                text: "a".into(),
                at: [0.0; 3],
                health: Some(0.5),
            }],
            chat: true,
            chat_bar: false,
            topbar: true,
        }),
        chat: vec![ChatLine {
            prefix: "a:".into(),
            text: "hi".into(),
            channel: "RBXGeneral".into(),
            user_id: 1,
        }],
        notices: vec![("n".into(), "m".into())],
        dialog: Some(("Buy".into(), "it".into())),
        bursts: vec![(Ref::new(3), Some(10)), (Ref::new(4), None)],
        rows: vec![(LineKind::Warn, "careful".into())],
        paused: true,
        sounds: vec![(Ref::new(13), Some(1.5)), (Ref::new(14), None)],
        ack: 77,
    };
    for down in [
        Down::Open {
            host: "Host".into(),
        },
        Down::Welcome {
            player: Ref::new(40),
            name: "Someone".into(),
        },
        Down::Frame(Box::new(frame)),
        Down::Ended("bye".into()),
    ] {
        assert_eq!(decode_down(&encode_down(&down)).unwrap(), down);
    }
    for up in [
        Up::Ask {
            name: "Guest".into(),
            user_id: 42,
            template: Some(vec![9; 1000]),
        },
        Up::Actions(vec![
            Action::Input(Input::Key {
                code: "W".into(),
                down: true,
            }),
            Action::Viewport([800.0, 600.0]),
            Action::Chat("hi".into()),
        ]),
        Up::Purchase(true),
        Up::Leave,
        Up::Resync,
    ] {
        assert_eq!(decode_up(&encode_up(&up)).unwrap(), up);
    }
    let mut other = encode_up(&Up::Leave);
    other[0] = VERSION + 1;
    assert!(decode_up(&other).unwrap_err().contains("version"));

    // A version 1 editor's messages still read: its frames end before
    // the sounds and the input count.
    let old = Frame {
        rows: vec![(LineKind::Print, "hi".into())],
        paused: true,
        ..Frame::default()
    };
    let new = encode_down(&Down::Frame(Box::new(old.clone())));
    let body = lz4_flex::decompress_size_prepended(&new[2..]).unwrap();
    let mut v1 = vec![1, b'D'];
    v1.extend(lz4_flex::compress_prepend_size(&body[..body.len() - 2]));
    assert_eq!(decode_down(&v1).unwrap(), Down::Frame(Box::new(old)));
    let mut leave = encode_up(&Up::Leave);
    leave[0] = 1;
    assert_eq!(decode_up(&leave).unwrap(), Up::Leave);
    assert!(
        decode_down(&encode_up(&Up::Leave)).is_err(),
        "each way is its own"
    );
}

/// The client's world as it shows: every instance under Workspace, by
/// referent, with class, name, parent and where it is. (What the
/// simulation writes without a change, an assembly's velocity, a joint's
/// `Transform`, isn't in a frame; the test windows don't get it either.)
fn shown(dom: &WeakDom) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = dom
        .root_refs()
        .iter()
        .copied()
        .filter(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
        .collect();
    while let Some(at) = stack.pop() {
        let Some(i) = dom.get(at) else { continue };
        stack.extend(i.children().iter().copied());
        out.push(format!(
            "{} {} {} {:?} {:?}",
            at.value(),
            i.class(),
            i.name(),
            dom.parent(at),
            i.properties().get("CFrame")
        ));
    }
    out.sort();
    out
}

fn root_part(dom: &WeakDom, name: &str) -> Option<Vector3Data> {
    let workspace = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))?;
    let model = dom
        .get(workspace)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))?;
    let root = dom
        .get(model)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == "HumanoidRootPart"))?;
    match dom.get(root)?.properties().get("CFrame")? {
        Variant::CFrame(c) => Some(c.position),
        _ => None,
    }
}

/// Two peers in one process over a loopback (a queue each way, every
/// message encoded and decoded as on the wire): the remote player asks,
/// is let in, walks on their own keys, sees their client's tree kept in
/// step by the frames, leaves; a second one is told when the host ends.
#[test]
fn a_remote_player_joins_walks_sees_their_client_and_leaves() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/smoke/obby.rbxlx");
    let mut session = Session::new(crate::load::read_place(&path).unwrap()).unwrap();
    session.start();
    session.join_test_players(1, None).unwrap();
    let mut host = Host::default();
    let mut viewer = Viewer::default();
    const GUEST: PeerId = 0xfeed;
    let mut up: Vec<Vec<u8>> = vec![encode_up(&Up::Ask {
        name: "Guest".into(),
        user_id: 4242,
        template: None,
    })];
    let mut down: Vec<Vec<u8>> = Vec::new();
    let (mut up_bytes, mut first, mut walking) = (0usize, 0usize, 0usize);
    let mut welcomed = None;
    let mut start = None;
    const FRAMES: u32 = 240;
    const WALK_FROM: u32 = 60;
    const WALK_TO: u32 = 200;
    for frame in 0..FRAMES {
        // The guest's side: what arrived, then this frame's input.
        for bytes in down.drain(..) {
            match decode_down(&bytes).unwrap() {
                Down::Welcome { player, name } => {
                    assert_eq!(name, "Guest");
                    welcomed = Some(player);
                }
                Down::Frame(f) => {
                    if first == 0 {
                        first = bytes.len();
                    } else if (WALK_FROM..WALK_TO).contains(&frame) {
                        walking += bytes.len();
                    }
                    viewer.apply(&f);
                }
                other => panic!("{other:?}"),
            }
        }
        if welcomed.is_some() {
            let mut actions = vec![Action::Input(Input::Move {
                position: [400.0, 300.0],
                delta: [1.0, 0.0],
            })];
            match frame {
                30 => actions.push(Action::Input(Input::Key {
                    code: "W".into(),
                    down: true,
                })),
                200 => actions.push(Action::Input(Input::Key {
                    code: "W".into(),
                    down: false,
                })),
                _ => {}
            }
            up.push(encode_up(&Up::Actions(actions)));
        }
        // The host's side.
        for bytes in up.drain(..) {
            up_bytes += bytes.len();
            match host.receive(&mut session, GUEST, decode_up(&bytes).unwrap()) {
                Some(Event::Offered(name)) => {
                    assert_eq!(name, "Guest");
                    let (_, welcome) = host.accept(&mut session, GUEST).unwrap();
                    down.push(encode_down(&welcome));
                }
                None => {}
                other => panic!("{other:?}"),
            }
        }
        session.step();
        let Some(player) = welcomed.or_else(|| host.guests.first().map(|g| g.player)) else {
            continue;
        };
        let i = session
            .clients
            .iter()
            .position(|c| c.player == player)
            .unwrap();
        let changes = session.clients[i].sim.take_view_changes();
        if frame == 31 {
            start = root_part(&session.server.dom(), "Guest");
        }
        let sim = &mut session.clients[i].sim;
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
        if let Some((to, message)) = host.frame(player, shot, view, false) {
            assert_eq!(to, GUEST);
            down.push(encode_down(&message));
        }
    }
    for bytes in down.drain(..) {
        if let Down::Frame(f) = decode_down(&bytes).unwrap() {
            viewer.apply(&f);
        }
    }
    let player = welcomed.expect("welcomed");
    let i = session
        .clients
        .iter()
        .position(|c| c.player == player)
        .unwrap();
    assert_eq!(session.clients[i].name, "Guest");

    // Their keys walked their character on the host.
    let (from, to) = (
        start.unwrap(),
        root_part(&session.server.dom(), "Guest").unwrap(),
    );
    let walked = ((to.x - from.x).powi(2) + (to.z - from.z).powi(2)).sqrt();
    assert!(walked > 10.0, "walked {walked} studs");
    // Their copy is their client's world, where everything is.
    assert_eq!(
        shown(&viewer.dom_rc().borrow()),
        shown(&session.clients[i].sim.dom())
    );
    let seconds = f64::from(FRAMES) / crate::FRAME_HZ;
    let down_rate = walking as f64 / (f64::from(WALK_TO - WALK_FROM) / crate::FRAME_HZ);
    let up_rate = up_bytes as f64 / seconds;
    eprintln!(
        "remote player at 60 Hz: first frame {first} B, then {:.1} KB/s down while walking, {:.2} KB/s up",
        down_rate / 1000.0,
        up_rate / 1000.0
    );
    assert!(up_rate < 4000.0, "input stays small: {up_rate} B/s");
    assert!(down_rate < 64_000.0, "frames stay small: {down_rate} B/s");

    // They leave: their player goes.
    let left = host.receive(
        &mut session,
        GUEST,
        decode_up(&encode_up(&Up::Leave)).unwrap(),
    );
    assert_eq!(left, Some(Event::Left(player)));
    session.step();
    assert!(session.clients.iter().all(|c| c.player != player));
    assert!(host.guests.is_empty());

    // Another asks, is turned down; a third waits as the host ends.
    host.receive(
        &mut session,
        2,
        Up::Ask {
            name: "B".into(),
            user_id: 2,
            template: None,
        },
    );
    assert!(matches!(host.decline(2), Down::Ended(_)));
    host.receive(
        &mut session,
        3,
        Up::Ask {
            name: "C".into(),
            user_id: 3,
            template: None,
        },
    );
    assert_eq!(host.end(), vec![3]);
}

/// A remote player can't run the session: only their own window's
/// actions reach their client.
#[test]
fn a_remote_player_cannot_start_join_or_shape_the_network() {
    assert!(allowed(&Action::Reset));
    assert!(!allowed(&Action::Start));
    assert!(!allowed(&Action::Leave));
    assert!(!allowed(&Action::Purchase {
        nth: 0,
        bought: true
    }));
    assert!(!allowed(&Action::Join {
        name: "x".into(),
        user_id: 1,
        template: None
    }));
}

fn obby() -> Session {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/smoke/obby.rbxlx");
    let mut session = Session::new(crate::load::read_place(&path).unwrap()).unwrap();
    session.start();
    session
}

/// A remote player let into `session` wearing `template`: their player.
fn let_in(session: &mut Session, host: &mut Host, from: PeerId, template: Option<&WeakDom>) -> Ref {
    let ask = Up::Ask {
        name: format!("Guest{from}"),
        user_id: 4000 + from as i64,
        template: template.map(|t| rbx_binary::serialize(t).unwrap()),
    };
    host.receive(session, from, decode_up(&encode_up(&ask)).unwrap());
    let (_, welcome) = host.accept(session, from).unwrap();
    let Down::Welcome { player, .. } = welcome else {
        panic!("welcomed")
    };
    player
}

fn client_of(session: &Session, player: Ref) -> usize {
    session
        .clients
        .iter()
        .position(|c| c.player == player)
        .unwrap()
}

/// One step of the session and `player`'s frame, through the wire.
fn step(session: &mut Session, host: &mut Host, player: Ref, slow: bool) -> Option<Frame> {
    session.step();
    let i = client_of(session, player);
    let sim = &mut session.clients[i].sim;
    let changes = sim.take_view_changes();
    let view = Frame {
        camera: sim.camera(),
        ..Frame::default()
    };
    let dom = sim.dom();
    let shot = Shot {
        dom: &dom,
        changes: &changes,
        sounds: sim.sound_clocks(),
        time: sim.time(),
    };
    let (_, down) = host.frame(player, shot, view, slow)?;
    match decode_down(&encode_down(&down)).unwrap() {
        Down::Frame(f) => Some(*f),
        other => panic!("{other:?}"),
    }
}

fn character_parts(dom: &WeakDom, player: Ref) -> Vec<(String, CFrameData)> {
    let Some(Variant::Ref(model)) = dom
        .get(player)
        .and_then(|p| p.properties().get("Character"))
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stack = vec![*model];
    while let Some(at) = stack.pop() {
        let Some(i) = dom.get(at) else { continue };
        stack.extend(i.children().iter().copied());
        if let Some(c) = cframe_of(dom, at) {
            out.push((format!("{} {}", at.value(), i.name()), c));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// The noob wearing a hat: a handle welded above its head, as a
/// catalog accessory hangs.
fn hatted() -> WeakDom {
    let mut dom = WeakDom::new();
    let model = crate::character::noob(&mut dom, "Hatted", None);
    let head = dom
        .get(model)
        .unwrap()
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).unwrap().name() == "Head")
        .unwrap();
    let hat = dom.new_instance("Accessory", "Hair", Some(model));
    let handle = dom.new_instance("Part", "Handle", Some(hat));
    let like_head: Vec<(String, Variant)> = dom
        .get(head)
        .unwrap()
        .properties()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (key, value) in like_head {
        let _ = dom.set_property(handle, &key, value);
    }
    let up = |y: f32| CFrameData {
        position: Vector3Data { x: 0.0, y, z: 0.0 },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    };
    for (key, value) in [
        ("CFrame", Variant::CFrame(up(5.5))),
        (
            "size",
            Variant::Vector3(Vector3Data {
                x: 2.0,
                y: 1.0,
                z: 2.0,
            }),
        ),
        ("CanCollide", Variant::Bool(false)),
        ("Massless", Variant::Bool(true)),
    ] {
        let _ = dom.set_property(handle, key, value);
    }
    // As a live server holds a catalog accessory: a rigid constraint
    // between the handle's attachment and the head's.
    let mine = dom.new_instance("Attachment", "HatAttachment", Some(handle));
    let _ = dom.set_property(mine, "CFrame", Variant::CFrame(up(-0.5)));
    let theirs = dom.new_instance("Attachment", "HatAttachment", Some(head));
    let _ = dom.set_property(theirs, "CFrame", Variant::CFrame(up(0.5)));
    let rigid = dom.new_instance("RigidConstraint", "AccessoryRigidConstraint", Some(handle));
    let _ = dom.set_property(rigid, "Attachment0", Variant::Ref(mine));
    let _ = dom.set_property(rigid, "Attachment1", Variant::Ref(theirs));
    dom.take_changes();
    dom
}

/// The hair the collaborator's camera sat in: an accessory's handle (and
/// every limb) is where the animations lay it out on the host, standing
/// still as well as walking, not where it was when the frame added it.
#[test]
fn a_remote_players_accessories_and_limbs_follow_their_animations() {
    let mut session = obby();
    let mut host = Host::default();
    let player = let_in(&mut session, &mut host, 7, Some(&hatted()));
    let mut viewer = Viewer::default();
    for frame in 0..300 {
        if frame == 120 || frame == 160 {
            let i = client_of(&session, player);
            session.act(
                Peer::Client(i),
                Action::Input(Input::Key {
                    code: "W".into(),
                    down: frame == 120,
                }),
            );
        }
        if let Some(f) = step(&mut session, &mut host, player, false) {
            viewer.apply(&f);
        }
    }
    let i = client_of(&session, player);
    let host_parts = character_parts(&session.clients[i].sim.dom(), player);
    assert!(host_parts.iter().any(|(n, _)| n.ends_with("Handle")));
    let seen = character_parts(&viewer.dom_rc().borrow(), player);
    assert_eq!(seen.len(), host_parts.len());
    for ((name, theirs), (_, ours)) in seen.iter().zip(&host_parts) {
        assert!(
            length(sub(vec3(theirs.position), vec3(ours.position))) < 1e-3
                && theirs
                    .rotation
                    .iter()
                    .zip(ours.rotation)
                    .all(|(a, b)| (a - b).abs() < 1e-3),
            "{name}: {theirs:?} on the player's copy, {ours:?} on the host"
        );
    }
}

/// A slow link's frames, held back and merged, build the same tree as
/// every frame would, in fewer bytes.
#[test]
fn slow_links_get_fewer_merged_frames_that_build_the_same_tree() {
    let mut session = obby();
    let mut host = Host::default();
    let player = let_in(&mut session, &mut host, 8, None);
    let mut every = Encoder::default();
    let (mut all, mut thin) = (Viewer::default(), Viewer::default());
    let (mut all_bytes, mut thin_bytes, mut sent) = (0, 0, 0);
    for frame in 0..240u32 {
        if frame == 30 || frame == 200 {
            let i = client_of(&session, player);
            session.act(
                Peer::Client(i),
                Action::Input(Input::Key {
                    code: "W".into(),
                    down: frame == 30,
                }),
            );
        }
        session.step();
        let i = client_of(&session, player);
        let sim = &mut session.clients[i].sim;
        let changes = sim.take_view_changes();
        let dom = sim.dom();
        let one = Frame {
            reset: frame == 0,
            ops: if frame == 0 {
                every.snapshot(&dom)
            } else {
                every.ops(&dom, &changes)
            },
            ..Frame::default()
        };
        all_bytes += encode_down(&Down::Frame(Box::new(one.clone()))).len();
        all.apply(&one);
        // Behind on all but one frame in five (the first and last go).
        let slow = frame % 5 != 0 && frame != 239;
        let shot = Shot {
            dom: &dom,
            changes: &changes,
            sounds: Vec::new(),
            time: 0.0,
        };
        if let Some((_, down)) = host.frame(player, shot, Frame::default(), slow) {
            let bytes = encode_down(&down);
            thin_bytes += bytes.len();
            sent += 1;
            let Down::Frame(f) = decode_down(&bytes).unwrap() else {
                panic!()
            };
            thin.apply(&f);
        }
    }
    let i = client_of(&session, player);
    let client = shown(&session.clients[i].sim.dom());
    assert_eq!(shown(&all.dom_rc().borrow()), client);
    assert_eq!(shown(&thin.dom_rc().borrow()), client);
    assert_eq!(sent, 49);
    eprintln!("slow link: {sent} of 240 frames, {thin_bytes} B instead of {all_bytes} B");
    assert!(thin_bytes * 2 < all_bytes, "{thin_bytes} vs {all_bytes}");
}

/// Merging keeps every event and the newest state, and a reset frame
/// replaces what came before it.
#[test]
fn merged_frames_keep_their_events_and_the_newest_state() {
    let a = Frame {
        ops: vec![
            Op::Prop {
                at: Ref::new(1),
                name: "CFrame".into(),
                value: Some(Variant::Bool(false)),
            },
            Op::Remove(Ref::new(2)),
        ],
        rows: vec![(LineKind::Print, "one".into())],
        sounds: vec![(Ref::new(5), Some(0.0))],
        ack: 3,
        core: Some(CoreGui::default()),
        ..Frame::default()
    };
    let b = Frame {
        ops: vec![Op::Prop {
            at: Ref::new(1),
            name: "CFrame".into(),
            value: Some(Variant::Bool(true)),
        }],
        rows: vec![(LineKind::Print, "two".into())],
        sounds: vec![(Ref::new(5), None)],
        ack: 4,
        paused: true,
        ..Frame::default()
    };
    let merged = a.clone().merge(b.clone());
    assert_eq!(merged.ops, vec![Op::Remove(Ref::new(2)), b.ops[0].clone()]);
    assert_eq!(merged.rows.len(), 2);
    assert_eq!(merged.sounds, vec![(Ref::new(5), None)]);
    assert_eq!((merged.ack, merged.paused), (4, true));
    assert!(merged.core.is_some(), "the core GUI that changed is kept");
    let reset = Frame {
        reset: true,
        ops: vec![Op::Remove(Ref::new(9))],
        ..Frame::default()
    };
    let merged = a.merge(reset);
    assert!(merged.reset);
    assert_eq!(merged.ops, vec![Op::Remove(Ref::new(9))]);
    assert!(merged.sounds.is_empty(), "a reset names every sound itself");
}

/// Sounds cross as events: a start with its clock, nothing while the
/// clock runs as sent, its new place after a seek, a stop; the player's
/// speaker keeps the clock between them.
#[test]
fn sounds_cross_as_starts_seeks_and_stops() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let part = dom.new_instance("Part", "Box", Some(workspace));
    let sound = dom.new_instance("Sound", "Music", Some(part));
    let _ = dom.set_property(sound, "PlaybackSpeed", Variant::Float32(2.0));
    let mut encoder = Encoder::default();
    encoder.snapshot(&dom);
    assert_eq!(
        encoder.sounds(&dom, &[(sound, 0.0)], 10.0),
        vec![(sound, Some(0.0))]
    );
    assert!(encoder.sounds(&dom, &[(sound, 1.0)], 10.5).is_empty());
    assert_eq!(
        encoder.sounds(&dom, &[(sound, 30.0)], 11.0),
        vec![(sound, Some(30.0))]
    );
    assert_eq!(encoder.sounds(&dom, &[], 11.5), vec![(sound, None)]);

    let mut speaker = Speaker::new(None);
    speaker.hear(&Frame {
        sounds: vec![(sound, Some(1.0))],
        ..Frame::default()
    });
    speaker.advance(&dom, 0.5);
    assert_eq!(speaker.playing().get(&sound), Some(&2.0));
    speaker.hear(&Frame {
        sounds: vec![(sound, None)],
        ..Frame::default()
    });
    assert!(speaker.playing().is_empty());
}

/// A one-way leg of the tunnel: ordered as a WebSocket is, each message
/// `latency ± jitter` late, in whole 60 Hz frames.
struct Leg {
    queue: VecDeque<(u32, Vec<u8>)>,
    way: crate::link::Way,
    seed: u64,
}

impl Leg {
    fn new(way: crate::link::Way, seed: u64) -> Leg {
        Leg {
            queue: VecDeque::new(),
            way,
            seed,
        }
    }

    fn send(&mut self, now: u32, bytes: Vec<u8>) {
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let spread = ((self.seed >> 33) as f64 / f64::from(1u32 << 31)) * 2.0 - 1.0;
        let delay = (self.way.latency + spread * self.way.jitter).max(0.0);
        let mut at = now + (delay * crate::FRAME_HZ).round() as u32;
        if let Some((last, _)) = self.queue.back() {
            at = at.max(*last);
        }
        self.queue.push_back((at, bytes));
    }

    fn due(&mut self, now: u32) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        while self.queue.front().is_some_and(|(at, _)| *at <= now) {
            out.extend(self.queue.pop_front().map(|(_, b)| b));
        }
        out
    }
}

fn root_of(dom: &WeakDom, player: Ref) -> Option<[f64; 3]> {
    let Some(Variant::Ref(model)) = dom.get(player)?.properties().get("Character") else {
        return None;
    };
    let root = dom
        .get(*model)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == "HumanoidRootPart"))?;
    cframe_of(dom, root).map(|c| vec3(c.position))
}

/// What the player saw of their W key over `profile`'s tunnel: frames
/// from the press until their character moved on their screen, the most
/// the shown root strayed from the host's, and how far apart they ended.
fn input_lag(profile: &str, predict: bool) -> (u32, f64, f64) {
    let network = crate::integration::profile(profile).unwrap();
    let mut session = obby();
    let mut host = Host::default();
    let player = let_in(&mut session, &mut host, 9, None);
    let (mut up, mut down) = (Leg::new(network.up, 1), Leg::new(network.down, 2));
    let mut viewer = Viewer::default();
    viewer.predictor.off = !predict;
    const PRESS: u32 = 120;
    const RELEASE: u32 = 240;
    let (mut start, mut lag, mut stray) = (None, None, 0.0f64);
    let mut camera = None;
    let mut last = [0.0; 3];
    for now in 0..420u32 {
        for bytes in down.due(now) {
            let Down::Frame(f) = decode_down(&bytes).unwrap() else {
                panic!()
            };
            viewer.apply(&f);
            camera = f.camera;
        }
        let mut actions = Vec::new();
        if now == PRESS || now == RELEASE {
            actions.push(Action::Input(Input::Key {
                code: "W".into(),
                down: now == PRESS,
            }));
        }
        viewer.input(&actions);
        viewer.predict(player, camera, 1.0 / crate::FRAME_HZ);
        let seen = root_of(&viewer.dom_rc().borrow(), player).unwrap_or_default();
        if now == PRESS {
            start = Some(seen);
        }
        if let (Some(from), None) = (start, lag) {
            if (seen[0] - from[0]).hypot(seen[2] - from[2]) > 0.1 {
                lag = Some(now - PRESS);
            }
        }
        if !actions.is_empty() || viewer.predictor.moving() {
            up.send(now, encode_up(&Up::Actions(actions)));
            viewer.predictor.sent();
        }
        for bytes in up.due(now) {
            host.receive(&mut session, 9, decode_up(&bytes).unwrap());
        }
        if let Some(f) = step(&mut session, &mut host, player, false) {
            down.send(now, encode_down(&Down::Frame(Box::new(f))));
        }
        let i = client_of(&session, player);
        let truth = root_of(&session.clients[i].sim.dom(), player).unwrap_or_default();
        if now > PRESS {
            stray = stray.max(length(sub(seen, truth)));
        }
        last = sub(seen, truth);
    }
    (lag.expect("the character moved"), stray, length(last))
}

/// Client-side prediction over the Network Simulator's profiles: input
/// to motion on the player's screen before (frames only: the tunnel's
/// round trip and a frame) and after (their next frame), and the shown
/// character ending where the host has it.
#[test]
fn prediction_moves_the_character_at_once_and_settles_on_the_host() {
    for profile in ["fiber", "4g", "3g"] {
        let (before, _, before_end) = input_lag(profile, false);
        let (after, stray, end) = input_lag(profile, true);
        let ms = |frames: u32| f64::from(frames) * 1000.0 / crate::FRAME_HZ;
        eprintln!(
            "{profile}: input to motion {:.0} ms before, {:.0} ms with prediction \
             (shown root at most {stray:.2} studs off the host's, {end:.3} apart at rest)",
            ms(before),
            ms(after)
        );
        assert!(
            after <= 1,
            "{profile}: moved {after} frames after the press"
        );
        assert!(
            before > after,
            "{profile}: {before} frames without prediction"
        );
        assert!(
            end < 0.05 && before_end < 0.05,
            "{profile}: ends {end} studs apart"
        );
    }
}

/// A teleport carries the remote players along: out of the old session
/// unended, into the new one with the avatar they came with, welcomed
/// again and sent the whole tree.
#[test]
fn a_teleport_carries_remote_players_to_the_next_session() {
    let mut session = obby();
    let mut host = Host::default();
    let player = let_in(&mut session, &mut host, 10, Some(&hatted()));
    step(&mut session, &mut host, player, false);
    let carried = host.carry();
    assert!(
        host.guests.is_empty() && host.end().is_empty(),
        "nobody is ended"
    );
    assert_eq!(carried.len(), 1);
    let mut next = obby();
    let arrived = host.arrive(&mut next, carried);
    let [(
        10,
        Down::Welcome {
            player: again,
            name,
        },
    )] = arrived.as_slice()
    else {
        panic!("{arrived:?}")
    };
    assert_eq!(name, "Guest10");
    let mut viewer = Viewer::default();
    let mut first = None;
    for _ in 0..60 {
        if let Some(f) = step(&mut next, &mut host, *again, false) {
            first.get_or_insert(f.reset);
            viewer.apply(&f);
        }
    }
    assert_eq!(first, Some(true));
    let parts = character_parts(&viewer.dom_rc().borrow(), *again);
    assert!(
        parts.iter().any(|(n, _)| n.ends_with("Handle")),
        "they wear what they came with"
    );
}
