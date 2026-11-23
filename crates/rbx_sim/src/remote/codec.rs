//! The bytes of [`super::Up`] and [`super::Down`]: the version, the way a
//! message goes, then its fields LZ4-compressed, in the recording's
//! little-endian, LEB128-length layout. Messages are written at
//! [`VERSION`]; any earlier version still reads (a field it didn't have
//! reads as its default).

use rbx_dom::{
    Axes, CFrameData, Color3Data, ColorSequence, ColorSequenceKeypoint, Content, Faces, Font,
    NumberRange, NumberSequence, NumberSequenceKeypoint, PhysicalProperties, Rect, Ref, UDim,
    UDim2, UniqueId, Variant, Vector2Data, Vector3Data,
};
use rbx_lua::server::ChatLine;

use super::{Down, Frame, Op, Parent, Up, VERSION};
use crate::gui::{CoreGui, HotbarSlot, NameLabel, PlayerRow};
use crate::replay::format::{action, read_action, In, Out, R};
use crate::{LineKind, MouseBehavior};

/// The most a message may unpack to (an avatar or a big place's first
/// frame fits many times over).
const MOST_BYTES: usize = 64 << 20;

/// The second byte of every message: which way it goes, so an end that
/// both hosts and hears other hosts tells them apart.
const UP: u8 = b'U';
const DOWN: u8 = b'D';

fn pack(way: u8, body: Out) -> Vec<u8> {
    let mut out = vec![VERSION, way];
    out.extend(lz4_flex::compress_prepend_size(&body.0));
    out
}

/// The body and the version it was written at.
fn unpack(way: u8, bytes: &[u8]) -> R<(u8, Vec<u8>)> {
    match bytes {
        [v @ 1..=VERSION, w, rest @ ..] if *w == way => {
            let size = rest.get(..4).map_or(usize::MAX, |s| {
                u32::from_le_bytes(s.try_into().unwrap()) as usize
            });
            if size > MOST_BYTES {
                return Err("a remote play message too big".into());
            }
            lz4_flex::decompress_size_prepended(rest)
                .map(|body| (*v, body))
                .map_err(|e| e.to_string())
        }
        [1..=VERSION, ..] => Err("a remote play message going the other way".into()),
        [v, ..] => Err(format!(
            "remote play version {v}, this build speaks up to {VERSION}"
        )),
        [] => Err("an empty remote play message".into()),
    }
}

pub fn encode_up(up: &Up) -> Vec<u8> {
    let mut w = Out::default();
    match up {
        Up::Ask {
            name,
            user_id,
            template,
        } => {
            w.u(0);
            w.s(name);
            w.i(*user_id);
            w.opt(template.as_deref(), Out::bytes);
        }
        Up::Actions(actions) => {
            w.u(1);
            w.list(actions, action);
        }
        Up::Purchase(bought) => {
            w.u(2);
            w.b(*bought);
        }
        Up::Leave => w.u(3),
        Up::Resync => w.u(4),
    }
    pack(UP, w)
}

pub fn decode_up(bytes: &[u8]) -> R<Up> {
    let (_, body) = unpack(UP, bytes)?;
    let r = &mut In(&body);
    Ok(match r.u()? {
        0 => Up::Ask {
            name: r.s()?,
            user_id: r.i()?,
            template: r.opt(In::bytes)?,
        },
        1 => Up::Actions(r.list(read_action)?),
        2 => Up::Purchase(r.b()?),
        3 => Up::Leave,
        4 => Up::Resync,
        n => return Err(format!("unknown upstream message {n}")),
    })
}

pub fn encode_down(down: &Down) -> Vec<u8> {
    let mut w = Out::default();
    match down {
        Down::Open { host } => {
            w.u(0);
            w.s(host);
        }
        Down::Welcome { player, name } => {
            w.u(1);
            w.u(player.value().into());
            w.s(name);
        }
        Down::Frame(frame) => {
            w.u(2);
            write_frame(&mut w, frame);
        }
        Down::Ended(why) => {
            w.u(3);
            w.s(why);
        }
    }
    pack(DOWN, w)
}

pub fn decode_down(bytes: &[u8]) -> R<Down> {
    let (version, body) = unpack(DOWN, bytes)?;
    let r = &mut In(&body);
    Ok(match r.u()? {
        0 => Down::Open { host: r.s()? },
        1 => Down::Welcome {
            player: read_ref(r)?,
            name: r.s()?,
        },
        2 => Down::Frame(Box::new(read_frame(r, version)?)),
        3 => Down::Ended(r.s()?),
        n => return Err(format!("unknown downstream message {n}")),
    })
}

