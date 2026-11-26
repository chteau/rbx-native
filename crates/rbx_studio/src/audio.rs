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

pub(crate) use rbx_sim::audio::{decode, Clip};

/// What an output pulls samples from: interleaved `f32` frames at its own
/// channel count and rate.
pub(crate) trait Source: Send + 'static {
    fn channels(&self) -> u16;
    fn rate(&self) -> u32;
    /// Fills `out` (whole frames) and says how many frames it wrote; 0 is
    /// the end.
    fn fill(&mut self, out: &mut [f32]) -> usize;
}

/// What the preview's output thread reads: the clip, where it is in it,
/// and how loud.
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

impl Source for Cursor {
    fn channels(&self) -> u16 {
        self.clip.channels
    }
    fn rate(&self) -> u32 {
        self.clip.rate
    }
    fn fill(&mut self, out: &mut [f32]) -> usize {
        let channels = self.clip.channels as usize;
        let frames = out.len() / channels;
        let mut n = 0;
        for (slot, sample) in out.iter_mut().zip(self.take(frames)) {
            *slot = sample;
            n += 1;
        }
        n / channels
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
            if let Err(err) = output::run(cursor, stopped, "sound preview") {
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

/// Plays `source` on the default output until it ends or `stopped`
/// fires; an error when there is no output to play through.
pub(crate) fn output_run(
    source: impl Source,
    stopped: oneshot::Receiver<()>,
    name: &'static str,
) -> Result<(), String> {
    output::run(source, stopped, name)
}

#[cfg(target_os = "linux")]
mod output {
    use futures::channel::oneshot::Receiver;
    use futures::future::{select, Either};
    use pulseaudio::{protocol, AsPlaybackSource, Client};

    use super::Source;

    pub(super) fn run(
        mut source: impl Source,
        stopped: Receiver<()>,
        _name: &'static str,
    ) -> Result<(), String> {
        let client = Client::from_env(c"rbxstudio")
            .map_err(|err| format!("no sound server to play through: {err}"))?;
        let (channels, rate) = (source.channels(), source.rate());
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
            // `PULSE_SINK` picks the sink as it does for libpulse clients
            // (a null sink to record from, say).
            sink_name: Some(
                std::env::var("PULSE_SINK")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .and_then(|s| std::ffi::CString::new(s).ok())
                    .unwrap_or_else(|| protocol::DEFAULT_SINK.to_owned()),
            ),
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
        let mut samples = Vec::new();
        let source = move |buf: &mut [u8]| {
            let frames = buf.len() / frame_bytes;
            samples.resize(frames * channels as usize, 0f32);
            let n = source.fill(&mut samples) * channels as usize;
            for (slot, sample) in buf.as_chunks_mut::<4>().0.iter_mut().zip(&samples[..n]) {
                *slot = sample.to_le_bytes();
            }
            n * 4
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

    use super::Source;

    pub(super) fn run(
        source: impl Source,
        stopped: Receiver<()>,
        name: &'static str,
    ) -> Result<(), String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no audio output device")?;
        let config = device
            .default_output_config()
            .map_err(|err| format!("the audio output has no usable format: {err}"))?;
        let (done_tx, done_rx) = futures::channel::oneshot::channel();
        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => build::<f32>(&device, &config.into(), source, done_tx, name),
            cpal::SampleFormat::I16 => build::<i16>(&device, &config.into(), source, done_tx, name),
            cpal::SampleFormat::U16 => build::<u16>(&device, &config.into(), source, done_tx, name),
            other => return Err(format!("the audio output wants {other:?} samples")),
        }?;
        stream.play().map_err(|err| err.to_string())?;
        // Either the end or a stop; dropping the stream silences it.
        futures::executor::block_on(futures::future::select(done_rx, stopped));
        Ok(())
    }

    /// An output stream that walks the source at the device's rate
    /// (linear resampling, since the device rate rarely matches) and its
    /// channel count, and says when the source ends.
    fn build<T: SizedSample + FromSample<f32>>(
        device: &cpal::Device,
        config: &cpal::StreamConfig,
        mut source: impl Source,
        done: futures::channel::oneshot::Sender<()>,
        name: &'static str,
    ) -> Result<cpal::Stream, String> {
        let out_channels = config.channels as usize;
        let step = source.rate() as f64 / config.sample_rate.0 as f64;
        let channels = source.channels() as usize;
        let mut done = Some(done);
        // Two source frames, interpolated between; `phase` is how far along.
        let mut pair = vec![0f32; 2 * channels];
        let mut have = source.fill(&mut pair);
        let mut phase = 0f64;
        device
            .build_output_stream(
                config,
                move |out: &mut [T], _| {
                    for frame in out.chunks_mut(out_channels) {
                        if have < 2 {
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
                        while phase >= 1. && have == 2 {
                            phase -= 1.;
                            pair.copy_within(channels.., 0);
                            have = 1 + source.fill(&mut pair[channels..]);
                        }
                    }
                },
                move |err| eprintln!("{name}: {err}"),
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
}
