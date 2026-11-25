//! Video at run time: `VideoFrame` and `VideoPlayer` clocks over the
//! asset's own length, and what they show and sound like. The WebM
//! container gives what scripts see (`TimeLength`, `Resolution`,
//! `IsLoaded`) and the clock runs as the real one does: `Loaded` once the
//! asset arrives, `Played`/`Paused` as playback starts and stops, `DidLoop`
//! at the end of a looped video, `Ended` (`DidEnd` on a player) at the end
//! of any other, which stops it back at the start.
//!
//! On a client, a video on screen (a visible `VideoFrame`, or a
//! `VideoPlayer` wired to a visible `VideoDisplay`) is decoded at its
//! clock on a worker thread ([`picture`]) and bound to the frame or display
//! through [`rbx_dom::editable::VIDEO_PICTURE`]. A `VideoFrame`'s sound is
//! one of the sim's voices, a `VideoPlayer`'s goes through the audio graph;
//! both follow the same clock as the picture.

mod picture;

use std::collections::{BTreeSet, HashMap};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;

use rbx_animation::import::asset::content_id;
use rbx_assets::{AssetCache, AssetFetcher};
use rbx_dom::editable::VIDEO_PICTURE;
use rbx_dom::{Content, Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, CallReply, Event};
use rbx_reflection::ReflectionDatabase;

use crate::audio::{Clip, Voice};
use crate::humanoid::{flag, number};
use picture::{Movie, Picture};

/// The longest `LoadAsync` holds the script while the asset downloads.
const LOAD_WAIT: Duration = Duration::from_secs(10);

pub(crate) fn watches(class: &str) -> bool {
    matches!(class, "VideoFrame" | "VideoPlayer")
}

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    match class {
        "VideoFrame" => matches!(key, "Play" | "Pause"),
        "VideoPlayer" => matches!(key, "Play" | "Pause" | "Unload" | "LoadAsync"),
        _ => false,
    }
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    match class {
        "VideoFrame" => matches!(
            key,
            "DidLoop" | "Ended" | "Loaded" | "Paused" | "Played" | "SubtitlesChanged"
        ),
        "VideoPlayer" => matches!(key, "DidEnd" | "DidLoop" | "PlayFailed"),
        _ => false,
    }
}

pub(crate) fn is_property(class: &str, key: &str) -> bool {
    watches(class)
        && matches!(
            key,
            "IsLoaded" | "TimeLength" | "Resolution" | "TimePosition"
        )
        || (class == "VideoPlayer" && key == "IsPlaying")
}

/// What the container says of a video.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Info {
    pub seconds: f64,
    pub width: f32,
    pub height: f32,
}

enum Load {
    Pending(Receiver<Result<Movie, String>>),
    Ready(Arc<Movie>),
    Failed,
}

#[derive(Default)]
struct Clock {
    position: f64,
    /// A `VideoPlayer`'s `IsPlaying`.
    playing: bool,
    loaded: bool,
    failed: bool,
}

#[derive(Default)]
pub(crate) struct Videos {
    pub fetcher: Option<Arc<dyn AssetFetcher>>,
    /// Record or replay: when loads land.
    pub(crate) gate: Option<crate::replay::Gate>,
    loads: HashMap<u64, Load>,
    clocks: HashMap<Ref, Clock>,
    known: BTreeSet<Ref>,
    /// Signals a write queued, for the next step.
    queued: Vec<Event>,
    /// The videos on screen, by `VideoFrame` or `VideoPlayer`.
    pictures: HashMap<Ref, Picture>,
    /// The frames and displays a picture is bound to.
    bound: BTreeSet<Ref>,
}

/// A `VideoPlayer`'s sound as the audio graph plays it.
pub(crate) struct Playback {
    pub clip: Arc<Clip>,
    pub position: f64,
}

fn video_uri(dom: &WeakDom, r: Ref) -> String {
    let props = match dom.get(r) {
        Some(i) => i.properties(),
        None => return String::new(),
    };
    ["VideoContent", "Video"]
        .iter()
        .find_map(|k| match props.get(*k)? {
            Variant::Content(Content::Uri(s)) | Variant::String(s) if !s.is_empty() => {
                Some(s.clone())
            }
            _ => None,
        })
        .unwrap_or_default()
}

fn signal(owner: Ref, name: &str, args: Vec<Arg>) -> Event {
    Event::Signal {
        owner,
        name: name.into(),
        args,
    }
}