fn read_ref(r: &mut In) -> R<Ref> {
    u32::try_from(r.u()?)
        .map(Ref::new)
        .map_err(|_| "a bad referent".into())
}

fn write_parent(w: &mut Out, parent: Parent) {
    match parent {
        Parent::Root => w.u(0),
        Parent::Nil => w.u(1),
        Parent::Under(r) => w.u(u64::from(r.value()) + 2),
    }
}

fn read_parent(r: &mut In) -> R<Parent> {
    Ok(match r.u()? {
        0 => Parent::Root,
        1 => Parent::Nil,
        n => Parent::Under(Ref::new(u32::try_from(n - 2).map_err(|_| "a bad parent")?)),
    })
}

fn floats(w: &mut Out, fs: &[f32]) {
    fs.iter().for_each(|f| w.f32(*f));
}

fn read_floats<const N: usize>(r: &mut In) -> R<[f32; N]> {
    let mut out = [0.0; N];
    for f in &mut out {
        *f = r.f32()?;
    }
    Ok(out)
}

fn cframe(w: &mut Out, c: &CFrameData) {
    floats(w, &[c.position.x, c.position.y, c.position.z]);
    floats(w, &c.rotation);
}

fn read_cframe(r: &mut In) -> R<CFrameData> {
    let [x, y, z] = read_floats(r)?;
    Ok(CFrameData {
        position: Vector3Data { x, y, z },
        rotation: read_floats(r)?,
    })
}

fn v3(r: &mut In) -> R<Vector3Data> {
    let [x, y, z] = read_floats(r)?;
    Ok(Vector3Data { x, y, z })
}

fn v2(r: &mut In) -> R<Vector2Data> {
    let [x, y] = read_floats(r)?;
    Ok(Vector2Data { x, y })
}

fn udim(w: &mut Out, u: &UDim) {
    w.f32(u.scale);
    w.i(u.offset.into());
}

fn read_udim(r: &mut In) -> R<UDim> {
    Ok(UDim {
        scale: r.f32()?,
        offset: i32::try_from(r.i()?).map_err(|_| "a bad UDim")?,
    })
}

