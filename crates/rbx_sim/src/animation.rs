//! `Animator` and `AnimationTrack` at runtime: loading animations by id
//! (the asset cache, else the app's fetcher, on a thread of their own),
//! playing them through `rbx_animation`'s blend, and writing the blended
//! pose to each `Motor6D.Transform`. Lifecycle and event order follow the
//! AnimationTrack golden: `Stopped` at once on `Stop` and `Ended` a frame
//! after the fade, a one-shot that ends fires `KeyframeReached`, `Stopped`
//! and `DidLoop` together and keeps posing its last frame.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use rbx_animation::animator::{Animator, Event as TrackEvent, TrackId};
use rbx_animation::clip::{Clip, Priority};
use rbx_animation::import::asset::{clip_from_model, content_id};
use rbx_assets::{AssetCache, AssetFetcher};
use rbx_dom::{CFrameData, Content, Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, CallReply, Event};
use rbx_reflection::ReflectionDatabase;

use crate::humanoid::{inverse, mul, reference, set};

/// `Play`, `Stop` and `AdjustWeight` fade over 0.1 s unless told otherwise
/// (AnimationTrack `weight_1`, `fade_3`, `adjust_3`).
const DEFAULT_FADE: f64 = 0.1;

struct Rig {
    animator: Animator,
}

struct TrackRef {
    animator: Ref,
    id: TrackId,
    asset: Option<u64>,
    /// `Stop` was called: the fade's end is `Ended`, not `Stopped`.
    stopping: bool,
}

/// What a script did to a track, kept for replication: the owner's tracks
/// replicate to the server and from there to every other client (the
/// PlayersCharacter golden: the server sees the client's Animate tracks).
#[derive(Clone, Debug)]
pub enum TrackOp {
    Load {
        track: Ref,
        animator: Ref,
        animation: Ref,
        id: String,
    },
    Call {
        track: Ref,
        method: String,
        args: Vec<Arg>,
    },
    Write {
        track: Ref,
        key: String,
        value: Arg,
    },
}

#[derive(Default)]
pub struct Animations {
    /// Track operations since the session last took them, with the peer
    /// they came from (`None`: made here).
    pub(crate) ops: Vec<(Option<Ref>, TrackOp)>,
    /// Whether to keep `ops` (a session's peers do).
    pub(crate) record: bool,
    /// The peer the operation being mirrored came from.
    origin: Option<Ref>,
    /// Tracks made here for another peer's: (that peer, its track) → ours.
    mirrors: HashMap<(Option<Ref>, Ref), Ref>,
    /// Inside a recorded call, so it is recorded once.
    recording_call: bool,
    rigs: HashMap<Ref, Rig>,
    tracks: HashMap<Ref, TrackRef>,
    clips: HashMap<u64, Result<Clip, String>>,
    loading: HashMap<u64, Receiver<Result<Clip, String>>>,
    pub fetcher: Option<Arc<dyn AssetFetcher>>,
    /// Record or replay: when loads land.
    pub(crate) gate: Option<crate::replay::Gate>,
    /// The warnings a client prints for animations that failed to load,
    /// for the Output.
    pub failures: Vec<String>,
    /// Tracks whose `Ended` is coming, and in how many frames.
    ended: Vec<(Ref, u32)>,
    pub events: Vec<Event>,
}

const ANIMATOR_METHODS: &[&str] = &["LoadAnimation", "GetPlayingAnimationTracks"];
const TRACK_METHODS: &[&str] = &[
    "Play",
    "Stop",
    "AdjustSpeed",
    "AdjustWeight",
    "GetTimeOfKeyframe",
];
const TRACK_SIGNALS: &[&str] = &["Stopped", "Ended", "DidLoop", "KeyframeReached"];
const TRACK_PROPERTIES: &[&str] = &[
    "Length",
    "IsPlaying",
    "Speed",
    "TimePosition",
    "WeightCurrent",
    "WeightTarget",
    "Looped",
    "Priority",
];

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    match class {
        "Animator" => ANIMATOR_METHODS.contains(&key) || key == "GetTrackByAnimationId",
        "AnimationTrack" => TRACK_METHODS.contains(&key),
        "Humanoid" | "AnimationController" => ANIMATOR_METHODS.contains(&key),
        "KeyframeSequenceProvider" => {
            matches!(
                key,
                "RegisterKeyframeSequence" | "GetKeyframeSequenceAsync#fetch"
            )
        }
        "AnimationClipProvider" => key == "GetAnimationClipAsync#fetch",
        _ => false,
    }
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    (class == "AnimationTrack" && TRACK_SIGNALS.contains(&key))
        || (class == "Animator" && key == "AnimationPlayed")
}

