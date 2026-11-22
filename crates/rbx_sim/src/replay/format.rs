//! The recording file: a fixed header (magic, format version, engine
//! version, place hash), then the rest as LZ4-compressed little-endian
//! fields with LEB128 lengths.

use std::collections::BTreeMap;

use rbx_lua::tape::{Answer, Reel};

use super::{Action, FrameHash, Logged, PeerHash, Recording, ServicesSnapshot};
use crate::input::{Input, Phase, Sensor};
use crate::link::{Network, Way};
use crate::Peer;

const MAGIC: &[u8; 8] = b"RBXNREPL";
/// Bumped whenever the layout changes; older files are refused.
pub const FORMAT: u16 = 1;

#[derive(Default)]
struct Out(Vec<u8>);

impl Out {
    fn u(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.0.push(byte);
                return;
            }
            self.0.push(byte | 0x80);
        }
    }
    fn i(&mut self, v: i64) {
        self.u(((v << 1) ^ (v >> 63)) as u64);
    }
    fn b(&mut self, v: bool) {
        self.0.push(v as u8);
    }
    fn f32(&mut self, v: f32) {
        self.0.extend(v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend(v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend(v.to_le_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.u(v.len() as u64);
        self.0.extend_from_slice(v);
    }
    fn s(&mut self, v: &str) {
        self.bytes(v.as_bytes());
    }
    fn opt<T>(&mut self, v: Option<T>, f: impl FnOnce(&mut Self, T)) {
        self.b(v.is_some());
        if let Some(v) = v {
            f(self, v);
        }
    }
    fn res<T>(&mut self, v: &Result<T, String>, f: impl FnOnce(&mut Self, &T)) {
        match v {
            Ok(v) => {
                self.b(true);
                f(self, v);
            }
            Err(e) => {
                self.b(false);
                self.s(e);
            }
        }
    }
    fn list<T>(&mut self, v: &[T], mut f: impl FnMut(&mut Self, &T)) {
        self.u(v.len() as u64);
        for item in v {
            f(self, item);
        }
    }
}

struct In<'a>(&'a [u8]);

type R<T> = Result<T, String>;

impl<'a> In<'a> {
    fn take(&mut self, n: usize) -> R<&'a [u8]> {
        if self.0.len() < n {
            return Err("the recording is cut short".into());
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(head)
    }
    fn u(&mut self) -> R<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = self.take(1)?[0];
            v |= u64::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("a bad number in the recording".into())
    }
    fn n(&mut self) -> R<usize> {
        let n = self.u()? as usize;
        if n > self.0.len() * 8 + 64 {
            return Err("a bad length in the recording".into());
        }
        Ok(n)
    }
    fn i(&mut self) -> R<i64> {
        let v = self.u()?;
        Ok((v >> 1) as i64 ^ -((v & 1) as i64))
    }
    fn b(&mut self) -> R<bool> {
        Ok(self.take(1)?[0] != 0)
    }
    fn f32(&mut self) -> R<f32> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f64(&mut self) -> R<f64> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> R<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn bytes(&mut self) -> R<Vec<u8>> {
        let n = self.n()?;
        Ok(self.take(n)?.to_vec())
    }
    fn s(&mut self) -> R<String> {
        String::from_utf8(self.bytes()?).map_err(|_| "bad text in the recording".into())
    }
    fn opt<T>(&mut self, f: impl FnOnce(&mut Self) -> R<T>) -> R<Option<T>> {
        Ok(if self.b()? { Some(f(self)?) } else { None })
    }
    fn res<T>(&mut self, f: impl FnOnce(&mut Self) -> R<T>) -> R<Result<T, String>> {
        Ok(if self.b()? {
            Ok(f(self)?)
        } else {
            Err(self.s()?)
        })
    }
    fn list<T>(&mut self, mut f: impl FnMut(&mut Self) -> R<T>) -> R<Vec<T>> {
        let n = self.n()?;
        (0..n).map(|_| f(self)).collect()
    }
}

