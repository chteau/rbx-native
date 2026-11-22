//! The Audio API's script side on a peer (the AudioApi golden): pins and
//! wires (`Wire.Connected` live, `WiringChanged` on both ends once a frame
//! sees the wiring change), `AudioPlayer`'s clock over the asset `Sound`
//! loads, `AudioTextToSpeech`'s over the line [`crate::speech`] speaks, a
//! `VideoPlayer`'s sound at its video's clock, the filter's response, how audible emitters and listeners are
//! to each other, and the members game scripts may not call. On a client
//! the graph renders every frame ([`crate::audio_graph`]): what its device
//! outputs play waits for the editor's output, its analyzers read their
//! input. The attenuation curves are set and read in Luau (`rbx_lua`'s
//! services/audio.luau), which keeps them on the instance as the text
//! [`curve`] parses.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use rbx_animation::import::asset::content_id;
use rbx_dom::{CFrameData, Content, Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, CallReply, Event};
use rbx_reflection::ReflectionDatabase;

use crate::audio::{rolloff, Clip, RollOff};
use crate::audio_graph::{
    filter, gain_at, pins, Biquad, Frame, Graph, Hearing, Kind, Node, Playback, Renderer, Wire,
    RATE,
};
use crate::humanoid::{flag, number};
use crate::sound::{enum_of, world_cframe, Sounds};
use crate::speech::{speak, Line};
use crate::video::Videos;

/// Rendered frames kept for the editor before the oldest go.
const BACKLOG: usize = RATE as usize / 2;

/// The instances [`Audio`] keeps track of.
pub(crate) fn watches(class: &str) -> bool {
    class == "Wire" || pins(class).is_some()
}

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    if class == "Wire" {
        return key == "RenameToDefault";
    }
    pins(class).is_some()
        && (matches!(key, "GetInputPins" | "GetOutputPins" | "GetConnectedWires")
            || matches!(
                (class, key),
                (
                    "AudioPlayer",
                    "Play" | "Stop" | "GetPlaybackState" | "Cancel" | "GetWaveformAsync"
                ) | (
                    "AudioTextToSpeech",
                    "Play" | "Pause" | "Unload" | "LoadAsync" | "GetWaveformAsync"
                ) | (
                    "AudioRecorder",
                    "RecordAsync"
                        | "Stop"
                        | "Clear"
                        | "CanRecordAsync"
                        | "GetUnrecordableInstancesAsync"
                ) | (
                    "AudioDeviceInput",
                    "GetUserIdAccessList" | "SetUserIdAccessList"
                ) | ("AudioFilter", "GetGainAt")
                    | ("AudioAnalyzer", "GetSpectrum")
                    | ("AudioStreamReader", "Read" | "Peek")
                    | ("AudioStreamWriter", "Write" | "Space")
                    | (
                        "AudioEmitter",
                        "GetAudibilityFor" | "GetInteractingListeners" | "GetWorldCFrame"
                    )
                    | (
                        "AudioListener",
                        "GetAudibilityFor" | "GetInteractingEmitters" | "GetWorldCFrame" | "Reset"
                    )
            ))
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    pins(class).is_some()
        && (key == "WiringChanged"
            || (matches!(class, "AudioPlayer" | "AudioTextToSpeech")
                && matches!(key, "Ended" | "Looped")))
}

pub(crate) fn is_property(class: &str, key: &str) -> bool {
    matches!(
        (class, key),
        ("Wire", "Connected")
            | ("AudioPlayer", "IsReady" | "TimeLength" | "TimePosition")
            | (
                "AudioTextToSpeech",
                "IsLoaded" | "TimeLength" | "TimePosition"
            )
            | ("AudioAnalyzer", "RmsLevel" | "PeakLevel")
            | ("AudioRecorder", "TimeLength")
    )
}

#[derive(Default)]
struct Player {
    position: f64,
    auto_played: bool,
}

#[derive(Default)]
pub(crate) struct Audio {
    known: BTreeSet<Ref>,
    players: HashMap<Ref, Player>,
    /// The connections the last frame saw.
    wired: Vec<Wire>,
    renderer: Renderer,
    /// What a client's device outputs played, not yet taken.
    pub(crate) out: Vec<Frame>,
    /// `SoundService:GetMixerTime()`: seconds of audio mixed so far.
    pub(crate) clock: f64,
    /// `AudioPlayer:Play(at)`/`Stop(at)` not yet due: (id, player, play,
    /// mixer time).
    scheduled: Vec<(i64, Ref, bool, f64)>,
    last_id: i64,
    /// `AudioDeviceInput:SetUserIdAccessList`, by input.
    access: HashMap<Ref, Vec<f64>>,
    /// What each `AudioTextToSpeech` loaded, and the line it says.
    speech: HashMap<Ref, (Line, Arc<Clip>)>,
}