/// `GetMarkerReachedSignal(name)`: the signal `<method>:<name>`.
pub(crate) fn is_signal_method(class: &str, key: &str) -> bool {
    class == "AnimationTrack" && key == "GetMarkerReachedSignal"
}

fn priority_arg(p: Priority) -> Arg {
    Arg::Enum {
        enum_name: "AnimationPriority".into(),
        item: p.label().into(),
    }
}

fn signal(owner: Ref, name: &str, args: Vec<Arg>) -> (Ref, String, Vec<Arg>) {
    (owner, name.into(), args)
}

fn number(args: &[Arg], i: usize) -> Option<f64> {
    match args.get(i) {
        Some(Arg::Number(n)) => Some(*n),
        _ => None,
    }
}

fn load_clip(id: u64, fetcher: Option<Arc<dyn AssetFetcher>>) -> Result<Clip, String> {
    clip_from_model(&fetch(id, fetcher)?, &id.to_string()).map_err(|e| e.to_string())
}

/// An asset's bytes: the asset cache's, else the fetcher's, kept.
fn fetch(id: u64, fetcher: Option<Arc<dyn AssetFetcher>>) -> Result<Vec<u8>, String> {
    let cache = AssetCache::new(None).ok();
    let bytes = match crate::replay::cached(fetcher.as_ref(), id) {
        Some(bytes) => bytes,
        None => {
            let fetcher = fetcher
                .ok_or_else(|| format!("animation {id} is not cached and we are offline"))?;
            let bytes = fetcher.fetch_id(id).map_err(|e| e.to_string())?;
            if let Some(cache) = &cache {
                let _ = cache.put_id(id, &bytes);
            }
            bytes
        }
    };
    Ok(bytes)
}

impl Animations {
    fn track_of(&self, track: Ref) -> Result<&TrackRef, String> {
        self.tracks
            .get(&track)
            .ok_or_else(|| "AnimationTrack is not loaded".into())
    }

    fn inner(&self, track: Ref) -> Option<&rbx_animation::animator::Track> {
        let t = self.tracks.get(&track)?;
        self.rigs.get(&t.animator)?.animator.track(t.id)
    }

    fn inner_mut(&mut self, track: Ref) -> Option<&mut rbx_animation::animator::Track> {
        let t = self.tracks.get(&track)?;
        self.rigs.get_mut(&t.animator)?.animator.track_mut(t.id)
    }

    /// `LoadAnimation` on an Animator, or on a Humanoid or
    /// AnimationController (through its Animator, made when missing).
    fn load(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        owner: Ref,
        animation: Ref,
    ) -> Result<Ref, String> {
        let animator = match dom.get(owner).map(|i| i.class()) {
            Some("Animator") => owner,
            _ => animator_under(dom, owner)
                .unwrap_or_else(|| dom.new_instance("Animator", "Animator", Some(owner))),
        };
        let text = match dom
            .get(animation)
            .and_then(|i| i.properties().get("AnimationId"))
        {
            Some(Variant::Content(Content::Uri(uri))) => uri.clone(),
            Some(Variant::String(s)) => s.clone(),
            _ => String::new(),
        };
        let asset = content_id(&text);
        if let Some(id) = asset {
            if !self.clips.contains_key(&id) && !self.loading.contains_key(&id) {
                let (send, receive) = mpsc::channel();
                let fetcher = self.fetcher.clone();
                std::thread::spawn(move || {
                    let _ = send.send(load_clip(id, fetcher));
                });
                self.loading.insert(id, receive);
            }
        }
        // Even a cached animation has no length until the next frame
        // (AnimationTrack `length_now`).
        let clip = Clip::default();
        let rig = self.rigs.entry(animator).or_insert_with(|| Rig {
            animator: Animator::new(),
        });
        let id = rig.animator.load(clip);
        if let Some(track) = rig.animator.track_mut(id) {
            track.hold_at_end = true;
        }
        let name = dom
            .get(animation)
            .map(|i| i.name().to_owned())
            .unwrap_or_default();
        let track = dom.new_instance("AnimationTrack", &name, None);
        set(dom, db, track, "Animation", Variant::Ref(animation));
        self.tracks.insert(
            track,
            TrackRef {
                animator,
                id,
                asset,
                stopping: false,
            },
        );
        Ok(track)
    }