/// The header alone: (engine version, place hash), without decoding the
/// rest.
pub fn header(bytes: &[u8]) -> R<(String, u64)> {
    let mut r = In(bytes);
    if r.take(8)? != MAGIC {
        return Err("not a Local Run recording".into());
    }
    let format = u16::from_le_bytes(r.take(2)?.try_into().unwrap());
    if format != FORMAT {
        return Err(format!(
            "recording format {format}, this build reads {FORMAT}"
        ));
    }
    Ok((r.s()?, r.u64()?))
}

pub fn encode(rec: &Recording) -> Vec<u8> {
    let mut head = Out(MAGIC.to_vec());
    head.0.extend(FORMAT.to_le_bytes());
    head.s(&rec.engine);
    head.u64(rec.place_hash);
    let mut w = Out::default();
    w.bytes(&rec.place);
    w.u64(rec.seed);
    w.opt(rec.services.as_ref(), services);
    w.list(&rec.actions, |w, l| {
        w.u(l.frame);
        peer(w, l.peer);
        action(w, &l.action);
    });
    w.list(&rec.frames, |w, f| {
        w.list(&f.0, |w, p| {
            w.u64(p.dom);
            w.u64(p.physics);
            w.b(p.capped);
            w.list(&p.entries, |w, e| w.0.extend(e.to_le_bytes()));
        })
    });
    w.list(&rec.reels, reel);
    w.u(rec.blobs.len() as u64);
    for (key, blob) in &rec.blobs {
        w.s(key);
        w.res(blob, |w, b| w.bytes(b));
    }
    head.bytes(&lz4_flex::compress_prepend_size(&w.0));
    head.0
}

pub fn decode(bytes: &[u8]) -> R<Recording> {
    let (engine, place_hash) = header(bytes)?;
    let mut r = In(bytes);
    r.take(10)?;
    r.s()?;
    r.u64()?;
    let body = lz4_flex::decompress_size_prepended(&r.bytes()?)
        .map_err(|e| format!("the recording is damaged: {e}"))?;
    let mut r = In(&body);
    let place = r.bytes()?;
    let seed = r.u64()?;
    let services = r.opt(read_services)?;
    let actions = r.list(|r| {
        Ok(Logged {
            frame: r.u()?,
            peer: read_peer(r)?,
            action: read_action(r)?,
        })
    })?;
    let frames = r.list(|r| {
        Ok(FrameHash(r.list(|r| {
            Ok(PeerHash {
                dom: r.u64()?,
                physics: r.u64()?,
                capped: r.b()?,
                entries: r.list(|r| Ok(u32::from_le_bytes(r.take(4)?.try_into().unwrap())))?,
            })
        })?))
    })?;
    let reels = r.list(read_reel)?;
    let mut blobs = BTreeMap::new();
    for _ in 0..r.n()? {
        let key = r.s()?;
        blobs.insert(key, r.res(|r| r.bytes())?);
    }
    Ok(Recording {
        engine,
        place_hash,
        place,
        seed,
        services,
        actions,
        frames,
        reels,
        blobs,
    })
}

fn services(w: &mut Out, s: &ServicesSnapshot) {
    w.opt(s.universe.as_deref(), Out::s);
    w.b(s.api_access);
    w.i(s.place_id);
    w.i(s.game_id);
    w.list(&s.teleport_places, |w, p| w.i(*p));
    for b in s.device {
        w.b(b);
    }
    w.b(s.web_lookups);
    w.opt(s.live, Out::b);
    w.b(s.assets);
    w.list(&s.secrets, |w, (name, domain)| {
        w.s(name);
        w.s(domain);
    });
    w.opt(s.data.as_deref(), Out::bytes);
}