fn line(dom: &WeakDom, db: &ReflectionDatabase, r: Ref) -> Line {
    Line {
        text: text(dom, db, r, "Text"),
        voice: text(dom, db, r, "VoiceId"),
        pitch: number(dom, db, r, "Pitch") as f32,
        speed: number(dom, db, r, "Speed") as f32,
    }
}

/// The longest an `AudioRecorder` keeps, in seconds.
const RECORD_LIMIT: f64 = 60.0;

/// `rbxtemp://<recorder>`: an `AudioRecorder`'s recording, as its
/// `GetTemporaryContent()` names it.
pub(crate) fn temporary(uri: &str) -> Option<Ref> {
    uri.strip_prefix("rbxtemp://")?.parse().ok().map(Ref::new)
}

fn class(dom: &WeakDom, r: Ref) -> &str {
    dom.get(r).map_or("", |i| i.class())
}

fn text(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> String {
    match dom.get(r).and_then(|i| db.stored_or_default(i, name)) {
        Some((_, Variant::String(s))) => s.clone(),
        _ => String::new(),
    }
}

fn instance(dom: &WeakDom, r: Ref, name: &str) -> Option<Ref> {
    match dom.get(r)?.properties().get(name) {
        Some(Variant::Ref(t)) if dom.get(*t).is_some() => Some(*t),
        _ => None,
    }
}

fn range(dom: &WeakDom, db: &ReflectionDatabase, r: Ref, name: &str) -> (f64, f64) {
    match dom.get(r).and_then(|i| db.stored_or_default(i, name)) {
        Some((_, Variant::NumberRange(n))) => (n.min.into(), n.max.into()),
        _ => (0.0, f64::MAX),
    }
}

fn decibels(level: f32) -> f32 {
    10f32.powf(level / 20.0)
}

/// `wire`'s connection: parented in the place, from an output pin of one
/// audio node to an input pin of another.
fn connection(dom: &WeakDom, db: &ReflectionDatabase, wire: Ref) -> Option<Wire> {
    if class(dom, wire) != "Wire" || !dom.in_tree(wire) {
        return None;
    }
    let source = instance(dom, wire, "SourceInstance")?;
    let target = instance(dom, wire, "TargetInstance")?;
    let source_pin = text(dom, db, wire, "SourceName");
    let target_pin = text(dom, db, wire, "TargetName");
    let valid = pins(class(dom, source))?.1.contains(&source_pin.as_str())
        && pins(class(dom, target))?.0.contains(&target_pin.as_str());
    valid.then_some(Wire {
        wire,
        source,
        source_pin,
        target,
        target_pin,
    })
}

fn clip(sounds: &mut Sounds, dom: &WeakDom, player: Ref) -> Option<Arc<Clip>> {
    let props = dom.get(player)?.properties();
    let uri = ["Asset", "AudioContent", "AssetId"]
        .iter()
        .find_map(|k| match props.get(*k)? {
            Variant::Content(Content::Uri(s)) | Variant::String(s) if !s.is_empty() => Some(s),
            _ => None,
        })?;
    match temporary(uri) {
        Some(recorder) => sounds.recordings.get(&recorder).cloned(),
        None => sounds.clip(content_id(uri)?),
    }
}

/// `samples` peaks of `clip` across `range` (seconds), as
/// `GetWaveformAsync` gives them; none without a clip.
fn waveform(clip: Option<&Clip>, range: (f64, f64), samples: usize) -> Arg {
    let Some(clip) = clip else {
        return Arg::List(Vec::new());
    };
    let channels = usize::from(clip.channels.max(1));
    let frames = clip.frames();
    let from = range.0.max(0.0);
    let span = (range.1.min(clip.seconds()) - from).max(0.0);
    let at = |k: usize| {
        (((from + span * k as f64 / samples as f64) * f64::from(clip.rate)).round() as usize)
            .min(frames)
    };
    Arg::List(
        (0..samples)
            .map(|i| {
                let peak = (at(i)..at(i + 1).max(at(i) + 1).min(frames))
                    .flat_map(|f| &clip.samples[f * channels..(f + 1) * channels])
                    .fold(0f32, |m, s| m.max(s.abs()));
                Arg::Number(f64::from(peak))
            })
            .collect(),
    )
}

/// A recording as a clip an `AudioPlayer` can play.
fn recording(frames: &[Frame]) -> Clip {
    Clip {
        samples: frames.iter().flatten().copied().collect(),
        channels: 2,
        rate: RATE as u32,
    }
}

/// Where `player` starts and stops, and where looping jumps back from and
/// to: its regions within the clip, the whole of it where one is empty.
fn regions(dom: &WeakDom, db: &ReflectionDatabase, player: Ref, length: f64) -> [f64; 4] {
    let within = |(lo, hi): (f64, f64), (from, to): (f64, f64)| {
        let (lo, hi) = (lo.clamp(from, to), hi.clamp(from, to));
        if hi > lo {
            (lo, hi)
        } else {
            (from, to)
        }
    };
    let (start, end) = within(range(dom, db, player, "PlaybackRegion"), (0.0, length));
    let (loop_start, loop_end) = within(range(dom, db, player, "LoopRegion"), (start, end));
    [start, end, loop_start, loop_end]
}

fn set_playing(dom: &mut WeakDom, player: Ref, playing: bool, position: f64) {
    let _ = dom.set_property(player, "IsPlaying", Variant::Bool(playing));
    let _ = dom.set_property(player, "TimePosition", Variant::Float64(position));
}

fn filter_of(dom: &WeakDom, db: &ReflectionDatabase, r: Ref) -> Vec<Biquad> {
    let n = |name| number(dom, db, r, name) as f32;
    filter(
        enum_of(dom, db, r, "FilterType"),
        n("Frequency"),
        n("Q"),
        n("Gain"),
    )
}

/// A curve as services/audio.luau keeps it ("key=volume ...", sorted by
/// key); empty when none was set.
fn curve(dom: &WeakDom, r: Ref, name: &str) -> Vec<(f64, f64)> {
    let Some(Variant::String(s)) = dom.get(r).and_then(|i| i.properties().get(name)) else {
        return Vec::new();
    };
    s.split_whitespace()
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            Some((k.parse().ok()?, v.parse().ok()?))
        })
        .collect()
}

