//! A video's picture and sound. [`demux`] splits a WebM file into its VP8
//! or VP9 frames and decodes its Vorbis or Opus track whole; a [`Picture`]
//! decodes on its own thread up to the time it is asked to show, scales the
//! frame down to the size it's drawn at and publishes it as an editable
//! image (`rbx_dom::editable`), which the renderer draws like a script's
//! `EditableImage`. Nothing is decoded while the time asked for stays on
//! the same frame, so a paused or hidden video costs nothing.

use std::io::Cursor;
use std::sync::{Arc, Condvar, Mutex};

use matroska_demuxer::{Frame, MatroskaFile};
use rbx_dom::editable::{self, lock, Editable, Pixels};
use rbx_dom::Ref;

use super::{webm_info, Info};
use crate::audio::Clip;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Codec {
    Vp8,
    Vp9,
}

struct Packet {
    /// When it shows, in seconds.
    time: f64,
    key: bool,
    data: Vec<u8>,
}

/// A loaded video.
pub(crate) struct Movie {
    pub info: Info,
    codec: Option<Codec>,
    frames: Vec<Packet>,
    pub audio: Option<Arc<Clip>>,
}

impl Movie {
    /// The frame showing at `time`.
    fn frame_at(&self, time: f64) -> usize {
        self.frames
            .partition_point(|p| p.time <= time)
            .saturating_sub(1)
    }

    /// The last keyframe at or before `frame`, where decoding can start.
    fn key_before(&self, frame: usize) -> usize {
        (0..=frame).rev().find(|i| self.frames[*i].key).unwrap_or(0)
    }
}

