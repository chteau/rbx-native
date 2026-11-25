//! Sounds at run time, on the server and on each client alike (the
//! SoundPlayback golden: a real server's sounds load, keep time and end
//! too). A sound's asset comes from the asset cache, else the app's
//! fetcher on a thread of its own, and is decoded once: `IsLoaded` and
//! `TimeLength` read it, `Loaded` fires when it arrives. A playing sound's
//! clock (`TimePosition`) runs at `PlaybackSpeed` once a frame; at the
//! end a looped one wraps and fires `DidLoop`, any other fires `Ended`
//! (still `Playing` in the handler), rewinds to 0 and stops.
//!
//! `Play`, `Stop`, `Pause` and `Resume` fire their signal before they
//! return. A client playing or stopping a sound tells the server, which
//! everyone then sees, unless `SoundService.RespectFilteringEnabled` is on
//! (ClientReplication golden). [`Sounds::voices`] is what the editor's
//! audio output mixes: each playing sound at its volume after its
//! SoundGroups and its distance from the listener.

use std::collections::{BTreeSet, HashMap};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use rbx_animation::import::asset::content_id;
use rbx_assets::{AssetCache, AssetFetcher};
use rbx_dom::{CFrameData, Content, Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, CallReply, Event};
use rbx_reflection::ReflectionDatabase;

use crate::audio::{rolloff, Clip, RollOff, Voice};
use crate::humanoid::{flag, number};

/// `Volume` above this is stored as this (SoundPlayback `clamps`).
const MAX_VOLUME: f64 = 10.0;

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    class == "Sound" && matches!(key, "Play" | "Stop" | "Pause" | "Resume")
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    class == "Sound"
        && matches!(
            key,
            "Played" | "Stopped" | "Paused" | "Resumed" | "Ended" | "Loaded" | "DidLoop"
        )
}

enum Load {
    Pending(Receiver<Result<Clip, String>>),
    Ready(Arc<Clip>),
    Failed,
}

#[derive(Default)]
struct Track {
    position: f64,
    loops: u32,
    /// A frame has looked at it.
    seen: bool,
    /// `Loaded` has fired, or the sound was loaded when first seen (a real
    /// server's cached sound is loaded the moment it's parented and never
    /// fires it: SoundPlayback `loaded_event`).
    announced: bool,
}

/// Every sound the simulation has seen, their assets and their clocks.
#[derive(Default)]
pub(crate) struct Sounds {
    pub fetcher: Option<Arc<dyn AssetFetcher>>,
    /// Record or replay: when loads land.
    pub(crate) gate: Option<crate::replay::Gate>,
    /// The warnings a client prints for sounds that failed to load, for
    /// the Output.
    pub failures: Vec<String>,
    clips: HashMap<u64, Load>,
    tracks: HashMap<Ref, Track>,
    known: BTreeSet<Ref>,
    /// What each `AudioRecorder` last recorded.
    pub(crate) recordings: HashMap<Ref, Arc<Clip>>,
}