/// The curve at `x`, straight between its points and level past its ends;
/// `None` for an empty one.
fn along(curve: &[(f64, f64)], x: f64) -> Option<f64> {
    let (first, last) = (curve.first()?, curve.last()?);
    if x <= first.0 {
        return Some(first.1);
    }
    Some(curve.windows(2).find(|w| x <= w[1].0).map_or(last.1, |w| {
        w[0].1 + (w[1].1 - w[0].1) * (x - w[0].0) / (w[1].0 - w[0].0)
    }))
}

/// Where an emitter or a listener is: its parent, or its
/// `PositionInstance`.
fn place(dom: &WeakDom, db: &ReflectionDatabase, node: Ref) -> Option<CFrameData> {
    let at = if enum_of(dom, db, node, "PositionType") == 1 {
        instance(dom, node, "PositionInstance")?
    } else {
        dom.parent(node)?
    };
    world_cframe(dom, db, at)
}

/// How loud `emitter` is to `listener` (both distance curves, the
/// emitter's default `DistanceAttenuationMode` rolloff between its
/// bounds when it has none, and both angle curves) and where it sits, -1
/// left to 1 right. Either without a place in the world is heard as if at
/// the listener.
fn hearing(dom: &WeakDom, db: &ReflectionDatabase, emitter: Ref, listener: Ref) -> (f64, f64) {
    let (Some(e), Some(l)) = (place(dom, db, emitter), place(dom, db, listener)) else {
        return (1.0, 0.0);
    };
    let to = [
        f64::from(l.position.x - e.position.x),
        f64::from(l.position.y - e.position.y),
        f64::from(l.position.z - e.position.z),
    ];
    let distance = to.iter().map(|v| v * v).sum::<f64>().sqrt();
    let dot =
        |a: [f64; 3], b: [f32; 3]| a.iter().zip(b).map(|(a, b)| a * f64::from(b)).sum::<f64>();
    let angle = |cf: &CFrameData, sign: f64| {
        let r = cf.rotation;
        let look = [-r[2], -r[5], -r[8]];
        if distance < 1e-6 {
            return 0.0;
        }
        (sign * dot(to, look) / distance)
            .clamp(-1.0, 1.0)
            .acos()
            .to_degrees()
    };
    let emitter_gain =
        along(&curve(dom, emitter, "DistanceAttenuation"), distance).unwrap_or_else(|| {
            let (min, max) = range(dom, db, emitter, "DistanceAttenuationBounds");
            let mode = match enum_of(dom, db, emitter, "DistanceAttenuationMode") {
                1 => RollOff::InverseTapered,
                2 => RollOff::Linear,
                3 => RollOff::LinearSquare,
                _ => RollOff::Inverse,
            };
            rolloff(mode, distance, min, max)
        });
    let gain = emitter_gain
        * along(&curve(dom, listener, "DistanceAttenuation"), distance).unwrap_or(1.0)
        * along(&curve(dom, emitter, "AngleAttenuation"), angle(&e, 1.0)).unwrap_or(1.0)
        * along(&curve(dom, listener, "AngleAttenuation"), angle(&l, -1.0)).unwrap_or(1.0);
    // The listener's right is its rotation's first column.
    let r = l.rotation;
    let pan = if distance < 1e-6 {
        0.0
    } else {
        -dot(to, [r[0], r[3], r[6]]) / distance
    };
    (gain.clamp(0.0, 1.0), pan)
}