fn read_services(r: &mut In) -> R<ServicesSnapshot> {
    Ok(ServicesSnapshot {
        universe: r.opt(In::s)?,
        api_access: r.b()?,
        place_id: r.i()?,
        game_id: r.i()?,
        teleport_places: r.list(In::i)?,
        device: [r.b()?, r.b()?, r.b()?, r.b()?],
        web_lookups: r.b()?,
        live: r.opt(In::b)?,
        assets: r.b()?,
        secrets: r.list(|r| Ok((r.s()?, r.s()?)))?,
        data: r.opt(In::bytes)?,
    })
}

fn peer(w: &mut Out, p: Peer) {
    w.u(match p {
        Peer::Server => 0,
        Peer::Client(i) => i as u64 + 1,
    });
}

fn read_peer(r: &mut In) -> R<Peer> {
    Ok(match r.u()? {
        0 => Peer::Server,
        n => Peer::Client(n as usize - 1),
    })
}

fn way(w: &mut Out, v: &Way) {
    w.f64(v.latency);
    w.f64(v.jitter);
    w.f64(v.loss);
}

fn read_way(r: &mut In) -> R<Way> {
    Ok(Way {
        latency: r.f64()?,
        jitter: r.f64()?,
        loss: r.f64()?,
    })
}

fn action(w: &mut Out, a: &Action) {
    match a {
        Action::Start => w.u(0),
        Action::Join {
            name,
            user_id,
            template,
        } => {
            w.u(1);
            w.s(name);
            w.i(*user_id);
            w.opt(template.as_deref(), Out::bytes);
        }
        Action::Leave => w.u(2),
        Action::Input(input) => {
            w.u(3);
            self::input(w, input);
        }
        Action::Viewport(v) => {
            w.u(4);
            v.iter().for_each(|x| w.f32(*x));
        }
        Action::SafeInsets(v) => {
            w.u(5);
            v.iter().for_each(|x| w.f32(*x));
        }
        Action::MenuOpen(open) => {
            w.u(6);
            w.b(*open);
        }
        Action::Chat(text) => {
            w.u(7);
            w.s(text);
        }
        Action::Reset => w.u(8),
        Action::Purchase { nth, bought } => {
            w.u(9);
            w.u(*nth as u64);
            w.b(*bought);
        }
        Action::Network(n) => {
            w.u(10);
            way(w, &n.down);
            way(w, &n.up);
        }
    }
}

fn read_action(r: &mut In) -> R<Action> {
    Ok(match r.u()? {
        0 => Action::Start,
        1 => Action::Join {
            name: r.s()?,
            user_id: r.i()?,
            template: r.opt(In::bytes)?,
        },
        2 => Action::Leave,
        3 => Action::Input(read_input(r)?),
        4 => Action::Viewport([r.f32()?, r.f32()?]),
        5 => Action::SafeInsets([r.f32()?, r.f32()?, r.f32()?, r.f32()?]),
        6 => Action::MenuOpen(r.b()?),
        7 => Action::Chat(r.s()?),
        8 => Action::Reset,
        9 => Action::Purchase {
            nth: r.u()? as usize,
            bought: r.b()?,
        },
        10 => Action::Network(Network {
            down: read_way(r)?,
            up: read_way(r)?,
        }),
        n => return Err(format!("unknown action {n} in the recording")),
    })
}

