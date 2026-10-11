//! An `Animator`: plays several clips at once and blends them into one pose
//! per joint, the way Roblox's does.
//!
//! The blend rule is the one the `AnimationPriority` enum documents. For each
//! joint, tracks are taken from the highest priority down, and evaluation
//! stops once the summed weight reaches 1. Tracks of equal priority blend in
//! proportion to their weights, normalised if they sum past 1. A total under 1
//! leaves the rest to the joint's rest pose, which is why one track at weight
//! 0.5 poses a joint halfway. The fades of `Play`, `Stop` and `AdjustWeight`
//! are linear here. Roblox does not document their curve.

use std::collections::BTreeMap;

use crate::clip::{Clip, Priority};
use crate::frame::Frame;
use crate::sample::{sample_joint, wrap};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TrackId(u32);

/// What a step of the clock made happen, in the order it happened.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// `AnimationTrack.KeyframeReached`, for a keyframe with its own name.
    KeyframeReached { track: TrackId, name: String },
    /// `GetMarkerReachedSignal(name)` firing with the marker's value.
    MarkerReached {
        track: TrackId,
        name: String,
        value: String,
    },
    DidLoop(TrackId),
    /// A track that does not loop reached its end.
    Ended(TrackId),
    /// A track's weight reached zero after `Stop`.
    Stopped(TrackId),
}

#[derive(Clone, Debug)]
pub struct Track {
    pub clip: Clip,
    pub time: f32,
    pub speed: f32,
    pub looped: bool,
    pub priority: Priority,
    pub playing: bool,
    weight: f32,
    target: f32,
    /// Weight per second towards `target`.
    rate: f32,
    stopping: bool,
}

impl Track {
    fn new(clip: Clip) -> Track {
        Track {
            looped: clip.looped,
            priority: clip.priority,
            clip,
            time: 0.,
            speed: 1.,
            playing: false,
            weight: 0.,
            target: 1.,
            rate: 0.,
            stopping: false,
        }
    }

    /// `WeightCurrent`.
    pub fn weight(&self) -> f32 {
        self.weight
    }

    /// `WeightTarget`.
    pub fn weight_target(&self) -> f32 {
        self.target
    }

    pub fn length(&self) -> f32 {
        self.clip.length()
    }