impl Audio {
    pub(crate) fn watch(&mut self, r: Ref) {
        self.known.insert(r);
    }

    fn track(&mut self, dom: &WeakDom, db: &ReflectionDatabase, player: Ref) -> &mut Player {
        self.players.entry(player).or_insert_with(|| Player {
            position: number(dom, db, player, "TimePosition"),
            auto_played: false,
        })
    }

    /// What `player` plays: an `AudioPlayer`'s asset, or the line an
    /// `AudioTextToSpeech` loaded while its properties still say it.
    fn source(
        &self,
        sounds: &mut Sounds,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        player: Ref,
    ) -> Option<Arc<Clip>> {
        if class(dom, player) != "AudioTextToSpeech" {
            return clip(sounds, dom, player);
        }
        let (said, clip) = self.speech.get(&player)?;
        (*said == line(dom, db, player)).then(|| clip.clone())
    }

    /// Speaks `speaker`'s line (once per line); false for nothing to say.
    fn load_speech(&mut self, dom: &WeakDom, db: &ReflectionDatabase, speaker: Ref) -> bool {
        let said = line(dom, db, speaker);
        if self.speech.get(&speaker).is_some_and(|(s, _)| *s == said) {
            return true;
        }
        match speak(&said) {
            Some(clip) => {
                self.speech.insert(speaker, (said, clip));
                true
            }
            None => {
                self.speech.remove(&speaker);
                false
            }
        }
    }

    /// The `VideoPlayer` → `VideoDisplay` wires the last frame saw.
    pub(crate) fn displays(&self, dom: &WeakDom) -> Vec<(Ref, Ref)> {
        self.wired
            .iter()
            .filter(|w| {
                class(dom, w.source) == "VideoPlayer" && class(dom, w.target) == "VideoDisplay"
            })
            .map(|w| (w.source, w.target))
            .collect()
    }

    fn connections(&self, dom: &WeakDom, db: &ReflectionDatabase) -> Vec<Wire> {
        self.known
            .iter()
            .filter_map(|w| connection(dom, db, *w))
            .collect()
    }