fn variant(w: &mut Out, value: &Variant) {
    match value {
        Variant::String(s) => {
            w.u(0);
            w.s(s);
        }
        Variant::Bool(b) => {
            w.u(1);
            w.b(*b);
        }
        Variant::Int32(n) => {
            w.u(2);
            w.i((*n).into());
        }
        Variant::Int64(n) => {
            w.u(3);
            w.i(*n);
        }
        Variant::Float32(f) => {
            w.u(4);
            w.f32(*f);
        }
        Variant::Float64(f) => {
            w.u(5);
            w.f64(*f);
        }
        Variant::BrickColor(n) => {
            w.u(6);
            w.u((*n).into());
        }
        Variant::Color3(c) => {
            w.u(7);
            floats(w, &[c.r, c.g, c.b]);
        }
        Variant::Color3uint8 { r, g, b } => {
            w.u(8);
            w.0.extend([*r, *g, *b]);
        }
        Variant::Vector2(p) => {
            w.u(9);
            floats(w, &[p.x, p.y]);
        }
        Variant::Vector3(p) => {
            w.u(10);
            floats(w, &[p.x, p.y, p.z]);
        }
        Variant::Vector3int16 { x, y, z } => {
            w.u(11);
            [x, y, z].iter().for_each(|n| w.i(i64::from(**n)));
        }
        Variant::Ray { origin, direction } => {
            w.u(12);
            floats(
                w,
                &[
                    origin.x,
                    origin.y,
                    origin.z,
                    direction.x,
                    direction.y,
                    direction.z,
                ],
            );
        }
        Variant::Faces(f) => {
            w.u(13);
            let bits = [f.front, f.bottom, f.left, f.back, f.top, f.right]
                .iter()
                .enumerate()
                .fold(0u8, |bits, (i, on)| bits | (u8::from(*on) << i));
            w.0.push(bits);
        }
        Variant::Axes(a) => {
            w.u(14);
            w.0.push(u8::from(a.x) | u8::from(a.y) << 1 | u8::from(a.z) << 2);
        }
        Variant::CFrame(c) => {
            w.u(15);
            cframe(w, c);
        }
        Variant::OptionalCFrame(c) => {
            w.u(16);
            w.opt(c.as_ref(), cframe);
        }
        Variant::Enum(n) => {
            w.u(17);
            w.u((*n).into());
        }
        Variant::Ref(r) => {
            w.u(18);
            w.u(r.value().into());
        }
        Variant::NumberSequence(s) => {
            w.u(19);
            w.list(&s.keypoints, |w, k| {
                floats(w, &[k.time, k.value, k.envelope])
            });
        }
        Variant::ColorSequence(s) => {
            w.u(20);
            w.list(&s.keypoints, |w, k| {
                floats(w, &[k.time, k.color.r, k.color.g, k.color.b, k.envelope])
            });
        }
        Variant::NumberRange(n) => {
            w.u(21);
            floats(w, &[n.min, n.max]);
        }
        Variant::Rect(n) => {
            w.u(22);
            floats(w, &[n.min.x, n.min.y, n.max.x, n.max.y]);
        }
        Variant::PhysicalProperties(p) => {
            w.u(23);
            match p {
                PhysicalProperties::Default => w.b(false),
                PhysicalProperties::Custom {
                    density,
                    friction,
                    elasticity,
                    friction_weight,
                    elasticity_weight,
                } => {
                    w.b(true);
                    floats(
                        w,
                        &[
                            *density,
                            *friction,
                            *elasticity,
                            *friction_weight,
                            *elasticity_weight,
                        ],
                    );
                }
            }
        }
        Variant::SharedString(n) => {
            w.u(24);
            w.u((*n).into());
        }
        Variant::UDim(u) => {
            w.u(25);
            udim(w, u);
        }
        Variant::UDim2(u) => {
            w.u(26);
            udim(w, &u.x);
            udim(w, &u.y);
        }
        Variant::UniqueId(u) => {
            w.u(27);
            w.u(u.index.into());
            w.u(u.time.into());
            w.i(u.random);
        }
        Variant::Font(f) => {
            w.u(28);
            w.s(&f.family);
            w.u(f.weight.into());
            w.0.push(u8::from(f.style));
            w.opt(f.cached_face_id.as_deref(), Out::s);
        }
        Variant::SecurityCapabilities(n) => {
            w.u(29);
            w.u(*n);
        }
        Variant::Content(c) => {
            w.u(30);
            match c {
                Content::Uri(uri) => w.opt(Some(uri.as_str()), Out::s),
                _ => w.opt(None::<&str>, Out::s),
            }
        }
        Variant::Unknown { type_id, raw } => {
            w.u(31);
            w.0.push(*type_id);
            w.bytes(raw);
        }
    }
}

fn small<T: TryFrom<u64>>(r: &mut In) -> R<T> {
    T::try_from(r.u()?).map_err(|_| "a number out of range".into())
}

fn byte(r: &mut In) -> R<u8> {
    Ok(r.take(1)?[0])
}

fn read_variant(r: &mut In) -> R<Variant> {
    Ok(match r.u()? {
        0 => Variant::String(r.s()?),
        1 => Variant::Bool(r.b()?),
        2 => Variant::Int32(i32::try_from(r.i()?).map_err(|_| "a bad Int32")?),
        3 => Variant::Int64(r.i()?),
        4 => Variant::Float32(r.f32()?),
        5 => Variant::Float64(r.f64()?),
        6 => Variant::BrickColor(small(r)?),
        7 => {
            let [r_, g, b] = read_floats(r)?;
            Variant::Color3(Color3Data { r: r_, g, b })
        }
        8 => {
            let [r_, g, b] = [byte(r)?, byte(r)?, byte(r)?];
            Variant::Color3uint8 { r: r_, g, b }
        }
        9 => Variant::Vector2(v2(r)?),
        10 => Variant::Vector3(v3(r)?),
        11 => {
            let mut n = [0i16; 3];
            for v in &mut n {
                *v = i16::try_from(r.i()?).map_err(|_| "a bad Vector3int16")?;
            }
            Variant::Vector3int16 {
                x: n[0],
                y: n[1],
                z: n[2],
            }
        }
        12 => Variant::Ray {
            origin: v3(r)?,
            direction: v3(r)?,
        },
        13 => Variant::Faces(Faces::from_bits(byte(r)?)),
        14 => Variant::Axes(Axes::from_bits(byte(r)?)),
        15 => Variant::CFrame(read_cframe(r)?),
        16 => Variant::OptionalCFrame(r.opt(read_cframe)?),
        17 => Variant::Enum(small(r)?),
        18 => Variant::Ref(read_ref(r)?),
        19 => Variant::NumberSequence(NumberSequence {
            keypoints: r.list(|r| {
                let [time, value, envelope] = read_floats(r)?;
                Ok(NumberSequenceKeypoint {
                    time,
                    value,
                    envelope,
                })
            })?,
        }),
        20 => Variant::ColorSequence(ColorSequence {
            keypoints: r.list(|r| {
                let [time, r_, g, b, envelope] = read_floats(r)?;
                Ok(ColorSequenceKeypoint {
                    time,
                    color: Color3Data { r: r_, g, b },
                    envelope,
                })
            })?,
        }),
        21 => {
            let [min, max] = read_floats(r)?;
            Variant::NumberRange(NumberRange { min, max })
        }
        22 => Variant::Rect(Rect {
            min: v2(r)?,
            max: v2(r)?,
        }),
        23 => Variant::PhysicalProperties(if r.b()? {
            let [density, friction, elasticity, friction_weight, elasticity_weight] =
                read_floats(r)?;
            PhysicalProperties::Custom {
                density,
                friction,
                elasticity,
                friction_weight,
                elasticity_weight,
            }
        } else {
            PhysicalProperties::Default
        }),
        24 => Variant::SharedString(small(r)?),
        25 => Variant::UDim(read_udim(r)?),
        26 => Variant::UDim2(UDim2 {
            x: read_udim(r)?,
            y: read_udim(r)?,
        }),
        27 => Variant::UniqueId(UniqueId {
            index: small(r)?,
            time: small(r)?,
            random: r.i()?,
        }),
        28 => Variant::Font(Font {
            family: r.s()?,
            weight: small(r)?,
            style: byte(r)?.into(),
            cached_face_id: r.opt(In::s)?,
        }),
        29 => Variant::SecurityCapabilities(r.u()?),
        30 => Variant::Content(r.opt(In::s)?.map_or(Content::None, Content::Uri)),
        31 => Variant::Unknown {
            type_id: byte(r)?,
            raw: r.bytes()?,
        },
        n => return Err(format!("unknown value type {n}")),
    })
}

