//! Video at run time: `VideoFrame` and `VideoPlayer` clocks over the
//! asset's own length. There's no MIT-compatible VP8/VP9 decoder in pure
//! Rust, so nothing is drawn; the WebM container is read for what scripts
//! see (`TimeLength`, `Resolution`, `IsLoaded`) and the clock runs as the
//! real one does: `Loaded` once the asset arrives, `Played`/`Paused` as
//! playback starts and stops, `DidLoop` at the end of a looped video,
//! `Ended` (`DidEnd` on a player) at the end of any other, which stops it
//! back at the start.

use std::collections::{BTreeSet, HashMap};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::time::Duration;

use rbx_animation::import::asset::content_id;
use rbx_assets::{AssetCache, AssetFetcher};
use rbx_dom::{Content, Ref, Variant, WeakDom};
use rbx_lua::server::{Arg, CallReply, Event};
use rbx_reflection::ReflectionDatabase;

use crate::humanoid::{flag, number};

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
    Pending(Receiver<Result<Info, String>>),
    Ready(Info),
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
    loads: HashMap<u64, Load>,
    clocks: HashMap<Ref, Clock>,
    known: BTreeSet<Ref>,
    /// Signals a write queued, for the next step.
    queued: Vec<Event>,
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

fn fetch(id: u64, fetcher: Option<Arc<dyn AssetFetcher>>) -> Result<Info, String> {
    let fetcher = fetcher.ok_or_else(|| format!("video {id} is not cached and we are offline"))?;
    let bytes = fetcher.fetch_id(id).map_err(|e| e.to_string())?;
    if let Ok(cache) = AssetCache::new(None) {
        let _ = cache.put_id(id, &bytes);
    }
    webm_info(&bytes).ok_or_else(|| format!("video {id} is not a WebM file"))
}

impl Videos {
    pub(crate) fn watch(&mut self, r: Ref) {
        self.known.insert(r);
    }

    /// The video `r` shows, asking for it the first time; `Err` once it
    /// failed to load.
    fn info(&mut self, dom: &WeakDom, r: Ref) -> Option<Result<Info, ()>> {
        let id = content_id(&video_uri(dom, r))?;
        if let Some(Load::Pending(receiver)) = self.loads.get(&id) {
            if let Ok(done) = receiver.try_recv() {
                self.loads
                    .insert(id, done.map_or(Load::Failed, Load::Ready));
            }
        }
        if !self.loads.contains_key(&id) {
            let cached = AssetCache::new(None).ok().and_then(|c| c.get_id(id));
            let load = match cached.as_deref().and_then(webm_info) {
                Some(info) => Load::Ready(info),
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
            Load::Ready(info) => Some(Ok(*info)),
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
                    Ok(Ok(info)) => Load::Ready(info),
                    _ => Load::Failed,
                };
                self.loads.insert(id, load);
            }
        }
        matches!(self.info(dom, r), Some(Ok(_)))
    }

    pub(crate) fn property(&mut self, dom: &WeakDom, r: Ref, key: &str) -> Option<Arg> {
        self.watch(r);
        let info = self.info(dom, r).and_then(Result::ok);
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

    /// One frame: every playing video's clock, and what that fired.
    pub(crate) fn step(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        dt: f64,
    ) -> Vec<Event> {
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
            let Some(Ok(info)) = info else {
                continue;
            };
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