    pub(crate) fn call(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        referent: Ref,
        class: &str,
        method: &str,
        args: &[Arg],
    ) -> Result<CallReply, String> {
        match (class, method) {
            (_, "LoadAnimation") => {
                let Some(Arg::Instance(animation)) = args.first() else {
                    return Err("Argument 1 missing or nil".into());
                };
                if dom.get(*animation).map(|i| i.class()) != Some("Animation") {
                    return Err("Unable to cast value to Object".into());
                }
                let track = self.load(dom, db, referent, *animation)?;
                if self.record {
                    let animator = self.track_of(track)?.animator;
                    let id = animation_id(dom, *animation);
                    self.ops.push((
                        self.origin,
                        TrackOp::Load {
                            track,
                            animator,
                            animation: *animation,
                            id,
                        },
                    ));
                }
                Ok(CallReply::value(Arg::Instance(track)))
            }
            (_, "GetPlayingAnimationTracks") => {
                let animator = match class {
                    "Animator" => Some(referent),
                    _ => animator_under(dom, referent),
                };
                let mut playing: Vec<Ref> = self
                    .tracks
                    .iter()
                    .filter(|(_, t)| Some(t.animator) == animator)
                    .filter(|(r, _)| self.inner(**r).is_some_and(|t| t.playing))
                    .map(|(r, _)| *r)
                    .collect();
                playing.sort();
                Ok(CallReply::value(Arg::Instances(playing)))
            }
            // The playing track of that animation, nil when none plays
            // (AnimationApi `an_by_id`: a loaded, stopped one isn't found).
            ("Animator", "GetTrackByAnimationId") => {
                let asset = match args.first() {
                    Some(Arg::String(text)) => content_id(text),
                    _ => None,
                };
                let found = self
                    .tracks
                    .iter()
                    .filter(|(_, t)| t.animator == referent && asset.is_some() && t.asset == asset)
                    .filter(|(r, _)| self.inner(**r).is_some_and(|t| t.playing))
                    .map(|(r, _)| *r)
                    .min();
                Ok(CallReply::value(found.map_or(Arg::Nil, Arg::Instance)))
            }
            // The asset's own KeyframeSequence or CurveAnimation, unparented;
            // `animation.luau` words the failure.
            (_, "GetKeyframeSequenceAsync#fetch" | "GetAnimationClipAsync#fetch") => {
                let id = match args.first() {
                    Some(Arg::String(text)) => content_id(text),
                    Some(Arg::Number(n)) => Some(*n as u64),
                    _ => None,
                }
                .ok_or("no asset")?;
                let bytes = fetch(id, self.fetcher.clone())?;
                let model = crate::load::parse_place(&bytes)?;
                let wanted: &[&str] = if method.starts_with("GetKeyframe") {
                    &["KeyframeSequence"]
                } else {
                    &["KeyframeSequence", "CurveAnimation"]
                };
                let root = model
                    .root_refs()
                    .iter()
                    .copied()
                    .find(|r| model.get(*r).is_some_and(|i| wanted.contains(&i.class())))
                    .ok_or("not an animation")?;
                let copy = crate::players::graft(dom, &model, root, None);
                Ok(CallReply::value(Arg::Instance(copy)))
            }
            ("KeyframeSequenceProvider", "RegisterKeyframeSequence") => {
                // A running server refuses it (AnimationTrack `register`).
                Err("RegisterKeyframeSequence can only be called from Studio".into())
            }
            ("AnimationTrack", "Play" | "Stop" | "AdjustSpeed" | "AdjustWeight")
                if self.record && !self.recording_call =>
            {
                self.recording_call = true;
                let reply = self.call(dom, db, referent, class, method, args);
                self.recording_call = false;
                if reply.is_ok() {
                    self.ops.push((
                        self.origin,
                        TrackOp::Call {
                            track: referent,
                            method: method.to_owned(),
                            args: args.to_vec(),
                        },
                    ));
                }
                reply
            }
            ("AnimationTrack", "Play") => {
                let fade = number(args, 0).unwrap_or(DEFAULT_FADE) as f32;
                let weight = number(args, 1).unwrap_or(1.0) as f32;
                let speed = number(args, 2).unwrap_or(1.0) as f32;
                let animator = self.track_of(referent)?.animator;
                let id = self.track_of(referent)?.id;
                if let Some(t) = self.tracks.get_mut(&referent) {
                    t.stopping = false;
                }
                if let Some(rig) = self.rigs.get_mut(&animator) {
                    rig.animator.play(id, fade, weight, speed);
                }
                Ok(CallReply {
                    fire: vec![signal(
                        animator,
                        "AnimationPlayed",
                        vec![Arg::Instance(referent)],
                    )],
                    ..CallReply::default()
                })
            }
            ("AnimationTrack", "Stop") => {
                let fade = number(args, 0).unwrap_or(DEFAULT_FADE) as f32;
                let (animator, id) = {
                    let t = self.track_of(referent)?;
                    (t.animator, t.id)
                };
                let playing = self.inner(referent).is_some_and(|t| t.playing);
                let mut reply = CallReply::default();
                if playing {
                    let finished = self.inner(referent).is_some_and(|t| t.finished);
                    if let Some(t) = self.tracks.get_mut(&referent) {
                        t.stopping = true;
                    }
                    if let Some(rig) = self.rigs.get_mut(&animator) {
                        rig.animator.stop(id, fade);
                    }
                    if !finished {
                        reply.fire.push(signal(referent, "Stopped", Vec::new()));
                    }
                }
                Ok(reply)
            }
            ("AnimationTrack", "AdjustSpeed") => {
                let speed = number(args, 0).unwrap_or(1.0) as f32;
                let t = self.track_of(referent)?;
                let (animator, id) = (t.animator, t.id);
                if let Some(rig) = self.rigs.get_mut(&animator) {
                    rig.animator.adjust_speed(id, speed);
                }
                Ok(CallReply::default())
            }
            ("AnimationTrack", "AdjustWeight") => {
                let weight = number(args, 0).unwrap_or(1.0) as f32;
                let fade = number(args, 1).unwrap_or(DEFAULT_FADE) as f32;
                let t = self.track_of(referent)?;
                let (animator, id) = (t.animator, t.id);
                if let Some(rig) = self.rigs.get_mut(&animator) {
                    rig.animator.adjust_weight(id, weight, fade);
                }
                Ok(CallReply::default())
            }
            ("AnimationTrack", "GetTimeOfKeyframe") => {
                let Some(Arg::String(name)) = args.first() else {
                    return Err("Argument 1 missing or nil".into());
                };
                let time = self
                    .inner(referent)
                    .and_then(|t| {
                        t.clip
                            .keyframes
                            .iter()
                            .find(|k| &k.name == name)
                            .map(|k| k.time)
                    })
                    .ok_or_else(|| format!("Could not find a keyframe by that name: {name}"))?;
                Ok(CallReply::value(Arg::Number(f64::from(time))))
            }
            _ => Err(format!("{method} is not a valid member of {class}")),
        }
    }

