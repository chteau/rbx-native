//! Plays a sound asset for Properties' preview on a `Sound` or an
//! `AudioPlayer`: [`decode`] turns the asset's bytes (OGG Vorbis, MP3, WAV
//! or FLAC, what Roblox accepts) into samples, and [`play`] sends them to
//! the default output from a given frame, on a thread of its own, until
//! they run out or the [`Playback`] is stopped or dropped. Its
//! [`Playback::position`] is where a pause resumes from.
//!
//! Linux talks to the PulseAudio server (PipeWire answers as one) over its
//! own protocol, so there is no ALSA library to build against; Windows and
//! macOS go through `cpal` (WASAPI, Core Audio).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use futures::channel::oneshot;

/// Decoded audio: interleaved `f32` frames, one or two channels.
pub(crate) struct Clip {
    pub(crate) samples: Vec<f32>,
    pub(crate) channels: u16,
    pub(crate) rate: u32,
}

impl Clip {
    pub(crate) fn frames(&self) -> usize {
        self.samples.len() / self.channels as usize
    }

    #[cfg(test)]
    fn seconds(&self) -> f32 {
        self.frames() as f32 / self.rate as f32
    }
}

/// What the output thread reads: the clip, where it is in it, and how loud.
pub(crate) struct Cursor {
    clip: Arc<Clip>,
    frame: Arc<AtomicUsize>,
    gain: f32,
}

impl Cursor {
    /// The next `frames` frames at the gain (fewer at the end), advancing.
    fn take(&self, frames: usize) -> impl Iterator<Item = f32> + '_ {
        let channels = self.clip.channels as usize;
        let start = self.frame.load(Ordering::Acquire).min(self.clip.frames());
        let end = (start + frames).min(self.clip.frames());
        self.frame.store(end, Ordering::Release);
        self.clip.samples[start * channels..end * channels]
            .iter()
            .map(|s| (s * self.gain).clamp(-1., 1.))
    }
}

/// Decodes a whole sound asset. More than two channels fold down to stereo.
pub(crate) fn decode(bytes: Vec<u8>) -> Result<Clip, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes)), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &Hint::new(),
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|_| "not an audio file this editor can play".to_string())?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or("the file holds no audio track")?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|err| format!("unsupported audio codec: {err}"))?;

    let mut samples = Vec::new();
    let mut spec = None;
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(_)) | Err(Error::ResetRequired) => break,
            Err(err) => return Err(err.to_string()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A damaged packet is skipped, as players do.
            Err(Error::DecodeError(_)) => continue,
            Err(err) => return Err(err.to_string()),
        };
        let this = *decoded.spec();
        spec.get_or_insert(this);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, this);
        buffer.copy_interleaved_ref(decoded);
        fold_into(&mut samples, buffer.samples(), this.channels.count());
    }
    let spec = spec.ok_or("the file holds no audio")?;
    Ok(Clip {
        samples,
        channels: spec.channels.count().min(2) as u16,
        rate: spec.rate,
    })
}

/// Appends `frames` (interleaved, `channels` wide), folding anything past
/// stereo into the first two channels.
fn fold_into(out: &mut Vec<f32>, frames: &[f32], channels: usize) {
    if channels <= 2 {
        out.extend_from_slice(frames);
        return;
    }
    for frame in frames.chunks_exact(channels) {
        let (mut left, mut right) = (0., 0.);
        for (i, sample) in frame.iter().enumerate() {
            if i % 2 == 0 {
                left += sample;
            } else {
                right += sample;
            }
        }
        let half = channels as f32 / 2.;
        out.push(left / half.ceil());
        out.push(right / half.floor());
    }
}

/// A sound playing. Stopping (or dropping) it cuts it off.
pub(crate) struct Playback {
    stop: Option<oneshot::Sender<()>>,
    frame: Arc<AtomicUsize>,
    finished: Arc<AtomicBool>,
    error: Arc<Mutex<Option<String>>>,
}