fn class(dom: &WeakDom, r: Ref) -> &str {
    dom.get(r).map_or("", |i| i.class())
}

/// Whether `r` is drawn: every GUI object up to its screen visible, the
/// screen enabled, and the screen in the player's `PlayerGui` or (a
/// `SurfaceGui`/`BillboardGui`) in the world.
fn on_screen(dom: &WeakDom, db: &ReflectionDatabase, r: Ref) -> bool {
    let mut at = Some(r);
    while let Some(here) = at {
        let class = class(dom, here);
        match class {
            "PlayerGui" | "Workspace" => return true,
            _ if db.is_subclass_of(class, "GuiObject") && !flag(dom, db, here, "Visible") => {
                return false
            }
            _ if db.is_subclass_of(class, "LayerCollector") && !flag(dom, db, here, "Enabled") => {
                return false
            }
            _ => at = dom.parent(here),
        }
    }
    false
}

/// `AbsoluteSize` as the GUI layout last left it.
fn absolute_size(dom: &WeakDom, r: Ref) -> [f32; 2] {
    match dom.get(r).and_then(|i| i.properties().get("AbsoluteSize")) {
        Some(Variant::Vector2(v)) => [v.x, v.y],
        _ => [0.0; 2],
    }
}

fn fetch(id: u64, fetcher: Option<Arc<dyn AssetFetcher>>) -> Result<Movie, String> {
    let fetcher = fetcher.ok_or_else(|| format!("video {id} is not cached and we are offline"))?;
    let bytes = fetcher.fetch_id(id).map_err(|e| e.to_string())?;
    if let Ok(cache) = AssetCache::new(None) {
        let _ = cache.put_id(id, &bytes);
    }
    picture::demux(&bytes).map_err(|e| format!("video {id}: {e}"))
}

impl Videos {
    pub(crate) fn watch(&mut self, r: Ref) {
        self.known.insert(r);
    }

    /// The video `r` shows, asking for it the first time; `Err` once it
    /// failed to load.
    fn info(&mut self, dom: &WeakDom, r: Ref) -> Option<Result<Arc<Movie>, ()>> {
        let id = content_id(&video_uri(dom, r))?;
        if let Some(Load::Pending(receiver)) = self.loads.get(&id) {
            if let Some(done) =
                crate::replay::Gate::take(&self.gate, || format!("video:{id}"), receiver)
            {
                let load = done.map_or(Load::Failed, |m| Load::Ready(Arc::new(m)));
                self.loads.insert(id, load);
            }
        }
        if !self.loads.contains_key(&id) {
            let cached = crate::replay::cached(self.fetcher.as_ref(), id);
            let load = match cached.as_deref().and_then(|b| picture::demux(b).ok()) {
                Some(movie) => Load::Ready(Arc::new(movie)),
                None => {
                    let (send, receive) = mpsc::channel();
                    let fetcher = self.fetcher.clone();
                    std::thread::spawn(move || {
                        let _ = send.send(fetch(id, fetcher));
                    });
                    Load::Pending(receive)
                }
            };
            self.loads.insert(id, load);
        }
        match self.loads.get(&id)? {
            Load::Ready(movie) => Some(Ok(movie.clone())),
            Load::Failed => Some(Err(())),
            Load::Pending(_) => None,
        }
    }

    /// `LoadAsync`: the asset, waiting for it a while.
    fn load_now(&mut self, dom: &WeakDom, r: Ref) -> bool {
        let Some(id) = content_id(&video_uri(dom, r)) else {
            return false;
        };
        if self.info(dom, r).is_none() {
            if let Some(Load::Pending(receiver)) = self.loads.get(&id) {
                let done = receiver.recv_timeout(LOAD_WAIT);
                let load = match done {
                    Ok(Ok(movie)) => Load::Ready(Arc::new(movie)),
                    _ => Load::Failed,
                };
                self.loads.insert(id, load);
            }
        }
        matches!(self.info(dom, r), Some(Ok(_)))
    }