    pub(crate) fn property(&self, track: Ref, key: &str) -> Option<Arg> {
        if !TRACK_PROPERTIES.contains(&key) {
            return None;
        }
        let stopping = self.tracks.get(&track)?.stopping;
        let t = self.inner(track)?;
        Some(match key {
            "Length" => Arg::Number(f64::from(t.length())),
            "IsPlaying" => Arg::Bool(t.playing && !t.finished && !stopping),
            "Speed" => Arg::Number(f64::from(t.speed)),
            "TimePosition" => Arg::Number(f64::from(t.time)),
            "WeightCurrent" => Arg::Number(f64::from(t.weight())),
            "WeightTarget" => Arg::Number(f64::from(t.weight_target())),
            "Looped" => Arg::Bool(t.looped),
            "Priority" => priority_arg(t.priority),
            _ => return None,
        })
    }

    pub(crate) fn write(&mut self, track: Ref, key: &str, value: &Arg) -> Option<Arg> {
        if self.record && matches!(key, "TimePosition" | "Looped" | "Priority") {
            self.ops.push((
                self.origin,
                TrackOp::Write {
                    track,
                    key: key.to_owned(),
                    value: value.clone(),
                },
            ));
        }
        let t = self.inner_mut(track)?;
        match (key, value) {
            ("TimePosition", Arg::Number(n)) => t.time = (*n as f32).clamp(0.0, t.length()),
            ("Looped", Arg::Bool(b)) => t.looped = *b,
            ("Priority", Arg::Enum { item, .. }) => {
                t.priority = Priority::ALL.into_iter().find(|p| p.label() == item)?;
            }
            _ => return None,
        }
        Some(value.clone())
    }