fn write_op(w: &mut Out, op: &Op) {
    match op {
        Op::Add {
            at,
            parent,
            class,
            name,
            props,
        } => {
            w.u(0);
            w.u(at.value().into());
            write_parent(w, *parent);
            w.s(class);
            w.s(name);
            w.list(props, |w, (name, value)| {
                w.s(name);
                variant(w, value);
            });
        }
        Op::Prop { at, name, value } => {
            w.u(1);
            w.u(at.value().into());
            w.s(name);
            w.opt(value.as_ref(), variant);
        }
        Op::Move { at, parent } => {
            w.u(2);
            w.u(at.value().into());
            write_parent(w, *parent);
        }
        Op::Remove(at) => {
            w.u(3);
            w.u(at.value().into());
        }
        Op::Class { at, class } => {
            w.u(4);
            w.u(at.value().into());
            w.s(class);
        }
    }
}

fn read_op(r: &mut In) -> R<Op> {
    Ok(match r.u()? {
        0 => Op::Add {
            at: read_ref(r)?,
            parent: read_parent(r)?,
            class: r.s()?,
            name: r.s()?,
            props: r.list(|r| Ok((r.s()?, read_variant(r)?)))?,
        },
        1 => Op::Prop {
            at: read_ref(r)?,
            name: r.s()?,
            value: r.opt(read_variant)?,
        },
        2 => Op::Move {
            at: read_ref(r)?,
            parent: read_parent(r)?,
        },
        3 => Op::Remove(read_ref(r)?),
        4 => Op::Class {
            at: read_ref(r)?,
            class: r.s()?,
        },
        n => return Err(format!("unknown op {n}")),
    })
}

fn pair(w: &mut Out, (a, b): &(String, String)) {
    w.s(a);
    w.s(b);
}

fn read_pair(r: &mut In) -> R<(String, String)> {
    Ok((r.s()?, r.s()?))
}

fn core(w: &mut Out, c: &CoreGui) {
    w.opt(c.health, |w, (h, m)| {
        w.f64(h);
        w.f64(m);
    });
    w.opt(c.hotbar.as_ref(), |w, slots| {
        w.list(slots, |w, s| {
            w.s(&s.name);
            w.s(&s.icon);
            w.b(s.held);
        })
    });
    w.opt(c.players.as_ref(), |w, rows| {
        w.list(rows, |w, p| {
            w.i(p.user_id);
            w.s(&p.name);
            w.s(&p.display_name);
            w.opt(p.team, |w, t| w.0.extend(t));
            w.list(&p.stats, |w, s| w.s(s));
        })
    });
    w.list(&c.stats, |w, s| w.s(s));
    w.list(&c.notifications, pair);
    w.b(c.can_reset);
    w.list(&c.heads, |w, (id, at)| {
        w.i(*id);
        floats(w, at);
    });
    w.list(&c.labels, |w, l| {
        w.s(&l.text);
        floats(w, &l.at);
        w.opt(l.health, Out::f32);
    });
    w.b(c.chat);
    w.b(c.chat_bar);
    w.b(c.topbar);
}