fn input(w: &mut Out, input: &Input) {
    match input {
        Input::Key { code, down } => {
            w.u(0);
            w.s(code);
            w.b(*down);
        }
        Input::Button { button, down } => {
            w.u(1);
            w.u(u64::from(*button));
            w.b(*down);
        }
        Input::Move { position, delta } => {
            w.u(2);
            position.iter().chain(delta).for_each(|x| w.f32(*x));
        }
        Input::Wheel { notches } => {
            w.u(3);
            w.f32(*notches);
        }
        Input::Touch {
            id,
            position,
            phase,
        } => {
            w.u(4);
            w.u(u64::from(*id));
            position.iter().for_each(|x| w.f32(*x));
            w.u(*phase as u64);
        }
        Input::Gamepad { pad, connected } => {
            w.u(5);
            w.u(u64::from(*pad));
            w.b(*connected);
        }
        Input::GamepadButton { pad, code, down } => {
            w.u(6);
            w.u(u64::from(*pad));
            w.s(code);
            w.b(*down);
        }
        Input::GamepadAxis {
            pad,
            code,
            position,
        } => {
            w.u(7);
            w.u(u64::from(*pad));
            w.s(code);
            position.iter().for_each(|x| w.f32(*x));
        }
        Input::Motion { sensor, vector } => {
            w.u(8);
            w.u(*sensor as u64);
            vector.iter().for_each(|x| w.f32(*x));
        }
    }
}

fn read_input(r: &mut In) -> R<Input> {
    let small = |r: &mut In| -> R<u8> { Ok(r.u()? as u8) };
    Ok(match r.u()? {
        0 => Input::Key {
            code: r.s()?,
            down: r.b()?,
        },
        1 => Input::Button {
            button: small(r)?,
            down: r.b()?,
        },
        2 => Input::Move {
            position: [r.f32()?, r.f32()?],
            delta: [r.f32()?, r.f32()?],
        },
        3 => Input::Wheel { notches: r.f32()? },
        4 => Input::Touch {
            id: r.u()? as u32,
            position: [r.f32()?, r.f32()?],
            phase: match r.u()? {
                0 => Phase::Began,
                1 => Phase::Moved,
                _ => Phase::Ended,
            },
        },
        5 => Input::Gamepad {
            pad: small(r)?,
            connected: r.b()?,
        },
        6 => Input::GamepadButton {
            pad: small(r)?,
            code: r.s()?,
            down: r.b()?,
        },
        7 => Input::GamepadAxis {
            pad: small(r)?,
            code: r.s()?,
            position: [r.f32()?, r.f32()?, r.f32()?],
        },
        8 => Input::Motion {
            sensor: match r.u()? {
                0 => Sensor::Gravity,
                1 => Sensor::Acceleration,
                _ => Sensor::Rotation,
            },
            vector: [r.f32()?, r.f32()?, r.f32()?],
        },
        n => return Err(format!("unknown input {n} in the recording")),
    })
}

fn reel(w: &mut Out, reel: &Reel) {
    w.u(u64::from(reel.seed));
    w.list(&reel.clock, |w, v| w.f64(*v));
    w.list(&reel.entropy, |w, v| w.u64(*v));
    w.list(&reel.answers, |w, a| {
        w.u(a.poll);
        w.u(u64::from(a.id));
        w.res(&a.result, |w, v| w.s(&v.to_string()));
    });
    w.u(reel.assets.len() as u64);
    for (id, asset) in &reel.assets {
        w.u(*id);
        w.res(asset, |w, b| w.bytes(b));
    }
    w.u(reel.loads.len() as u64);
    for (key, frame) in &reel.loads {
        w.s(key);
        w.u(*frame);
    }
}

fn read_reel(r: &mut In) -> R<Reel> {
    let seed = r.u()? as u32;
    let clock = r.list(In::f64)?;
    let entropy = r.list(In::u64)?;
    let answers = r.list(|r| {
        Ok(Answer {
            poll: r.u()?,
            id: r.u()? as u32,
            result: r.res(|r| serde_json::from_str(&r.s()?).map_err(|e| e.to_string()))?,
        })
    })?;
    let mut assets = BTreeMap::new();
    for _ in 0..r.n()? {
        let id = r.u()?;
        assets.insert(id, r.res(In::bytes)?);
    }
    let mut loads = BTreeMap::new();
    for _ in 0..r.n()? {
        let key = r.s()?;
        loads.insert(key, r.u()?);
    }
    Ok(Reel {
        seed,
        clock,
        entropy,
        answers,
        assets,
        loads,
    })
}