    fn fade_to(&mut self, target: f32, fade: f32) {
        self.target = target.max(0.);
        if fade > 0. {
            self.rate = (self.target - self.weight).abs() / fade;
        } else {
            self.rate = 0.;
            self.weight = self.target;
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Animator {
    tracks: BTreeMap<TrackId, Track>,
    next: u32,
}

/// The rank a priority blends at: `Core` is numerically 1000 but the lowest.
fn rank(priority: Priority) -> u32 {
    match priority {
        Priority::Core => 0,
        other => other.value() + 1,
    }
}

impl Animator {
    pub fn new() -> Animator {
        Animator::default()
    }

    /// `Animator:LoadAnimation`: a new, stopped track every call.
    pub fn load(&mut self, clip: Clip) -> TrackId {
        let id = TrackId(self.next);
        self.next += 1;
        self.tracks.insert(id, Track::new(clip));
        id
    }

    pub fn track(&self, id: TrackId) -> Option<&Track> {
        self.tracks.get(&id)
    }

    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        self.tracks.get_mut(&id)
    }

    /// `GetPlayingAnimationTracks`.
    pub fn playing(&self) -> impl Iterator<Item = TrackId> + '_ {
        self.tracks
            .iter()
            .filter(|(_, t)| t.playing)
            .map(|(id, _)| *id)
    }

    pub fn unload(&mut self, id: TrackId) {
        self.tracks.remove(&id);
    }

    /// `AnimationTrack:Play(fadeTime, weight, speed)`. Playing an already
    /// playing track restarts its fade but not its time, as Roblox's does.
    pub fn play(&mut self, id: TrackId, fade: f32, weight: f32, speed: f32) {
        let Some(track) = self.tracks.get_mut(&id) else {
            return;
        };
        if !track.playing {
            track.time = if speed < 0. { track.length() } else { 0. };
            track.weight = 0.;
        }
        track.playing = true;
        track.stopping = false;
        track.speed = speed;
        track.fade_to(weight, fade);
    }

    /// `AnimationTrack:Stop(fadeTime)`.
    pub fn stop(&mut self, id: TrackId, fade: f32) {
        if let Some(track) = self.tracks.get_mut(&id) {
            if track.playing {
                track.stopping = true;
                track.fade_to(0., fade);
            }
        }
    }

    pub fn adjust_weight(&mut self, id: TrackId, weight: f32, fade: f32) {
        if let Some(track) = self.tracks.get_mut(&id) {
            track.fade_to(weight, fade);
        }
    }

    pub fn adjust_speed(&mut self, id: TrackId, speed: f32) {
        if let Some(track) = self.tracks.get_mut(&id) {
            track.speed = speed;
        }
    }

    /// Advances every playing track by `dt` seconds of wall time.
    pub fn step(&mut self, dt: f32) -> Vec<Event> {
        let mut events = Vec::new();
        for (&id, track) in &mut self.tracks {
            if !track.playing {
                continue;
            }
            let length = track.length();
            let from = track.time;
            let mut to = from + dt * track.speed;
            let mut ended = false;
            if length > 0. && (to > length || to < 0.) {
                if track.looped {
                    crossed(track, id, from, if to > length { length } else { 0. }, &mut events);
                    events.push(Event::DidLoop(id));
                    to = to.rem_euclid(length);
                    let start = if track.speed >= 0. { 0. } else { length };
                    crossed(track, id, start, to, &mut events);
                } else {
                    to = to.clamp(0., length);
                    crossed(track, id, from, to, &mut events);
                    ended = true;
                }
            } else {
                crossed(track, id, from, to, &mut events);
            }
            track.time = to;

            let step = track.rate * dt;
            track.weight = if (track.target - track.weight).abs() <= step || track.rate == 0. {
                track.target
            } else {
                track.weight + step.copysign(track.target - track.weight)
            };

            if ended {
                events.push(Event::Ended(id));
                track.playing = false;
                track.weight = 0.;
                events.push(Event::Stopped(id));
            } else if track.stopping && track.weight <= 0. {
                track.playing = false;
                track.stopping = false;
                events.push(Event::Stopped(id));
            }
        }
        events
    }

    /// The blended `Transform` of every joint any playing track poses.
    pub fn pose(&self) -> BTreeMap<String, Frame> {
        let mut active: Vec<(&Track, f32)> = self
            .tracks
            .values()
            .filter(|t| t.playing && t.weight > 0.)
            .map(|t| (t, wrap(&t.clip, t.time)))
            .collect();
        active.sort_by_key(|(t, _)| std::cmp::Reverse(rank(t.priority)));

        let mut joints: Vec<String> = active.iter().flat_map(|(t, _)| t.clip.joints()).collect();
        joints.sort();
        joints.dedup();

        let mut out = BTreeMap::new();
        for joint in joints {
            let mut parts: Vec<(Frame, f32)> = Vec::new();
            let mut total = 0.;
            let mut i = 0;
            while i < active.len() && total < 1. {
                let level = rank(active[i].0.priority);
                let mut group: Vec<(Frame, f32)> = Vec::new();
                while i < active.len() && rank(active[i].0.priority) == level {
                    let (track, time) = active[i];
                    if let Some(s) = sample_joint(&track.clip, &joint, time) {
                        group.push((s.transform, track.weight * s.weight));
                    }
                    i += 1;
                }
                let sum: f32 = group.iter().map(|(_, w)| w).sum();
                let scale = if sum > 1. { 1. / sum } else { 1. };
                for (frame, w) in group {
                    parts.push((frame, w * scale));
                }
                total += sum.min(1.);
            }
            if let Some(frame) = average(&parts) {
                out.insert(joint, frame);
            }
        }
        out
    }
}

/// Fires the markers and named keyframes strictly after `from` and up to and
/// including `to`, in playback order (either direction).
fn crossed(track: &Track, id: TrackId, from: f32, to: f32, events: &mut Vec<Event>) {
    let hit = |time: f32| {
        if to >= from {
            time > from && time <= to
        } else {
            time < from && time >= to
        }
    };
    let mut reached: Vec<_> = track.clip.keyframes.iter().filter(|k| hit(k.time)).collect();
    if to < from {
        reached.reverse();
    }
    for k in reached {
        if k.name != "Keyframe" && !k.name.is_empty() {
            events.push(Event::KeyframeReached {
                track: id,
                name: k.name.clone(),
            });
        }
        for m in &k.markers {
            events.push(Event::MarkerReached {
                track: id,
                name: m.name.clone(),
                value: m.value.clone(),
            });
        }
    }
}

/// The weighted average of the given frames, with whatever weight is missing
/// from 1 going to the identity (the joint's rest pose).
pub fn average(parts: &[(Frame, f32)]) -> Option<Frame> {
    let mut acc: Option<Frame> = None;
    let mut seen = 0.;
    for &(frame, w) in parts {
        if w <= 0. {
            continue;
        }
        seen += w;
        acc = Some(match acc {
            None => frame,
            Some(so_far) => so_far.lerp(&frame, w / seen),
        });
    }
    let acc = acc?;
    Some(if seen < 1. {
        Frame::IDENTITY.lerp(&acc, seen)
    } else {
        acc
    })
}

#[cfg(test)]
mod tests;