fn read_core(r: &mut In) -> R<CoreGui> {
    Ok(CoreGui {
        health: r.opt(|r| Ok((r.f64()?, r.f64()?)))?,
        hotbar: r.opt(|r| {
            r.list(|r| {
                Ok(HotbarSlot {
                    name: r.s()?,
                    icon: r.s()?,
                    held: r.b()?,
                })
            })
        })?,
        players: r.opt(|r| {
            r.list(|r| {
                Ok(PlayerRow {
                    user_id: r.i()?,
                    name: r.s()?,
                    display_name: r.s()?,
                    team: r.opt(|r| Ok([byte(r)?, byte(r)?, byte(r)?]))?,
                    stats: r.list(In::s)?,
                })
            })
        })?,
        stats: r.list(In::s)?,
        notifications: r.list(read_pair)?,
        can_reset: r.b()?,
        heads: r.list(|r| Ok((r.i()?, read_floats(r)?)))?,
        labels: r.list(|r| {
            Ok(NameLabel {
                text: r.s()?,
                at: read_floats(r)?,
                health: r.opt(In::f32)?,
            })
        })?,
        chat: r.b()?,
        chat_bar: r.b()?,
        topbar: r.b()?,
    })
}

fn line_kind(kind: LineKind) -> u64 {
    match kind {
        LineKind::Print => 0,
        LineKind::Warn => 1,
        LineKind::Error => 2,
    }
}

fn write_frame(w: &mut Out, f: &Frame) {
    w.b(f.reset);
    w.list(&f.ops, write_op);
    w.opt(f.camera.as_ref(), |w, (c, fov)| {
        cframe(w, c);
        w.f32(*fov);
    });
    w.u(match f.mouse {
        MouseBehavior::Default => 0,
        MouseBehavior::LockCenter => 1,
        MouseBehavior::LockCurrentPosition => 2,
    });
    w.opt(f.player_gui, |w, r| w.u(r.value().into()));
    w.opt(f.core.as_ref(), core);
    w.list(&f.chat, |w, c| {
        w.s(&c.prefix);
        w.s(&c.text);
        w.s(&c.channel);
        w.i(c.user_id);
    });
    w.list(&f.notices, pair);
    w.opt(f.dialog.as_ref(), pair);
    w.list(&f.bursts, |w, (r, n)| {
        w.u(r.value().into());
        w.opt(*n, |w, n| w.u(n.into()));
    });
    w.list(&f.rows, |w, (kind, text)| {
        w.u(line_kind(*kind));
        w.s(text);
    });
    w.b(f.paused);
    w.list(&f.sounds, |w, (sound, at)| {
        w.u(sound.value().into());
        w.opt(*at, Out::f32);
    });
    w.u(f.ack.into());
}

fn read_frame(r: &mut In, version: u8) -> R<Frame> {
    let mut frame = Frame {
        reset: r.b()?,
        ops: r.list(read_op)?,
        camera: r.opt(|r| Ok((read_cframe(r)?, r.f32()?)))?,
        mouse: match r.u()? {
            1 => MouseBehavior::LockCenter,
            2 => MouseBehavior::LockCurrentPosition,
            _ => MouseBehavior::Default,
        },
        player_gui: r.opt(read_ref)?,
        core: r.opt(read_core)?,
        chat: r.list(|r| {
            Ok(ChatLine {
                prefix: r.s()?,
                text: r.s()?,
                channel: r.s()?,
                user_id: r.i()?,
            })
        })?,
        notices: r.list(read_pair)?,
        dialog: r.opt(read_pair)?,
        bursts: r.list(|r| Ok((read_ref(r)?, r.opt(small)?)))?,
        rows: r.list(|r| {
            let kind = match r.u()? {
                1 => LineKind::Warn,
                2 => LineKind::Error,
                _ => LineKind::Print,
            };
            Ok((kind, r.s()?))
        })?,
        paused: r.b()?,
        ..Frame::default()
    };
    if version >= 2 {
        frame.sounds = r.list(|r| Ok((read_ref(r)?, r.opt(In::f32)?)))?;
        frame.ack = small(r)?;
    }
    Ok(frame)
}