fn sound_id(dom: &WeakDom, sound: Ref) -> String {
    match dom.get(sound).and_then(|i| i.properties().get("SoundId")) {
        Some(Variant::Content(Content::Uri(s))) | Some(Variant::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn fetch(id: u64, fetcher: Option<Arc<dyn AssetFetcher>>) -> Result<Clip, String> {
    let fetcher = fetcher.ok_or_else(|| format!("sound {id} is not cached and we are offline"))?;
    let bytes = fetcher.fetch_id(id).map_err(|e| e.to_string())?;
    if let Ok(cache) = AssetCache::new(None) {
        let _ = cache.put_id(id, &bytes);
    }
    crate::audio::decode(bytes)
}

impl Sounds {
    /// Starts watching `sound` (and keeps its clock from now on).
    pub(crate) fn watch(&mut self, sound: Ref) {
        self.known.insert(sound);
    }

    /// The clip of `id`, asking for it the first time: a cached asset is
    /// decoded at once (a real server has Roblox's own sounds loaded the
    /// moment they're parented), anything else is fetched in the
    /// background.
    pub(crate) fn clip(&mut self, id: u64) -> Option<Arc<Clip>> {
        if let Some(Load::Pending(receiver)) = self.clips.get(&id) {
            if let Some(done) =
                crate::replay::Gate::take(&self.gate, || format!("sound:{id}"), receiver)
            {
                if done.is_err() {
                    self.failures.push(format!(
                        "Failed to load sound rbxassetid://{id}: Unable to download sound data"
                    ));
                }
                let load = done.map_or(Load::Failed, |c| Load::Ready(Arc::new(c)));
                self.clips.insert(id, load);
            }
        }
        if !self.clips.contains_key(&id) {
            let cached = crate::replay::cached(self.fetcher.as_ref(), id);
            let load = match cached.map(crate::audio::decode) {
                Some(Ok(clip)) => Load::Ready(Arc::new(clip)),
                _ => {
                    let (send, receive) = mpsc::channel();
                    let fetcher = self.fetcher.clone();
                    std::thread::spawn(move || {
                        let _ = send.send(fetch(id, fetcher));
                    });
                    Load::Pending(receive)
                }
            };
            self.clips.insert(id, load);
        }
        match self.clips.get(&id) {
            Some(Load::Ready(clip)) => Some(clip.clone()),
            _ => None,
        }
    }

    pub(crate) fn clip_of(&mut self, dom: &WeakDom, sound: Ref) -> Option<Arc<Clip>> {
        content_id(&sound_id(dom, sound)).and_then(|id| self.clip(id))
    }

    /// `IsPlaying`, `IsLoaded`, `TimeLength` and the live `TimePosition`.
    pub(crate) fn property(
        &mut self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        sound: Ref,
        key: &str,
    ) -> Option<Arg> {
        match key {
            "IsPlaying" => Some(Arg::Bool(flag(dom, db, sound, "Playing"))),
            "IsLoaded" => Some(Arg::Bool(self.clip_of(dom, sound).is_some())),
            "TimeLength" => Some(Arg::Number(
                self.clip_of(dom, sound).map_or(0.0, |c| c.seconds()),
            )),
            "TimePosition" => self
                .tracks
                .get(&sound)
                .map(|t| Arg::Number(t.position.max(0.0))),
            _ => None,
        }
    }

    /// A script's write: `TimePosition` moves the clock, `Volume` and
    /// `PlaybackSpeed` are clamped (SoundPlayback `clamps`).
    pub(crate) fn write(&mut self, sound: Ref, key: &str, value: &Arg) -> Option<Arg> {
        let Arg::Number(n) = value else {
            return None;
        };
        match key {
            "TimePosition" => {
                self.watch(sound);
                self.tracks.entry(sound).or_default().position = n.max(0.0);
                None
            }
            "Volume" => {
                (*n > MAX_VOLUME || *n < 0.0).then(|| Arg::Number(n.clamp(0.0, MAX_VOLUME)))
            }
            "PlaybackSpeed" => (*n < 0.0).then_some(Arg::Number(0.0)),
            _ => None,
        }
    }

    /// `Sound:Play()` and the rest: the new `Playing`, and the reply firing
    /// the signal before the call returns.
    pub(crate) fn call(
        &mut self,
        dom: &mut WeakDom,
        sound: Ref,
        method: &str,
    ) -> (bool, CallReply) {
        let (playing, rewind, signal) = match method {
            "Play" => (true, true, "Played"),
            "Stop" => (false, true, "Stopped"),
            "Pause" => (false, false, "Paused"),
            _ => (true, false, "Resumed"),
        };
        self.watch(sound);
        set(dom, sound, playing, rewind);
        if rewind {
            self.tracks.entry(sound).or_default().position = 0.0;
        }
        let id = sound_id(dom, sound);
        (
            playing,
            CallReply {
                fire: vec![(sound, signal.into(), vec![Arg::String(id)])],
                ..CallReply::default()
            },
        )
    }

    /// A client played or stopped `sound`. The server's clock starts a
    /// frame behind the client's, as it would a network trip away, so the
    /// client that plays a sound is the one that hears it end and fires
    /// `Ended` (SoundPlayback `client_ended_frames`) rather than being told
    /// it stopped.
    pub(crate) fn client_played(&mut self, dom: &mut WeakDom, sound: Ref, playing: bool) {
        self.watch(sound);
        set(dom, sound, playing, playing);
        if playing {
            self.tracks.entry(sound).or_default().position = -1.0 / crate::FRAME_HZ;
        }
    }

    /// One frame of every sound: what loaded, the clocks of the playing
    /// ones, and the events to fire, then the sounds that ended (to stop
    /// once their `Ended` handlers have run).
    pub(crate) fn step(
        &mut self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        dt: f64,
    ) -> (Vec<Event>, Vec<Ref>) {
        self.known.retain(|r| dom.get(*r).is_some());
        self.tracks.retain(|r, _| dom.get(*r).is_some());
        let (mut events, mut ended) = (Vec::new(), Vec::new());
        for sound in self.known.clone() {
            let id = sound_id(dom, sound);
            let clip = self.clip_of(dom, sound);
            let track = self.tracks.entry(sound).or_default();
            let signal = |name: &str, args: Vec<Arg>| Event::Signal {
                owner: sound,
                name: name.into(),
                args,
            };
            if clip.is_some() && !track.announced {
                track.announced = true;
                if track.seen {
                    events.push(signal("Loaded", vec![Arg::String(id.clone())]));
                }
            }
            track.seen = true;
            let Some(clip) = clip else { continue };
            if !flag(dom, db, sound, "Playing") {
                continue;
            }
            let length = clip.seconds();
            track.position += dt * number(dom, db, sound, "PlaybackSpeed").max(0.0);
            if track.position < length {
                continue;
            }
            if flag(dom, db, sound, "Looped") && length > 0.0 {
                track.position %= length;
                track.loops += 1;
                events.push(signal(
                    "DidLoop",
                    vec![Arg::String(id), Arg::Number(f64::from(track.loops))],
                ));
            } else {
                track.position = 0.0;
                events.push(signal("Ended", vec![Arg::String(id)]));
                ended.push(sound);
            }
        }
        (events, ended)
    }

    /// The sounds playing now and where each one's clock is, for a remote
    /// player's editor to play them too.
    pub(crate) fn clocks(&self, dom: &WeakDom, db: &ReflectionDatabase) -> Vec<(Ref, f64)> {
        self.known
            .iter()
            .filter(|s| flag(dom, db, **s, "Playing"))
            .map(|s| (*s, self.tracks.get(s).map_or(0.0, |t| t.position.max(0.0))))
            .collect()
    }

    /// The sounds playing now as `listener` hears them (the camera; `None`
    /// hears every sound as if at its source).
    pub(crate) fn voices(
        &mut self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        listener: Option<&CFrameData>,
    ) -> Vec<Voice> {
        let clocks: Vec<(Ref, f64)> = self
            .known
            .iter()
            .filter(|s| flag(dom, db, **s, "Playing"))
            .map(|s| (*s, self.tracks.get(s).map_or(0.0, |t| t.position)))
            .collect();
        self.voices_at(dom, db, listener, &clocks)
    }

    /// `clocks`' sounds (each at its position, in seconds) as `listener`
    /// hears them: each at its volume after its SoundGroups and its
    /// distance, panned by where it is from the listener's right.
    pub(crate) fn voices_at(
        &mut self,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        listener: Option<&CFrameData>,
        clocks: &[(Ref, f64)],
    ) -> Vec<Voice> {
        let scale = service(dom, "SoundService")
            .map_or(1.0, |s| number(dom, db, s, "RolloffScale"))
            .max(0.0);
        let mut out = Vec::new();
        for &(sound, position) in clocks {
            let Some(clip) = self.clip_of(dom, sound) else {
                continue;
            };
            let mut gain = number(dom, db, sound, "Volume").clamp(0.0, MAX_VOLUME);
            gain *= group_volume(dom, db, sound);
            let mut pan = 0.0;
            if let (Some(at), Some(ear)) = (emitter(dom, db, sound), listener) {
                let offset = [
                    at[0] - f64::from(ear.position.x),
                    at[1] - f64::from(ear.position.y),
                    at[2] - f64::from(ear.position.z),
                ];
                let distance = offset.iter().map(|v| v * v).sum::<f64>().sqrt();
                let mode = RollOff::from_value(enum_of(dom, db, sound, "RollOffMode"));
                gain *= rolloff(
                    mode,
                    distance * scale,
                    number(dom, db, sound, "RollOffMinDistance"),
                    number(dom, db, sound, "RollOffMaxDistance"),
                );
                // The camera's right is its rotation's first column.
                let r = ear.rotation;
                let right = [f64::from(r[0]), f64::from(r[3]), f64::from(r[6])];
                if distance > 1e-6 {
                    let side = (offset[0] * right[0] + offset[1] * right[1] + offset[2] * right[2])
                        / distance;
                    pan = side.clamp(-1.0, 1.0) as f32;
                }
            }
            out.push(Voice {
                sound,
                clip,
                position,
                speed: number(dom, db, sound, "PlaybackSpeed").max(0.0),
                gain: gain as f32,
                pan,
                looped: flag(dom, db, sound, "Looped"),
            });
        }
        out
    }
}

fn service(dom: &WeakDom, class: &str) -> Option<Ref> {
    dom.root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class))
}

pub(crate) fn enum_of(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> u32 {
    match dom.get(r).and_then(|i| db.stored_or_default(i, name)) {
        Some((_, Variant::Enum(v))) => *v,
        _ => 0,
    }
}

/// The product of the volumes of the sound's `SoundGroup` and the groups
/// it sits in.
fn group_volume(dom: &WeakDom, db: &ReflectionDatabase, sound: Ref) -> f64 {
    let mut group = match dom
        .get(sound)
        .and_then(|i| i.properties().get("SoundGroup"))
    {
        Some(Variant::Ref(r)) if dom.get(*r).is_some() => Some(*r),
        _ => None,
    };
    let mut volume = 1.0;
    while let Some(g) = group.filter(|g| dom.get(*g).is_some_and(|i| i.class() == "SoundGroup")) {
        volume *= number(dom, db, g, "Volume").max(0.0);
        group = dom.parent(g);
    }
    volume
}

/// Where a sound parented to a part or an attachment is; `None` for one
/// heard everywhere alike.
fn emitter(dom: &WeakDom, db: &ReflectionDatabase, sound: Ref) -> Option<[f64; 3]> {
    let p = world_cframe(dom, db, dom.parent(sound)?)?.position;
    Some([f64::from(p.x), f64::from(p.y), f64::from(p.z)])
}

/// Where a part, a camera or an attachment is in the world.
pub(crate) fn world_cframe(dom: &WeakDom, db: &ReflectionDatabase, at: Ref) -> Option<CFrameData> {
    let class = dom.get(at)?.class();
    let cframe = |r: Ref| match dom.get(r).and_then(|i| db.stored_or_default(i, "CFrame")) {
        Some((_, Variant::CFrame(cf))) => Some(*cf),
        _ => None,
    };
    if db.is_subclass_of(class, "BasePart") || class == "Camera" {
        return cframe(at);
    }
    if db.is_subclass_of(class, "Attachment") {
        let part = cframe(dom.parent(at)?)?;
        return Some(crate::humanoid::mul(&part, &cframe(at)?));
    }
    None
}

/// Whether a client's playback reaches the server.
/// Where sounds are heard from, as `SoundService:SetListener` left it:
/// the `camera`, a `CFrame`, or an object (at its position, facing as
/// the camera does, or with its whole `CFrame`).
pub(crate) fn listener(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    camera: Option<CFrameData>,
) -> Option<CFrameData> {
    let Some(service) = service(dom, "SoundService") else {
        return camera;
    };
    let object = || match dom.get(service)?.properties().get("ListenerObject") {
        Some(Variant::Ref(r)) => world_cframe(dom, db, *r),
        _ => None,
    };
    match enum_of(dom, db, service, "ListenerType") {
        1 => match dom.get(service)?.properties().get("ListenerCFrame") {
            Some(Variant::CFrame(c)) => Some(*c),
            _ => camera,
        },
        2 => object().map(|o| CFrameData {
            position: o.position,
            rotation: camera.map_or(o.rotation, |c| c.rotation),
        }),
        3 => object(),
        _ => camera,
    }
    .or(camera)
}

pub(crate) fn replicates(dom: &WeakDom, db: &ReflectionDatabase) -> bool {
    !service(dom, "SoundService").is_some_and(|s| flag(dom, db, s, "RespectFilteringEnabled"))
}

/// Writes the playback state, logged so it replicates.
pub(crate) fn set(dom: &mut WeakDom, referent: Ref, playing: bool, rewind: bool) {
    let _ = dom.set_property(referent, "Playing", Variant::Bool(playing));
    if rewind {
        let _ = dom.set_property(referent, "TimePosition", Variant::Float64(0.0));
    }
}