    pub(crate) fn property(&mut self, dom: &WeakDom, r: Ref, key: &str) -> Option<Arg> {
        self.watch(r);
        let info = self.info(dom, r).and_then(Result::ok).map(|m| m.info);
        let clock = self.clocks.entry(r).or_default();
        Some(match key {
            "IsLoaded" => Arg::Bool(info.is_some()),
            "TimeLength" => Arg::Number(info.map_or(0.0, |i| i.seconds)),
            "Resolution" => Arg::Vector2(info.map_or([0.0; 2], |i| [i.width, i.height])),
            "TimePosition" => Arg::Number(clock.position),
            "IsPlaying" => Arg::Bool(clock.playing),
            _ => return None,
        })
    }

    pub(crate) fn call(
        &mut self,
        dom: &mut WeakDom,
        r: Ref,
        class: &str,
        method: &str,
    ) -> Result<CallReply, String> {
        self.watch(r);
        let uri = video_uri(dom, r);
        if class == "VideoFrame" {
            let play = method == "Play";
            if flag(dom, ReflectionDatabase::shared(), r, "Playing") == play {
                return Ok(CallReply::default());
            }
            let _ = dom.set_property(r, "Playing", Variant::Bool(play));
            let name = if play { "Played" } else { "Paused" };
            return Ok(CallReply {
                fire: vec![(r, name.into(), vec![Arg::String(uri)])],
                ..CallReply::default()
            });
        }
        let value = match method {
            "LoadAsync" => {
                let item = if self.load_now(dom, r) {
                    "Success"
                } else {
                    "Failure"
                };
                Arg::Enum {
                    enum_name: "AssetFetchStatus".into(),
                    item: item.into(),
                }
            }
            _ => {
                let clock = self.clocks.entry(r).or_default();
                clock.playing = method == "Play";
                clock.failed = false;
                if method == "Unload" {
                    clock.position = 0.0;
                }
                return Ok(CallReply::default());
            }
        };
        Ok(CallReply::value(value))
    }

    /// A script's write: `TimePosition` moves the clock, `Playing` starts
    /// or stops a frame (firing `Played`/`Paused` on the next step).
    pub(crate) fn write(&mut self, dom: &WeakDom, r: Ref, key: &str, value: &Arg) {
        self.watch(r);
        match (key, value) {
            ("TimePosition", Arg::Number(n)) => {
                self.clocks.entry(r).or_default().position = n.max(0.0);
            }
            ("Playing", Arg::Bool(play)) if class(dom, r) == "VideoFrame" => {
                let was = flag(dom, ReflectionDatabase::shared(), r, "Playing");
                if was != *play {
                    let name = if *play { "Played" } else { "Paused" };
                    let uri = video_uri(dom, r);
                    self.queued.push(signal(r, name, vec![Arg::String(uri)]));
                }
            }
            _ => {}
        }
    }

    /// One frame: every playing video's clock, and what that fired. A
    /// `client` then draws what's on screen; `displays` are the
    /// `VideoPlayer` → `VideoDisplay` wires.
    pub(crate) fn step(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        dt: f64,
        client: bool,
        displays: &[(Ref, Ref)],
    ) -> Vec<Event> {
        let events = self.tick(dom, db, dt);
        self.show(dom, db, client, displays);
        events
    }

    /// Starts, moves and stops the pictures of the videos on screen, and
    /// binds each to where it's drawn.
    fn show(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        client: bool,
        displays: &[(Ref, Ref)],
    ) {
        let mut wanted: HashMap<Ref, Vec<Ref>> = HashMap::new();
        for &r in self.known.iter().filter(|_| client) {
            match class(dom, r) {
                "VideoFrame" if on_screen(dom, db, r) => wanted.entry(r).or_default().push(r),
                "VideoPlayer" => {
                    let shown = displays
                        .iter()
                        .filter(|(p, d)| *p == r && on_screen(dom, db, *d))
                        .map(|(_, d)| *d);
                    let shown: Vec<Ref> = shown.collect();
                    if !shown.is_empty() {
                        wanted.insert(r, shown);
                    }
                }
                _ => {}
            }
        }
        self.pictures.retain(|r, _| wanted.contains_key(r));
        let bound: BTreeSet<Ref> = wanted.values().flatten().copied().collect();
        for (r, targets) in wanted {
            let Some(Ok(movie)) = self.info(dom, r) else {
                self.pictures.remove(&r);
                continue;
            };
            let picture = match self.pictures.remove(&r) {
                Some(p) if Arc::ptr_eq(&p.movie, &movie) => p,
                _ => match Picture::new(movie) {
                    Some(p) => p,
                    None => continue,
                },
            };
            let size = targets
                .iter()
                .map(|t| absolute_size(dom, *t))
                .fold([0.0f32; 2], |a, b| [a[0].max(b[0]), a[1].max(b[1])]);
            picture.show(self.clocks.get(&r).map_or(0.0, |c| c.position), size);
            let object = Variant::Content(Content::Object(picture.object));
            for target in targets {
                let props = dom.get(target).map(|i| i.properties());
                if props.and_then(|p| p.get(VIDEO_PICTURE)) != Some(&object) {
                    let _ = dom.set_property(target, VIDEO_PICTURE, object.clone());
                }
            }
            self.pictures.insert(r, picture);
        }
        // Frames and displays no longer showing anything let go of it.
        for r in self.bound.difference(&bound) {
            if dom.get(*r).is_some() {
                let _ = dom.remove_property(*r, VIDEO_PICTURE);
            }
        }
        self.bound = bound;
    }