    /// Does here what `op` did on peer `from` (`None`: the server): a
    /// track of our own for theirs, played, stopped and adjusted the same.
    /// Signals it fires go to `events`.
    pub(crate) fn mirror(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        from: Option<Ref>,
        op: TrackOp,
    ) {
        self.origin = from;
        match op {
            TrackOp::Load {
                track,
                animator,
                animation,
                id,
            } => {
                if dom.get(animator).map(|i| i.class()) != Some("Animator") {
                    self.origin = None;
                    return;
                }
                let animation = if dom.get(animation).map(|i| i.class()) == Some("Animation") {
                    animation
                } else {
                    let made = dom.new_instance("Animation", "Animation", None);
                    dom.unparent(made);
                    set(dom, db, made, "AnimationId", Variant::String(id));
                    made
                };
                let reply = self.call(
                    dom,
                    db,
                    animator,
                    "Animator",
                    "LoadAnimation",
                    &[Arg::Instance(animation)],
                );
                if let Ok(CallReply { values, .. }) = reply {
                    if let Some(Arg::Instance(ours)) = values.first() {
                        dom.unparent(*ours);
                        self.mirrors.insert((from, track), *ours);
                    }
                }
            }
            TrackOp::Call {
                track,
                method,
                args,
            } => {
                if let Some(&ours) = self.mirrors.get(&(from, track)) {
                    if let Ok(reply) = self.call(dom, db, ours, "AnimationTrack", &method, &args) {
                        self.events.extend(
                            reply
                                .fire
                                .into_iter()
                                .map(|(owner, name, args)| Event::Signal { owner, name, args }),
                        );
                    }
                }
            }
            TrackOp::Write { track, key, value } => {
                if let Some(&ours) = self.mirrors.get(&(from, track)) {
                    self.write(ours, &key, &value);
                }
            }
        }
        self.origin = None;
    }

