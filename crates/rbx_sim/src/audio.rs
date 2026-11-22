//! Sound assets as samples, and how loud a playing sound is where the
//! listener stands: [`decode`] turns an asset's bytes (OGG Vorbis, MP3, WAV
//! or FLAC, what Roblox accepts) into samples, [`rolloff`] is the
//! distance attenuation of `Sound.RollOffMode`, and [`Voice`] is what an
//! output mixes for one playing sound.

use std::sync::Arc;

/// Decoded audio: interleaved `f32` frames, one or two channels.
pub struct Clip {
    pub samples: Vec<f32>,
    pub channels: u16,
    pub rate: u32,
}

impl Clip {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    /// Its length in seconds (`Sound.TimeLength`).
    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }

    /// The stereo frame `seconds` in, between samples (a mono clip on
    /// both sides).
    pub fn frame_at(&self, seconds: f64) -> [f32; 2] {
        let (channels, frames) = (self.channels.max(1) as usize, self.frames());
        if frames == 0 {
            return [0.0; 2];
        }
        let at = seconds.max(0.0) * f64::from(self.rate);
        let i = (at as usize).min(frames - 1);
        let j = (i + 1).min(frames - 1);
        let t = (at - i as f64) as f32;
        let sample = |c: usize| {
            let (a, b) = (
                self.samples[i * channels + c],
                self.samples[j * channels + c],
            );
            a + (b - a) * t
        };
        if channels == 1 {
            [sample(0); 2]
        } else {
            [sample(0), sample(1)]
        }
    }
}

/// One playing sound as the listener hears it this frame.
#[derive(Clone)]
pub struct Voice {
    /// The `Sound` instance, so an output keeps one stream per sound.
    pub sound: rbx_dom::Ref,
    pub clip: Arc<Clip>,
    /// Where the sound is, in seconds (`TimePosition`).
    pub position: f64,
    /// `PlaybackSpeed` (1 is the file's own rate).
    pub speed: f64,
    /// Volume after the SoundGroups and the distance: 1 is the file as it
    /// is.
    pub gain: f32,
    /// -1 (left) to 1 (right); 0 for a sound with no place in the world.
    pub pan: f32,
    pub looped: bool,
}

/// Decodes a whole sound asset. More than two channels fold down to stereo.
pub fn decode(bytes: Vec<u8>) -> Result<Clip, String> {
    use symphonia::core::audio::SampleBuffer;
    use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL, CODEC_TYPE_OPUS};
    use symphonia::core::errors::Error;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    let ogg_length = ogg_frames(&bytes);
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
    // The container's own length: a Vorbis stream's last packet decodes
    // past it (3.087 s for a sound Roblox and ffprobe call 3.072 s).
    let length = ogg_length.or(track.codec_params.n_frames);
    if track.codec_params.codec == CODEC_TYPE_OPUS {
        let head = track.codec_params.extra_data.clone().unwrap_or_default();
        return opus(format, track_id, &head);
    }
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
    let channels = spec.channels.count().min(2);
    if let Some(frames) = length {
        samples.truncate(frames as usize * channels);
    }
    Ok(Clip {
        samples,
        channels: channels as u16,
        rate: spec.rate,
    })
}

/// An Opus track (a WebM video's sound), which symphonia demuxes but
/// doesn't decode. `head` is its `OpusHead`: the channel count and the
/// pre-skip, the encoder's priming samples dropped from the start.
fn opus(
    mut format: Box<dyn symphonia::core::formats::FormatReader>,
    track: u32,
    head: &[u8],
) -> Result<Clip, String> {
    const RATE: u32 = 48_000;
    /// The longest Opus packet, 120 ms.
    const MAX: usize = 5760;
    let channels = head.get(9).map_or(2, |c| usize::from(*c).clamp(1, 2));
    let skip = head
        .get(10..12)
        .map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])));
    let mut decoder = opus_pure::OpusDecoder::new(RATE as i32, channels)
        .map_err(|err| format!("unsupported audio codec: {err:?}"))?;
    let mut buffer = vec![0.0; MAX * channels];
    let mut samples = Vec::new();
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track {
            continue;
        }
        // A damaged packet is skipped, as players do.
        if let Ok(frames) = decoder.decode(&packet.data, MAX, &mut buffer) {
            samples.extend_from_slice(&buffer[..frames * channels]);
        }
    }
    samples.drain(..(skip * channels).min(samples.len()));
    if samples.is_empty() {
        return Err("the file holds no audio".into());
    }
    Ok(Clip {
        samples,
        channels: channels as u16,
        rate: RATE,
    })
}