    /// The nodes of `class` in the place sharing `node`'s
    /// `AudioInteractionGroup`.
    fn interacting(&self, dom: &WeakDom, db: &ReflectionDatabase, node: Ref, of: &str) -> Arg {
        let group = text(dom, db, node, "AudioInteractionGroup");
        Arg::Instances(
            self.known
                .iter()
                .copied()
                .filter(|r| {
                    class(dom, *r) == of
                        && dom.in_tree(*r)
                        && text(dom, db, *r, "AudioInteractionGroup") == group
                })
                .collect(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn call(
        &mut self,
        sounds: &mut Sounds,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        node: Ref,
        class: &str,
        method: &str,
        args: &[Arg],
    ) -> Result<CallReply, String> {
        let value = match method {
            // Live servers refuse these for now (NewApis `reader_read`,
            // `writer_space`).
            "Read" | "Peek" | "Write" | "Space" => {
                return Err(format!("{class} is not enabled"));
            }
            "GetInputPins" | "GetOutputPins" => {
                let (inputs, outputs) = pins(class).unwrap_or_default();
                let list = if method == "GetInputPins" {
                    inputs
                } else {
                    outputs
                };
                Arg::List(list.iter().map(|p| Arg::String((*p).into())).collect())
            }
            "GetConnectedWires" => {
                let pin = match args.first() {
                    Some(Arg::String(s)) => s.as_str(),
                    _ => "",
                };
                Arg::Instances(
                    self.connections(dom, db)
                        .into_iter()
                        .filter(|w| {
                            (w.source == node && w.source_pin == pin)
                                || (w.target == node && w.target_pin == pin)
                        })
                        .map(|w| w.wire)
                        .collect(),
                )
            }
            "Play" | "Stop" if class == "AudioPlayer" => {
                let play = method == "Play";
                if let Some(Arg::Number(at)) = args.first() {
                    self.last_id += 1;
                    self.scheduled.push((self.last_id, node, play, *at));
                    self.due(dom, db);
                    return Ok(CallReply::value(Arg::Number(self.last_id as f64)));
                }
                let position = self.track(dom, db, node).position;
                set_playing(dom, node, play, position);
                return Ok(CallReply::default());
            }
            "Cancel" => {
                let id = match args.first() {
                    Some(Arg::Number(id)) => *id,
                    _ => return Ok(CallReply::value(Arg::Bool(false))),
                };
                let before = self.scheduled.len();
                self.scheduled
                    .retain(|(i, r, ..)| !(*r == node && *i as f64 == id));
                Arg::Bool(self.scheduled.len() < before)
            }
            // A speaker loads its line as it starts; one with nothing to
            // say ends on the next frame.
            "Play" => {
                self.load_speech(dom, db, node);
                let position = self.track(dom, db, node).position;
                set_playing(dom, node, true, position);
                return Ok(CallReply::default());
            }
            "Pause" => {
                let _ = dom.set_property(node, "IsPlaying", Variant::Bool(false));
                return Ok(CallReply::default());
            }
            "Unload" => {
                self.speech.remove(&node);
                self.track(dom, db, node).position = 0.0;
                set_playing(dom, node, false, 0.0);
                return Ok(CallReply::default());
            }
            "LoadAsync" => Arg::Enum {
                enum_name: "AssetFetchStatus".into(),
                item: if self.load_speech(dom, db, node) {
                    "Success"
                } else {
                    "Failure"
                }
                .into(),
            },
            "GetWaveformAsync" => {
                let range = match args.first() {
                    Some(Arg::List(r)) => match r.as_slice() {
                        [Arg::Number(a), Arg::Number(b)] => (*a, *b),
                        _ => (0.0, f64::MAX),
                    },
                    _ => return Err("Argument 1 missing or nil".into()),
                };
                let samples = match args.get(1) {
                    Some(Arg::Number(n)) => n.clamp(0.0, 65536.0) as usize,
                    _ => return Err("Argument 2 missing or nil".into()),
                };
                let clip = self.source(sounds, dom, db, node);
                waveform(clip.as_deref(), range, samples)
            }
            "RecordAsync" | "Stop" => {
                let on = method == "RecordAsync";
                if on {
                    sounds.recordings.remove(&node);
                    self.renderer.recorded.insert(node, Vec::new());
                }
                let _ = dom.set_property(node, "IsRecording", Variant::Bool(on));
                return Ok(CallReply::default());
            }
            "Clear" => {
                self.renderer.recorded.remove(&node);
                sounds.recordings.remove(&node);
                return Ok(CallReply::default());
            }
            // Everything here is ours to record.
            "CanRecordAsync" => Arg::Bool(true),
            "GetUnrecordableInstancesAsync" => Arg::Instances(Vec::new()),
            "GetUserIdAccessList" => Arg::List(
                self.access
                    .get(&node)
                    .map_or_else(Vec::new, |l| l.iter().map(|n| Arg::Number(*n)).collect()),
            ),
            "SetUserIdAccessList" => {
                let Some(Arg::List(list)) = args.first() else {
                    return Err("Unable to cast to Array".into());
                };
                let ids = list
                    .iter()
                    .map(|a| match a {
                        Arg::Number(n) => Ok(*n),
                        _ => Err("Unable to cast to Array".to_owned()),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.access.insert(node, ids);
                return Ok(CallReply::default());
            }
            "GetGainAt" => {
                let at = match args.first() {
                    Some(Arg::Number(n)) => *n as f32,
                    _ => 0.0,
                };
                Arg::Number(gain_at(&filter_of(dom, db, node), at))
            }
            "GetSpectrum" => Arg::List(self.renderer.analysis.get(&node).map_or_else(
                Vec::new,
                |a| {
                    a.spectrum
                        .iter()
                        .map(|v| Arg::Number(f64::from(*v)))
                        .collect()
                },
            )),
            "GetAudibilityFor" => {
                let Some(Arg::Instance(other)) = args.first() else {
                    return Err("Argument 1 missing or nil".into());
                };
                let (emitter, listener) = if class == "AudioEmitter" {
                    (node, *other)
                } else {
                    (*other, node)
                };
                Arg::Number(hearing(dom, db, emitter, listener).0)
            }
            "GetInteractingListeners" => self.interacting(dom, db, node, "AudioListener"),
            "GetInteractingEmitters" => self.interacting(dom, db, node, "AudioEmitter"),
            _ => {
                return Err(format!(
                    "The current thread cannot call '{method}' (lacking capability RobloxScript)"
                ))
            }
        };
        Ok(CallReply::value(value))
    }

    /// Plays and stops whose mixer time came.
    fn due(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase) {
        let now = self.clock;
        let (ready, later) = std::mem::take(&mut self.scheduled)
            .into_iter()
            .partition::<Vec<_>, _>(|(.., at)| *at <= now);
        self.scheduled = later;
        for (_, player, play, _) in ready {
            let position = self.track(dom, db, player).position;
            set_playing(dom, player, play, position);
        }
    }

    pub(crate) fn property(
        &mut self,
        sounds: &mut Sounds,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        node: Ref,
        key: &str,
    ) -> Option<Arg> {
        let analysis = |pick: fn(&crate::audio_graph::Analysis) -> f32| {
            Arg::Number(
                self.renderer
                    .analysis
                    .get(&node)
                    .map_or(0.0, |a| f64::from(pick(a))),
            )
        };
        Some(match key {
            "Connected" => Arg::Bool(connection(dom, db, node).is_some()),
            "IsReady" | "IsLoaded" => Arg::Bool(self.source(sounds, dom, db, node).is_some()),
            "TimeLength" if class(dom, node) != "AudioRecorder" => Arg::Number(
                self.source(sounds, dom, db, node)
                    .map_or(0.0, |c| c.seconds()),
            ),
            "RmsLevel" => analysis(|a| a.rms),
            "PeakLevel" => analysis(|a| a.peak),
            "TimePosition" => Arg::Number(self.track(dom, db, node).position),
            "TimeLength" if class(dom, node) == "AudioRecorder" => {
                Arg::Number(match self.renderer.recorded.get(&node) {
                    Some(frames) => frames.len() as f64 / f64::from(RATE),
                    None => sounds.recordings.get(&node).map_or(0.0, |c| c.seconds()),
                })
            }
            _ => return None,
        })
    }

    /// A script moving an `AudioPlayer`'s `TimePosition` moves its clock.
    pub(crate) fn write(&mut self, dom: &WeakDom, db: &ReflectionDatabase, node: Ref, value: &Arg) {
        if let Arg::Number(n) = value {
            self.track(dom, db, node).position = n.max(0.0);
        }
    }

    /// One frame: the wiring that changed since the last, rendering on a
    /// `client` (whose `local` player decides which device outputs it
    /// hears), then every player's clock and the `Ended`/`Looped` it fires.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn step(
        &mut self,
        sounds: &mut Sounds,
        videos: &mut Videos,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        dt: f64,
        client: bool,
        local: Option<Ref>,
    ) -> Vec<Event> {
        self.known.retain(|r| dom.get(*r).is_some());
        self.players.retain(|r, _| dom.get(*r).is_some());
        self.clock += dt;
        self.due(dom, db);
        let signal = |owner: Ref, name: &str, args: Vec<Arg>| Event::Signal {
            owner,
            name: name.into(),
            args,
        };
        let mut events = Vec::new();
        let now = self.connections(dom, db);
        let gone = self
            .wired
            .iter()
            .filter(|w| !now.contains(w))
            .map(|w| (false, w));
        let new = now
            .iter()
            .filter(|w| !self.wired.contains(w))
            .map(|w| (true, w));
        for (connected, w) in gone.chain(new) {
            for (owner, pin, other) in [
                (w.source, &w.source_pin, w.target),
                (w.target, &w.target_pin, w.source),
            ] {
                events.push(signal(
                    owner,
                    "WiringChanged",
                    vec![
                        Arg::Bool(connected),
                        Arg::String(pin.clone()),
                        Arg::Instance(w.wire),
                        Arg::Instance(other),
                    ],
                ));
            }
        }
        self.wired = now;

        if client {
            let graph = self.graph(sounds, videos, dom, db, local);
            if !graph.nodes.is_empty() {
                let block = self
                    .renderer
                    .render(&graph, (dt * f64::from(RATE)).round() as usize);
                self.out.extend(block);
                let excess = self.out.len().saturating_sub(BACKLOG);
                self.out.drain(..excess);
            }
            // A recorder that stopped, or filled up, keeps its recording as
            // a clip.
            let limit = (RECORD_LIMIT * f64::from(RATE)) as usize;
            let done: Vec<(Ref, bool)> = self
                .renderer
                .recorded
                .iter_mut()
                .filter_map(|(r, frames)| {
                    frames.truncate(limit);
                    let full = frames.len() >= limit;
                    (full || !flag(dom, db, *r, "IsRecording")).then_some((*r, full))
                })
                .collect();
            for (recorder, full) in done {
                if full {
                    let _ = dom.set_property(recorder, "IsRecording", Variant::Bool(false));
                }
                if let Some(frames) = self.renderer.recorded.remove(&recorder) {
                    sounds
                        .recordings
                        .insert(recorder, Arc::new(recording(&frames)));
                }
            }
        }
        let players: Vec<Ref> = self
            .known
            .iter()
            .copied()
            .filter(|r| matches!(class(dom, *r), "AudioPlayer" | "AudioTextToSpeech"))
            .collect();
        for player in players {
            let Some(clip) = self.source(sounds, dom, db, player) else {
                // A voice with nothing loaded is done as soon as it starts.
                if class(dom, player) == "AudioTextToSpeech" && flag(dom, db, player, "IsPlaying") {
                    let _ = dom.set_property(player, "IsPlaying", Variant::Bool(false));
                    events.push(signal(player, "Ended", Vec::new()));
                }
                continue;
            };
            let track = self.track(dom, db, player);
            if !track.auto_played && flag(dom, db, player, "AutoPlay") && dom.in_tree(player) {
                track.auto_played = true;
                let position = track.position;
                set_playing(dom, player, true, position);
            }
            if !flag(dom, db, player, "IsPlaying") {
                continue;
            }
            let [start, end, loop_start, loop_end] = regions(dom, db, player, clip.seconds());
            let looping = flag(dom, db, player, "Looping");
            let speed = number(dom, db, player, "PlaybackSpeed").max(0.0);
            let track = self.track(dom, db, player);
            track.position = track.position.max(start) + dt * speed;
            if looping && track.position >= loop_end {
                track.position = loop_start + (track.position - loop_end) % (loop_end - loop_start);
                events.push(signal(player, "Looped", Vec::new()));
            } else if !looping && track.position >= end {
                track.position = start;
                set_playing(dom, player, false, start);
                events.push(signal(player, "Ended", Vec::new()));
            }
        }
        events
    }

    /// The client's graph as its DOM has it now.
    fn graph(
        &mut self,
        sounds: &mut Sounds,
        videos: &mut Videos,
        dom: &WeakDom,
        db: &ReflectionDatabase,
        local: Option<Ref>,
    ) -> Graph {
        let mut graph = Graph {
            wires: self.wired.clone(),
            ..Graph::default()
        };
        let (mut emitters, mut listeners) = (Vec::new(), Vec::new());
        for r in self.known.clone() {
            if !dom.in_tree(r) {
                continue;
            }
            let n = |name| number(dom, db, r, name) as f32;
            let level = |name| decibels(n(name));
            let kind = match class(dom, r) {
                "AudioPlayer" | "AudioTextToSpeech" => {
                    let clip = self
                        .source(sounds, dom, db, r)
                        .filter(|_| flag(dom, db, r, "IsPlaying"));
                    let length = clip.as_ref().map_or(0.0, |c| c.seconds());
                    let [_, end, loop_start, loop_end] = regions(dom, db, r, length);
                    let looping = flag(dom, db, r, "Looping");
                    Kind::Player(Playback {
                        clip,
                        position: self.track(dom, db, r).position,
                        speed: number(dom, db, r, "PlaybackSpeed").max(0.0),
                        volume: n("Volume"),
                        end: if looping { loop_end } else { end },
                        loop_start,
                        looping,
                    })
                }
                "VideoPlayer" => {
                    let playback = videos.playback(dom, r);
                    let end = playback.as_ref().map_or(0.0, |p| p.clip.seconds());
                    Kind::Player(Playback {
                        position: playback.as_ref().map_or(0.0, |p| p.position),
                        clip: playback.map(|p| p.clip),
                        speed: number(dom, db, r, "PlaybackSpeed").max(0.0),
                        volume: n("Volume"),
                        end,
                        loop_start: 0.0,
                        looping: flag(dom, db, r, "Looping"),
                    })
                }
                "AudioDeviceInput" | "AudioWindSynthesizer" => Kind::Silence,
                "AudioGate" => {
                    let (close, open) = range(dom, db, r, "Threshold");
                    Kind::Gate {
                        open: open as f32,
                        close: close as f32,
                        attack: n("Attack"),
                        release: n("Release"),
                    }
                }
                "AudioRecorder" if flag(dom, db, r, "IsRecording") => Kind::Recorder,
                "AudioFader" => Kind::Fader(n("Volume")),
                "AudioFilter" => Kind::Filter(filter_of(dom, db, r)),
                "AudioEqualizer" => {
                    let (lo, hi) = range(dom, db, r, "MidRange");
                    let (lo, hi) = (lo as f32, hi as f32);
                    let mid = (lo * hi).sqrt();
                    Kind::Filter(
                        [
                            filter(1, lo, 0.707, n("LowGain")),
                            filter(0, mid, mid / (hi - lo).max(1.0), n("MidGain")),
                            filter(2, hi, 0.707, n("HighGain")),
                        ]
                        .concat(),
                    )
                }
                "AudioEcho" => Kind::Echo {
                    delay: n("DelayTime"),
                    feedback: n("Feedback"),
                    dry: level("DryLevel"),
                    wet: level("WetLevel"),
                },
                "AudioReverb" => Kind::Reverb {
                    decay: n("DecayTime"),
                    pre_delay: n("EarlyDelayTime"),
                    high_cut: n("HighCutFrequency"),
                    dry: level("DryLevel"),
                    wet: level("WetLevel"),
                },
                "AudioDistortion" => Kind::Distortion(n("Level")),
                "AudioChorus" => Kind::Modulation {
                    center: 0.02,
                    swing: 0.01 * n("Depth"),
                    rate: n("Rate"),
                    mix: n("Mix"),
                    feedback: 0.0,
                },
                "AudioFlanger" => Kind::Modulation {
                    center: 0.003,
                    swing: 0.0025 * n("Depth"),
                    rate: n("Rate"),
                    mix: n("Mix"),
                    feedback: 0.5,
                },
                "AudioTremolo" => Kind::Tremolo {
                    depth: n("Depth"),
                    duty: n("Duty"),
                    frequency: n("Frequency"),
                    square: n("Square"),
                },
                "AudioCompressor" => Kind::Compressor {
                    threshold: n("Threshold"),
                    ratio: n("Ratio"),
                    attack: n("Attack"),
                    release: n("Release"),
                    makeup: n("MakeupGain"),
                },
                "AudioLimiter" => Kind::Limiter {
                    max: level("MaxLevel"),
                    release: n("Release"),
                },
                "AudioPitchShifter" => Kind::PitchShifter {
                    pitch: n("Pitch"),
                    window: [0.025, 0.05, 0.1][enum_of(dom, db, r, "WindowSize").min(2) as usize],
                },
                "AudioChannelMixer" => Kind::Mixer,
                "AudioChannelSplitter" => Kind::Splitter,
                "AudioEmitter" => {
                    emitters.push(r);
                    Kind::Emitter
                }
                "AudioListener" => {
                    listeners.push(r);
                    Kind::Listener
                }
                "AudioDeviceOutput" => match instance(dom, r, "Player") {
                    Some(player) if Some(player) != local => continue,
                    _ => Kind::Output,
                },
                "AudioAnalyzer" => Kind::Analyzer {
                    window: [512, 1024, 2048][enum_of(dom, db, r, "WindowSize").min(2) as usize],
                    spectrum: flag(dom, db, r, "SpectrumEnabled"),
                },
                _ => continue,
            };
            let bypass = flag(dom, db, r, "Bypass");
            graph.nodes.insert(r, Node { kind, bypass });
        }
        for &listener in &listeners {
            let group = text(dom, db, listener, "AudioInteractionGroup");
            for &emitter in &emitters {
                if text(dom, db, emitter, "AudioInteractionGroup") == group {
                    let (gain, pan) = hearing(dom, db, emitter, listener);
                    graph.hearing.push(Hearing {
                        emitter,
                        listener,
                        gain: gain as f32,
                        pan: pan as f32,
                    });
                }
            }
        }
        graph
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::humanoid::{identity, translated};

    fn placed(dom: &mut WeakDom, class: &str, at: [f32; 3]) -> Ref {
        let part = dom.new_instance("Part", "Part", None);
        let cframe = Variant::CFrame(translated(identity(), at));
        let _ = dom.set_property(part, "CFrame", cframe);
        dom.new_instance(class, class, Some(part))
    }

    #[test]
    fn an_emitter_fades_with_distance_and_sits_to_its_side() {
        let db = ReflectionDatabase::shared();
        let mut dom = WeakDom::new();
        let listener = placed(&mut dom, "AudioListener", [0.0, 0.0, 0.0]);
        let near = placed(&mut dom, "AudioEmitter", [0.0, 0.0, -3.0]);
        let far = placed(&mut dom, "AudioEmitter", [0.0, 0.0, -40.0]);
        let right = placed(&mut dom, "AudioEmitter", [10.0, 0.0, 0.0]);
        assert_eq!(
            hearing(&dom, db, near, listener),
            (1.0, 0.0),
            "inside the bounds"
        );
        let (gain, pan) = hearing(&dom, db, far, listener);
        assert!(
            (gain - 0.1).abs() < 1e-9 && pan.abs() < 1e-9,
            "inverse: 4 / 40"
        );
        assert!(
            (hearing(&dom, db, right, listener).1 - 1.0).abs() < 1e-6,
            "on the right"
        );
        let _ = dom.set_property(
            far,
            "DistanceAttenuation",
            Variant::String("0=1 100=0".into()),
        );
        assert!(
            (hearing(&dom, db, far, listener).0 - 0.6).abs() < 1e-9,
            "its own curve"
        );
    }
}