    /// One frame: finished loads join their tracks, every rig steps by
    /// `dt` and its pose goes to its joints' `Transform`s.
    pub fn step(&mut self, dom: &mut WeakDom, dt: f64) {
        let done: Vec<(u64, Result<Clip, String>)> = self
            .loading
            .iter()
            .filter_map(|(id, r)| {
                crate::replay::Gate::take(&self.gate, || format!("animation:{id}"), r)
                    .map(|clip| (*id, clip))
            })
            .collect();
        for (id, clip) in done {
            self.loading.remove(&id);
            if clip.is_err() {
                self.failures.push(format!(
                    "Failed to load animation with sanitized ID rbxassetid://{id}: Animation failed to load, assetId: https://assetdelivery.roblox.com/v1/asset?id={id}"
                ));
            }
            self.clips.insert(id, clip);
        }
        // Only a track still waiting on its clip takes a copy: copying every
        // track's clip each frame was most of a busy lobby's frame.
        let waiting: Vec<(Ref, Clip)> = self
            .tracks
            .iter()
            .filter(|(r, _)| self.inner(**r).is_some_and(|t| t.clip.keyframes.is_empty()))
            .filter_map(|(r, t)| Some((*r, self.clips.get(&t.asset?)?.as_ref().ok()?)))
            .filter(|(_, clip)| !clip.keyframes.is_empty())
            .map(|(r, clip)| (r, clip.clone()))
            .collect();
        for (track, clip) in waiting {
            if let Some(t) = self.inner_mut(track) {
                t.looped = clip.looped;
                t.priority = clip.priority;
                t.clip = clip;
            }
        }
        // `Ended` comes the frame after the fade-out's last (AnimationTrack
        // `stop_events`: weight 0 on the 6th frame, `ended@7`).
        let mut due = Vec::new();
        self.ended.retain_mut(|(track, frames)| {
            *frames -= 1;
            if *frames == 0 {
                due.push(*track);
            }
            *frames > 0
        });
        for track in due {
            self.events.push(Event::Signal {
                owner: track,
                name: "Ended".into(),
                args: Vec::new(),
            });
        }
        // Tracks whose animator left the place stop with it.
        self.rigs.retain(|animator, _| dom.get(*animator).is_some());
        self.tracks
            .retain(|r, t| self.rigs.contains_key(&t.animator) && dom.get(*r).is_some());
        let by_id: HashMap<(Ref, TrackId), Ref> = self
            .tracks
            .iter()
            .map(|(r, t)| ((t.animator, t.id), *r))
            .collect();
        for (animator, rig) in &mut self.rigs {
            for event in rig.animator.step(dt as f32) {
                let (id, name, args) = match event {
                    TrackEvent::KeyframeReached { track, name } => {
                        (track, "KeyframeReached".to_owned(), vec![Arg::String(name)])
                    }
                    TrackEvent::MarkerReached { track, name, value } => (
                        track,
                        format!("GetMarkerReachedSignal:{name}"),
                        vec![Arg::String(value)],
                    ),
                    TrackEvent::DidLoop(track) => (track, "DidLoop".to_owned(), Vec::new()),
                    TrackEvent::Ended(track) => (track, "Ended".to_owned(), Vec::new()),
                    TrackEvent::Stopped(track) => (track, "Stopped".to_owned(), Vec::new()),
                };
                let Some(&owner) = by_id.get(&(*animator, id)) else {
                    continue;
                };
                let stopping = self.tracks.get(&owner).is_some_and(|t| t.stopping);
                if name == "Stopped" && stopping {
                    if let Some(t) = self.tracks.get_mut(&owner) {
                        t.stopping = false;
                    }
                    self.ended.push((owner, 2));
                    continue;
                }
                self.events.push(Event::Signal { owner, name, args });
            }
            pose_rig(dom, *animator, &rig.animator);
        }
    }
}

fn animation_id(dom: &WeakDom, animation: Ref) -> String {
    match dom
        .get(animation)
        .and_then(|i| i.properties().get("AnimationId"))
    {
        Some(Variant::Content(Content::Uri(uri))) => uri.clone(),
        Some(Variant::String(s)) => s.clone(),
        _ => String::new(),
    }
}

fn animator_under(dom: &WeakDom, owner: Ref) -> Option<Ref> {
    dom.get(owner)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.class() == "Animator"))
}

/// The model an Animator animates: its Humanoid's or
/// AnimationController's parent.
fn model_of(dom: &WeakDom, animator: Ref) -> Option<Ref> {
    dom.parent(dom.parent(animator)?)
}

fn motors(dom: &WeakDom, model: Ref) -> Vec<Ref> {
    of_classes(dom, model, &["Motor6D"])
}

/// The Motor6Ds and the C0/C1 welds under `model`.
fn joints(dom: &WeakDom, model: Ref) -> Vec<Ref> {
    of_classes(
        dom,
        model,
        &["Motor6D", "Weld", "ManualWeld", "Snap", "Glue", "Motor"],
    )
}