/// An Ogg stream's length in frames: the granule position of its last
/// page, which for Vorbis counts the samples to play.
fn ogg_frames(bytes: &[u8]) -> Option<u64> {
    if !bytes.starts_with(b"OggS") {
        return None;
    }
    let last = bytes.windows(4).rposition(|w| w == b"OggS")?;
    let granule = bytes.get(last + 6..last + 14)?;
    let frames = u64::from_le_bytes(granule.try_into().ok()?);
    (frames > 0 && frames != u64::MAX).then_some(frames)
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

/// `Enum.RollOffMode`, by its item value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RollOff {
    Inverse,
    Linear,
    LinearSquare,
    InverseTapered,
}

impl RollOff {
    pub fn from_value(value: u32) -> Self {
        match value {
            1 => RollOff::Linear,
            2 => RollOff::LinearSquare,
            3 => RollOff::InverseTapered,
            _ => RollOff::Inverse,
        }
    }
}

/// The gain of a sound `distance` studs from the listener, from the creator
/// docs' description of each mode: full volume inside `min`; past it
/// Inverse falls as `min / distance`, Linear straight to nothing at `max`,
/// LinearSquare as the square of that, and InverseTapered follows the
/// inverse curve near `min` and the squared line near `max` (the lower of
/// the two). Nothing past `max`. Not measured against the real client.
pub fn rolloff(mode: RollOff, distance: f64, min: f64, max: f64) -> f64 {
    let min = min.max(0.0);
    let max = max.max(min);
    if distance <= min {
        return 1.0;
    }
    if distance >= max {
        return 0.0;
    }
    let inverse = if distance > 0.0 { min / distance } else { 1.0 };
    let line = if max > min {
        (max - distance) / (max - min)
    } else {
        0.0
    };
    match mode {
        RollOff::Inverse => inverse,
        RollOff::Linear => line,
        RollOff::LinearSquare => line * line,
        RollOff::InverseTapered => inverse.min(line * line),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-second 440 Hz mono WAV, built by hand.
    pub(crate) fn tone_wav(rate: u32, seconds: f32) -> Vec<u8> {
        let count = (rate as f32 * seconds) as u32;
        let samples: Vec<i16> = (0..count)
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
        let clip = decode(tone_wav(8000, 1.0)).unwrap();
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
    fn surround_folds_to_stereo() {
        let mut out = Vec::new();
        fold_into(&mut out, &[1., 0., 1., 0., 1., 0.], 6);
        assert_eq!(out, vec![1., 0.]);
    }

    #[test]
    fn each_rolloff_mode_has_its_shape() {
        let (min, max) = (10.0, 110.0);
        for mode in [
            RollOff::Inverse,
            RollOff::Linear,
            RollOff::LinearSquare,
            RollOff::InverseTapered,
        ] {
            assert_eq!(rolloff(mode, 5.0, min, max), 1.0);
            assert_eq!(rolloff(mode, 200.0, min, max), 0.0);
        }
        assert!((rolloff(RollOff::Inverse, 20.0, min, max) - 0.5).abs() < 1e-9);
        assert!((rolloff(RollOff::Linear, 60.0, min, max) - 0.5).abs() < 1e-9);
        assert!((rolloff(RollOff::LinearSquare, 60.0, min, max) - 0.25).abs() < 1e-9);
        // Near min the inverse curve is the lower, near max the square.
        assert!((rolloff(RollOff::InverseTapered, 12.0, min, max) - 10.0 / 12.0).abs() < 1e-9);
        let near_max = rolloff(RollOff::InverseTapered, 100.0, min, max);
        assert!((near_max - 0.01).abs() < 1e-9);
    }
}
