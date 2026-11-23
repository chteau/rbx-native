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
        if let Some((to, message)) = host.frame(player, &dom, &changes, view) {
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