    /// The `VideoFrame`s playing now as the listener hears them: their
    /// sound at their clock, `Volume` loud.
    pub(crate) fn voices(&mut self, dom: &WeakDom, db: &ReflectionDatabase) -> Vec<Voice> {
        let mut out = Vec::new();
        for r in self.known.clone() {
            if class(dom, r) != "VideoFrame" || !dom.in_tree(r) || !flag(dom, db, r, "Playing") {
                continue;
            }
            let Some(Ok(movie)) = self.info(dom, r) else {
                continue;
            };
            let Some(clip) = movie.audio.clone() else {
                continue;
            };
            out.push(Voice {
                sound: r,
                clip,
                position: self.clocks.get(&r).map_or(0.0, |c| c.position),
                speed: 1.0,
                gain: number(dom, db, r, "Volume").max(0.0) as f32,
                pan: 0.0,
                looped: flag(dom, db, r, "Looped"),
            });
        }
        out
    }

    /// A playing `VideoPlayer`'s sound, for the audio graph.
    pub(crate) fn playback(&mut self, dom: &WeakDom, r: Ref) -> Option<Playback> {
        let clock = self.clocks.get(&r)?;
        if !clock.playing {
            return None;
        }
        let position = clock.position;
        let Some(Ok(movie)) = self.info(dom, r) else {
            return None;
        };
        Some(Playback {
            clip: movie.audio.clone()?,
            position,
        })
    }

    fn tick(&mut self, dom: &mut WeakDom, db: &ReflectionDatabase, dt: f64) -> Vec<Event> {
        self.known.retain(|r| dom.get(*r).is_some());
        self.clocks.retain(|r, _| dom.get(*r).is_some());
        let mut events = std::mem::take(&mut self.queued);
        for r in self.known.clone() {
            if !dom.in_tree(r) {
                continue;
            }
            let frame = class(dom, r) == "VideoFrame";
            let info = self.info(dom, r);
            let uri = video_uri(dom, r);
            let playing = if frame {
                flag(dom, db, r, "Playing")
            } else {
                self.clocks.entry(r).or_default().playing
            };
            let clock = self.clocks.entry(r).or_default();
            match info {
                Some(Ok(_)) if frame && !clock.loaded => {
                    clock.loaded = true;
                    events.push(signal(r, "Loaded", vec![Arg::String(uri.clone())]));
                }
                Some(Err(())) if !frame && playing && !clock.failed => {
                    clock.failed = true;
                    clock.playing = false;
                    let status = Arg::Enum {
                        enum_name: "AssetFetchStatus".into(),
                        item: "Failure".into(),
                    };
                    events.push(signal(r, "PlayFailed", vec![status]));
                }
                _ => {}
            }
            let Some(Ok(movie)) = info else {
                continue;
            };
            let info = movie.info;
            if !playing || info.seconds <= 0.0 {
                continue;
            }
            let speed = if frame {
                1.0
            } else {
                number(dom, db, r, "PlaybackSpeed").max(0.0)
            };
            let looped = flag(dom, db, r, if frame { "Looped" } else { "Looping" });
            let clock = self.clocks.entry(r).or_default();
            clock.position += dt * speed;
            if clock.position < info.seconds {
                continue;
            }
            let args = || {
                if frame {
                    vec![Arg::String(uri.clone())]
                } else {
                    Vec::new()
                }
            };
            if looped {
                clock.position %= info.seconds;
                events.push(signal(r, "DidLoop", args()));
            } else {
                clock.position = 0.0;
                if frame {
                    let _ = dom.set_property(r, "Playing", Variant::Bool(false));
                } else {
                    clock.playing = false;
                }
                events.push(signal(r, if frame { "Ended" } else { "DidEnd" }, args()));
            }
        }
        events
    }
}