impl Playback {
    /// The frame the output has reached (handed to the sound server, which
    /// keeps about a tenth of a second queued).
    pub(crate) fn position(&self) -> usize {
        self.frame.load(Ordering::Acquire)
    }

    /// Played to its end, or failed.
    pub(crate) fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// Why the output failed, once it has.
    pub(crate) fn error(&self) -> Option<String> {
        self.error.lock().ok()?.clone()
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// Starts `clip` at frame `from` on the default output at `volume`
/// (Roblox's scale: 1 is the file as it is, louder is clipped).
pub(crate) fn play(clip: Arc<Clip>, from: usize, volume: f32) -> Playback {
    let frame = Arc::new(AtomicUsize::new(from));
    let cursor = Cursor {
        clip,
        frame: frame.clone(),
        gain: volume.max(0.),
    };
    let (stop, stopped) = oneshot::channel();
    let finished = Arc::new(AtomicBool::new(false));
    let error = Arc::new(Mutex::new(None));
    let (done, failed) = (finished.clone(), error.clone());
    std::thread::Builder::new()
        .name("sound-preview".into())
        .spawn(move || {
            if let Err(err) = output::run(cursor, stopped) {
                if let Ok(mut slot) = failed.lock() {
                    *slot = Some(err);
                }
            }
            done.store(true, Ordering::Release);
        })
        .expect("spawning the sound preview thread");
    Playback {
        stop: Some(stop),
        frame,
        finished,
        error,
    }
}

#[cfg(target_os = "linux")]
mod output {
    use futures::channel::oneshot::Receiver;
    use futures::future::{select, Either};
    use pulseaudio::{protocol, AsPlaybackSource, Client};

    use super::Cursor;

    pub(super) fn run(cursor: Cursor, stopped: Receiver<()>) -> Result<(), String> {
        let client = Client::from_env(c"rbxstudio")
            .map_err(|err| format!("no sound server to play through: {err}"))?;
        let (channels, rate) = (cursor.clip.channels, cursor.clip.rate);
        let params = protocol::PlaybackStreamParams {
            sample_spec: protocol::SampleSpec {
                format: protocol::SampleFormat::Float32Le,
                channels: channels as u8,
                sample_rate: rate,
            },
            channel_map: if channels == 1 {
                protocol::ChannelMap::mono()
            } else {
                protocol::ChannelMap::stereo()
            },
            cvolume: Some(protocol::ChannelVolume::norm(channels as u8)),
            sink_name: Some(protocol::DEFAULT_SINK.to_owned()),
            // A tenth of a second queued, not the server's default two: a
            // pause stops close to what was heard.
            buffer_attr: protocol::stream::BufferAttr {
                target_length: rate * channels as u32 * 4 / 10,
                ..Default::default()
            },
            flags: protocol::stream::StreamFlags {
                adjust_latency: true,
                ..Default::default()
            },
            ..Default::default()
        };
        let frame_bytes = channels as usize * 4;
        let source = move |buf: &mut [u8]| {
            let (mut n, frames) = (0, buf.len() / frame_bytes);
            for (slot, sample) in buf
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(cursor.take(frames))
            {
                *slot = sample.to_le_bytes();
                n += 4;
            }
            n
        };
        futures::executor::block_on(async move {
            let stream = client
                .create_playback_stream(params, source.as_playback_source())
                .await
                .map_err(|err| format!("the sound server refused the stream: {err}"))?;
            match select(Box::pin(stream.play_all()), stopped).await {
                Either::Left((played, _)) => played.map_err(|err| err.to_string())?,
                // Stopped: drop what the server still has queued at once.
                Either::Right(_) => {
                    let _ = stream.flush().await;
                }
            }
            let _ = stream.delete().await;
            Ok(())
        })
    }
}

#[cfg(not(target_os = "linux"))]
mod output {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{FromSample, SizedSample};
    use futures::channel::oneshot::Receiver;

    use super::Cursor;

    pub(super) fn run(cursor: Cursor, stopped: Receiver<()>) -> Result<(), String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no audio output device")?;
        let config = device
            .default_output_config()
            .map_err(|err| format!("the audio output has no usable format: {err}"))?;
        let (done_tx, done_rx) = futures::channel::oneshot::channel();
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config.into(), cursor, done_tx),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config.into(), cursor, done_tx),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config.into(), cursor, done_tx),
            other => return Err(format!("the audio output wants {other:?} samples")),
        }?;
        stream.play().map_err(|err| err.to_string())?;
        // Either the end or a stop; dropping the stream silences it.
        futures::executor::block_on(futures::future::select(done_rx, stopped));
        Ok(())
    }

    /// An output stream that walks the clip at the device's rate (linear
    /// resampling, since the device rate rarely matches the file's) and its
    /// channel count, and says when it reaches the end.
    fn build<T: SizedSample + FromSample<f32>>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        cursor: Cursor,
        done: futures::channel::oneshot::Sender<()>,
    ) -> Result<cpal::Stream, String> {
        let out_channels = config.channels as usize;
        let step = cursor.clip.rate as f64 / config.sample_rate.0 as f64;
        let channels = cursor.clip.channels as usize;
        let mut done = Some(done);
        // Two source frames, interpolated between; `phase` is how far along.
        let mut pair: Vec<f32> = cursor.take(2).collect();
        let mut phase = 0f64;
        device
            .build_output_stream(
                config,
                move |out: &mut [T], _| {
                    for frame in out.chunks_mut(out_channels) {
                        if pair.len() < 2 * channels {
                            frame.fill(T::from_sample(0.));
                            if let Some(done) = done.take() {
                                let _ = done.send(());
                            }
                            continue;
                        }
                        let t = phase as f32;
                        for (channel, slot) in frame.iter_mut().enumerate() {
                            let c = channel.min(channels - 1);
                            let (a, b) = (pair[c], pair[channels + c]);
                            *slot = T::from_sample(a + (b - a) * t);
                        }
                        phase += step;
                        while phase >= 1. && pair.len() == 2 * channels {
                            phase -= 1.;
                            pair.drain(..channels);
                            pair.extend(cursor.take(1));
                        }
                    }
                },
                |err| eprintln!("sound preview: {err}"),
                None,
            )
            .map_err(|err| format!("the audio output refused the stream: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-second 440 Hz mono WAV, built by hand.
    fn tone_wav() -> Vec<u8> {
        let rate = 8000u32;
        let samples: Vec<i16> = (0..rate)
            .map(|i| {
                ((i as f32 / rate as f32 * 440. * std::f32::consts::TAU).sin() * 16000.) as i16
            })
            .collect();
        let data = (samples.len() * 2) as u32;
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data.to_le_bytes());
        for s in samples {
            wav.extend_from_slice(&s.to_le_bytes());
        }
        wav
    }

    #[test]
    fn a_wav_decodes_to_its_length_and_rate() {
        let clip = decode(tone_wav()).unwrap();
        assert_eq!((clip.channels, clip.rate), (1, 8000));
        assert!((clip.seconds() - 1.).abs() < 0.01);
        let peak = clip.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!((peak - 16000. / 32768.).abs() < 0.01);
    }

    #[test]
    fn something_else_is_refused() {
        assert!(decode(b"not audio at all".to_vec()).is_err());
    }

    #[test]
    fn a_cursor_walks_from_where_it_starts_at_the_gain() {
        let clip = Arc::new(Clip {
            samples: vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6],
            channels: 2,
            rate: 10,
        });
        let cursor = Cursor {
            clip,
            frame: Arc::new(AtomicUsize::new(1)),
            gain: 2.,
        };
        let got: Vec<f32> = cursor.take(5).collect();
        assert_eq!(got, vec![0.6, 0.8, 1., 1.]);
        assert_eq!(cursor.frame.load(Ordering::Acquire), 3);
        assert_eq!(cursor.take(1).count(), 0);
    }

    #[test]
    fn surround_folds_to_stereo() {
        let mut out = Vec::new();
        fold_into(&mut out, &[1., 0., 1., 0., 1., 0.], 6);
        assert_eq!(out, vec![1., 0.]);
    }
}