fn of_classes(dom: &WeakDom, model: Ref, classes: &[&str]) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack = vec![model];
    while let Some(at) = stack.pop() {
        let Some(i) = dom.get(at) else { continue };
        stack.extend(i.children().iter().copied());
        if classes.contains(&i.class()) {
            out.push(at);
        }
    }
    out
}

const IDENTITY: CFrameData = CFrameData {
    position: rbx_dom::Vector3Data {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    },
    rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
};

/// Writes the blended pose to the `Transform` of each Motor6D whose `Part1`
/// a track poses. A joint no track poses keeps the last one (AnimationTrack
/// `rest_hip`: a sliver of the walk's last faded frame, not the rest pose).
/// Not logged: `Transform` changes every frame and fires no `Changed`.
fn pose_rig(dom: &mut WeakDom, animator: Ref, rig: &Animator) {
    let Some(model) = model_of(dom, animator) else {
        return;
    };
    let pose = rig.pose();
    if pose.is_empty() {
        return;
    }
    for motor in motors(dom, model) {
        let Some(part1) = reference(dom, motor, "Part1") else {
            continue;
        };
        let Some(frame) = dom.get(part1).and_then(|i| pose.get(i.name())) else {
            continue;
        };
        let transform = frame.to_data();
        if let Some(i) = dom.get_mut(motor) {
            i.properties_mut()
                .insert("Transform".into(), Variant::CFrame(transform));
        }
    }
}

/// Lays the parts of every animated model out through their joints, from
/// the parts no Motor6D moves down: a Motor6D puts
/// `Part1 = Part0 * C0 * Transform * C1⁻¹`, and a weld (an accessory's
/// `AccessoryWeld`, its handle as `Part0`) holds its two parts the same way
/// with no `Transform`, whichever side is reached first. Where the renderer
/// and scripts see the limbs and what they wear. Not logged: returns the
/// parts it moved.
pub fn lay_out(dom: &mut WeakDom, animations: &Animations) -> Vec<Ref> {
    let mut posed = Vec::new();
    for animator in animations.rigs.keys() {
        let Some(model) = model_of(dom, *animator) else {
            continue;
        };
        let joints: Vec<(Ref, Ref, CFrameData, bool)> = joints(dom, model)
            .into_iter()
            .filter_map(|j| {
                let p0 = reference(dom, j, "Part0")?;
                let p1 = reference(dom, j, "Part1")?;
                let instance = dom.get(j)?;
                let get = |key: &str| match instance.properties().get(key) {
                    Some(Variant::CFrame(c)) => Some(*c),
                    _ => None,
                };
                let motor = instance.class() == "Motor6D";
                let c0 = get("C0").unwrap_or(IDENTITY);
                let c1 = get("C1").unwrap_or(IDENTITY);
                let t = if motor {
                    get("Transform").unwrap_or(IDENTITY)
                } else {
                    IDENTITY
                };
                Some((p0, p1, mul(&mul(&c0, &t), &inverse(&c1)), motor))
            })
            .collect();
        let moved: HashSet<Ref> = joints.iter().filter(|j| j.3).map(|j| j.1).collect();
        let mut frontier: Vec<Ref> = joints
            .iter()
            .filter(|j| j.3 && !moved.contains(&j.0))
            .map(|j| j.0)
            .collect();
        frontier.dedup();
        let mut done: HashSet<Ref> = frontier.iter().copied().collect();
        while let Some(at) = frontier.pop() {
            let Some(Variant::CFrame(base)) = dom
                .get(at)
                .and_then(|i| i.properties().get("CFrame"))
                .cloned()
            else {
                continue;
            };
            for (p0, p1, offset, _) in &joints {
                let (next, frame) = if *p0 == at {
                    (*p1, mul(&base, offset))
                } else if *p1 == at {
                    (*p0, mul(&base, &inverse(offset)))
                } else {
                    continue;
                };
                if !done.insert(next) {
                    continue;
                }
                if let Some(i) = dom.get_mut(next) {
                    let old = i
                        .properties_mut()
                        .insert("CFrame".into(), Variant::CFrame(frame));
                    if old != Some(Variant::CFrame(frame)) {
                        posed.push(next);
                    }
                }
                frontier.push(next);
            }
        }
    }
    posed
}