/// An EBML element at `at`: its id (marker bits kept), the data's start
/// and its end (the buffer's for an unknown size).
fn element(bytes: &[u8], at: usize) -> Option<(u32, usize, usize)> {
    let vint = |at: usize, keep_marker: bool| -> Option<(u64, usize)> {
        let first = *bytes.get(at)?;
        let len = first.leading_zeros() as usize + 1;
        if len > 8 {
            return None;
        }
        let mut value = if keep_marker {
            u64::from(first)
        } else {
            u64::from(first) & (0xFF >> len)
        };
        for i in 1..len {
            value = (value << 8) | u64::from(*bytes.get(at + i)?);
        }
        Some((value, len))
    };
    let (id, id_len) = vint(at, true)?;
    let (size, size_len) = vint(at + id_len, false)?;
    let start = at + id_len + size_len;
    let unknown = size == (1u64 << (7 * size_len)) - 1;
    let end = if unknown {
        bytes.len()
    } else {
        start.checked_add(size as usize)?.min(bytes.len())
    };
    Some((id as u32, start, end))
}

fn uint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |v, b| (v << 8) | u64::from(*b))
}

/// `TimeLength` and `Resolution` from a WebM (Matroska) file's header.
pub fn webm_info(bytes: &[u8]) -> Option<Info> {
    const EBML: u32 = 0x1A45_DFA3;
    const SEGMENT: u32 = 0x1853_8067;
    const INFO: u32 = 0x1549_A966;
    const TRACKS: u32 = 0x1654_AE6B;
    const TRACK_ENTRY: u32 = 0xAE;
    const VIDEO: u32 = 0xE0;
    if element(bytes, 0)?.0 != EBML {
        return None;
    }
    let mut scale = 1_000_000u64;
    let mut duration = None;
    let (mut width, mut height) = (0u64, 0u64);
    let mut stack = vec![(0usize, bytes.len())];
    while let Some((mut at, end)) = stack.pop() {
        while at < end {
            let Some((id, start, stop)) = element(bytes, at) else {
                break;
            };
            let data = &bytes[start..stop.min(end)];
            match id {
                SEGMENT | INFO | TRACKS | TRACK_ENTRY | VIDEO => stack.push((start, stop.min(end))),
                0x2A_D7B1 => scale = uint(data),
                0x4489 => {
                    duration = match data.len() {
                        4 => Some(f64::from(f32::from_be_bytes(data.try_into().ok()?))),
                        8 => Some(f64::from_be_bytes(data.try_into().ok()?)),
                        _ => None,
                    }
                }
                0xB0 => width = uint(data),
                0xBA => height = uint(data),
                _ => {}
            }
            at = stop;
        }
    }
    Some(Info {
        seconds: duration? * scale as f64 / 1e9,
        width: width as f32,
        height: height as f32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(id: &[u8], data: &[u8]) -> Vec<u8> {
        let mut out = id.to_vec();
        out.push(0x80 | data.len() as u8);
        out.extend_from_slice(data);
        out
    }

    #[test]
    fn a_webm_header_gives_length_and_size() {
        let info = [
            el(&[0x2A, 0xD7, 0xB1], &[0x0F, 0x42, 0x40]),
            el(&[0x44, 0x89], &12500.0f64.to_be_bytes()),
        ]
        .concat();
        let video = [el(&[0xB0], &[0x05, 0x00]), el(&[0xBA], &[0x02, 0xD0])].concat();
        let tracks = el(
            &[0x16, 0x54, 0xAE, 0x6B],
            &el(&[0xAE], &el(&[0xE0], &video)),
        );
        let segment = el(
            &[0x18, 0x53, 0x80, 0x67],
            &[el(&[0x15, 0x49, 0xA9, 0x66], &info), tracks].concat(),
        );
        let file = [el(&[0x1A, 0x45, 0xDF, 0xA3], &[]), segment].concat();
        assert_eq!(
            webm_info(&file),
            Some(Info {
                seconds: 12.5,
                width: 1280.0,
                height: 720.0
            })
        );
        assert_eq!(webm_info(b"not a video"), None);
    }
}