/// Reads a WebM file. A codec other than VP8/VP9 gives a movie with no
/// picture; a missing or undecodable sound track gives none.
pub(crate) fn demux(bytes: &[u8]) -> Result<Movie, String> {
    let info = webm_info(bytes).ok_or("not a WebM file")?;
    let mut file = MatroskaFile::open(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let scale = file.info().timestamp_scale().get() as f64 / 1e9;
    let video = file.tracks().iter().find(|t| t.video().is_some());
    let codec = video.and_then(|t| match t.codec_id() {
        "V_VP8" => Some(Codec::Vp8),
        "V_VP9" => Some(Codec::Vp9),
        _ => None,
    });
    let track = video.map(|t| t.track_number().get());
    let mut frames = Vec::new();
    let mut frame = Frame::default();
    while track.is_some() && file.next_frame(&mut frame).map_err(|e| e.to_string())? {
        if Some(frame.track) != track {
            continue;
        }
        let key = match codec {
            // A VP8 frame tag's lowest bit is 0 on a keyframe.
            Some(Codec::Vp8) => frame.data.first().is_some_and(|b| b & 1 == 0),
            _ => frame.is_keyframe.unwrap_or(frames.is_empty()),
        };
        frames.push(Packet {
            time: frame.timestamp as f64 * scale,
            key,
            data: std::mem::take(&mut frame.data),
        });
    }
    frames.sort_by(|a, b| a.time.total_cmp(&b.time));
    Ok(Movie {
        info,
        codec,
        frames,
        audio: crate::audio::decode(bytes.to_vec()).ok().map(Arc::new),
    })
}

/// A decoded frame: Y, U and V planes, 8 bits a sample (`wide` for 16).
struct Yuv {
    width: usize,
    height: usize,
    planes: [Vec<u8>; 3],
    strides: [usize; 3],
    /// Chroma subsampling, as shifts: (x, y).
    shift: (u32, u32),
    wide: bool,
    /// For a wide frame, how far to shift a sample down to 8 bits.
    depth: u32,
}

enum Decoder {
    Vp8(Box<vp8_decoder::Decoder>),
    Vp9(Box<rusty_vp9::Vp9Decoder>),
}

impl Decoder {
    fn new(codec: Codec) -> Decoder {
        match codec {
            Codec::Vp8 => Decoder::Vp8(Box::new(vp8_decoder::Decoder::new())),
            Codec::Vp9 => Decoder::Vp9(Box::new(rusty_vp9::Vp9Decoder::new())),
        }
    }

    /// The picture `data` shows, if it shows one.
    fn decode(&mut self, data: &[u8]) -> Option<Yuv> {
        match self {
            Decoder::Vp8(d) => {
                let p = d.decode(data).ok()??;
                Some(Yuv {
                    width: p.width as usize,
                    height: p.height as usize,
                    planes: [p.y.to_vec(), p.u.to_vec(), p.v.to_vec()],
                    strides: [p.y_stride, p.uv_stride, p.uv_stride],
                    shift: (1, 1),
                    wide: false,
                    depth: 0,
                })
            }
            Decoder::Vp9(d) => {
                d.push(data, None).ok()?;
                let mut last = None;
                while let Ok(frame) = d.next_frame() {
                    last = Some(frame);
                }
                let f = last?;
                let [y, u, v]: [Vec<u8>; 3] = f.planes.try_into().ok()?;
                Some(Yuv {
                    width: f.width as usize,
                    height: f.height as usize,
                    planes: [y, u, v],
                    strides: [f.strides[0], f.strides[1], f.strides[2]],
                    shift: (f.subsampling_x, f.subsampling_y),
                    wide: f.bit_depth > 8,
                    depth: f.bit_depth.saturating_sub(8),
                })
            }
        }
    }
}

impl Yuv {
    fn sample(&self, plane: usize, x: usize, y: usize) -> i32 {
        let row = &self.planes[plane][y * self.strides[plane]..];
        if self.wide {
            i32::from(u16::from_le_bytes([row[2 * x], row[2 * x + 1]]) >> self.depth)
        } else {
            i32::from(row[x])
        }
    }

    /// RGBA pixels at `width` by `height`, sampling the nearest source
    /// pixel (BT.601, studio range, as VP8/VP9 streams are by default).
    fn rgba(&self, width: usize, height: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(width * height * 4);
        let (sx, sy) = (self.shift.0 as usize, self.shift.1 as usize);
        for oy in 0..height {
            let y = ((2 * oy + 1) * self.height / (2 * height)).min(self.height - 1);
            for ox in 0..width {
                let x = ((2 * ox + 1) * self.width / (2 * width)).min(self.width - 1);
                let c = (self.sample(0, x, y) - 16) * 298;
                let d = self.sample(1, x >> sx, y >> sy) - 128;
                let e = self.sample(2, x >> sx, y >> sy) - 128;
                let clamp = |v: i32| ((v + 128) >> 8).clamp(0, 255) as u8;
                out.extend_from_slice(&[
                    clamp(c + 409 * e),
                    clamp(c - 100 * d - 208 * e),
                    clamp(c + 516 * d),
                    255,
                ]);
            }
        }
        out
    }
}

#[derive(Default)]
struct Request {
    /// The time to show and the size to draw at, not yet taken.
    want: Option<(f64, [u32; 2])>,
    stop: bool,
}

#[derive(Default)]
struct Shared {
    request: Mutex<Request>,
    wake: Condvar,
}

/// One playing (or paused) video's picture, decoded on its own thread.
pub(crate) struct Picture {
    /// The editable image it publishes, for a `Content::Object` binding.
    pub object: Ref,
    pub movie: Arc<Movie>,
    shared: Arc<Shared>,
}

impl Picture {
    /// `None` for a movie with nothing to draw.
    pub(crate) fn new(movie: Arc<Movie>) -> Option<Picture> {
        let codec = movie.codec?;
        if movie.frames.is_empty() {
            return None;
        }
        let pixels = Arc::new(Mutex::new(Pixels::default()));
        let object = editable::publish(Editable::Image(pixels.clone()));
        let shared = Arc::new(Shared::default());
        let (worker, thread_movie) = (shared.clone(), movie.clone());
        let spawned = std::thread::Builder::new()
            .name("video-decode".into())
            .spawn(move || decode(&thread_movie, codec, &worker, &pixels));
        if spawned.is_err() {
            editable::withdraw(object);
            return None;
        }
        Some(Picture {
            object,
            movie,
            shared,
        })
    }

    /// Asks for the frame at `time`, drawn `size` pixels big (zero: the
    /// video's own size; never bigger than it).
    pub(crate) fn show(&self, time: f64, size: [f32; 2]) {
        let info = self.movie.info;
        let fit = |want: f32, own: f32| {
            let own = own.max(1.0);
            (if want >= 1.0 { want.min(own) } else { own }).round() as u32
        };
        let size = [fit(size[0], info.width), fit(size[1], info.height)];
        lock(&self.shared.request).want = Some((time, size));
        self.shared.wake.notify_one();
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        lock(&self.shared.request).stop = true;
        self.shared.wake.notify_one();
        editable::withdraw(self.object);
    }
}

/// The decoding thread: waits for a request, decodes forward to it (from
/// the keyframe before it when that's quicker, or when it goes back) and
/// publishes the frame.
fn decode(movie: &Movie, codec: Codec, shared: &Shared, pixels: &Mutex<Pixels>) {
    let mut decoder = Decoder::new(codec);
    let mut next = 0;
    let mut last: Option<Yuv> = None;
    let mut shown = None;
    loop {
        let (time, size) = {
            let mut request = lock(&shared.request);
            loop {
                if request.stop {
                    return;
                }
                if let Some(want) = request.want.take() {
                    break want;
                }
                request = shared
                    .wake
                    .wait(request)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        let target = movie.frame_at(time);
        if shown == Some((target, size)) {
            continue;
        }
        let key = movie.key_before(target);
        if target + 1 < next || key > next || next == 0 {
            decoder = Decoder::new(codec);
            next = key;
        }
        while next <= target {
            if let Some(yuv) = decoder.decode(&movie.frames[next].data) {
                last = Some(yuv);
            }
            next += 1;
        }
        let Some(yuv) = &last else {
            continue;
        };
        let rgba = yuv.rgba(size[0] as usize, size[1] as usize);
        {
            let mut out = lock(pixels);
            out.width = size[0];
            out.height = size[1];
            out.rgba = rgba;
            out.version += 1;
        }
        editable::touch();
        shown = Some((target, size));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLIP: &[u8] = include_bytes!("../../tests/data/clip.webm");

    fn decoded(movie: &Movie, upto: usize) -> Yuv {
        let mut decoder = Decoder::new(movie.codec.unwrap());
        let mut last = None;
        for packet in &movie.frames[..=upto] {
            last = decoder.decode(&packet.data).or(last);
        }
        last.unwrap()
    }

    #[test]
    fn a_webm_clip_demuxes_with_its_sound() {
        let movie = demux(CLIP).unwrap();
        assert_eq!(movie.codec, Some(Codec::Vp8));
        assert_eq!((movie.info.width, movie.info.height), (64.0, 48.0));
        assert!((movie.info.seconds - 1.0).abs() < 0.05, "{:?}", movie.info);
        assert_eq!(movie.frames.len(), 10);
        assert!(movie.frames[0].key);
        let audio = movie.audio.as_ref().expect("the Opus track decodes");
        assert_eq!(audio.rate, 48_000);
        assert!((audio.seconds() - 1.0).abs() < 0.05, "{}", audio.seconds());
    }

    #[test]
    fn frames_are_picked_by_time() {
        let movie = demux(CLIP).unwrap();
        assert_eq!(movie.frame_at(0.0), 0);
        assert_eq!(movie.frame_at(0.15), 1);
        assert_eq!(movie.frame_at(5.0), 9);
        // The cut to blue is a keyframe.
        assert_eq!(movie.key_before(9), 5);
        assert_eq!(movie.key_before(4), 0);
    }

    #[test]
    fn the_picture_is_the_clip_colour_scaled_down() {
        // The clip is a red frame that turns blue halfway.
        let movie = demux(CLIP).unwrap();
        let red = decoded(&movie, 0).rgba(8, 6);
        assert_eq!(red.len(), 8 * 6 * 4);
        assert!(
            red[0] > 200 && red[1] < 60 && red[2] < 60,
            "{:?}",
            &red[..4]
        );
        let blue = decoded(&movie, 9).rgba(2, 2);
        assert!(blue[2] > 200 && blue[0] < 60, "{:?}", &blue[..4]);
    }

    #[test]
    fn vp9_decodes_too() {
        let movie = demux(include_bytes!("../../tests/data/clip_vp9.webm")).unwrap();
        assert_eq!(movie.codec, Some(Codec::Vp9));
        assert!(movie.audio.is_none());
        let green = decoded(&movie, movie.frames.len() - 1).rgba(4, 4);
        assert!(
            green[1] > 100 && green[0] < 60 && green[2] < 60,
            "{:?}",
            &green[..4]
        );
    }

    #[test]
    fn a_picture_publishes_the_frame_it_is_asked_for() {
        let movie = Arc::new(demux(CLIP).unwrap());
        let picture = Picture::new(movie).unwrap();
        picture.show(0.9, [16.0, 0.0]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let pixels = loop {
            if let Some(Editable::Image(p)) = editable::get(picture.object) {
                let p = lock(&p).clone();
                if p.version > 0 {
                    break p;
                }
            }
            assert!(std::time::Instant::now() < deadline, "no frame");
            std::thread::yield_now();
        };
        assert_eq!((pixels.width, pixels.height), (16, 48));
        assert!(pixels.rgba[2] > 200, "{:?}", &pixels.rgba[..4]);
        let object = picture.object;
        drop(picture);
        assert!(editable::get(object).is_none());
    }
}
