//! `AudioTextToSpeech`'s voice, offline: Flite's American English voices
//! (`flite-rs`, pure Rust). Roblox's own voices aren't public, so `VoiceId`
//! picks between the two there are, a man's (`kal`) for odd ids and a
//! woman's (`slt`) for even ones. `Pitch` shifts the voice by semitones
//! and `Speed` speaks faster without changing the pitch.

use std::sync::{Arc, Mutex, OnceLock};

use flite_rs::Engine;

use crate::audio::Clip;

/// What a speaker says: its `Text`, `VoiceId`, `Pitch` and `Speed`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Line {
    pub text: String,
    pub voice: String,
    pub pitch: f32,
    pub speed: f32,
}

fn engine() -> &'static Mutex<Engine> {
    static ENGINE: OnceLock<Mutex<Engine>> = OnceLock::new();
    ENGINE.get_or_init(|| Mutex::new(Engine::new()))
}

/// `line` spoken, mono at the voice's own rate; `None` for no words.
pub(crate) fn speak(line: &Line) -> Option<Arc<Clip>> {
    if line.text.trim().is_empty() {
        return None;
    }
    let even = line.voice.trim().parse::<i64>().is_ok_and(|n| n % 2 == 0);
    let mut engine = rbx_dom::editable::lock(engine());
    engine.select_voice(if even { "slt" } else { "kal" });
    engine.set_f0_shift(2f32.powf(line.pitch.clamp(-24.0, 24.0) / 12.0));
    engine.set_duration_stretch(1.0 / line.speed.clamp(0.25, 4.0));
    let audio = engine.synthesize(&line.text);
    if audio.samples.is_empty() {
        return None;
    }
    Some(Arc::new(Clip {
        samples: audio
            .samples
            .iter()
            .map(|s| f32::from(*s) / 32768.0)
            .collect(),
        channels: 1,
        rate: audio.sample_rate,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(text: &str, voice: &str, speed: f32) -> Line {
        Line {
            text: text.into(),
            voice: voice.into(),
            pitch: 0.0,
            speed,
        }
    }

    #[test]
    fn words_become_sound() {
        let clip = speak(&line("Hello from the local run.", "1", 1.0)).unwrap();
        assert!(
            clip.seconds() > 0.8 && clip.seconds() < 4.0,
            "{}",
            clip.seconds()
        );
        let peak = clip.samples.iter().fold(0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.05, "{peak}");
        assert!(speak(&line("   ", "1", 1.0)).is_none());
    }

    #[test]
    fn speed_and_voice_change_it() {
        let normal = speak(&line("one two three four five", "1", 1.0)).unwrap();
        let fast = speak(&line("one two three four five", "1", 2.0)).unwrap();
        assert!(fast.seconds() < normal.seconds() * 0.75);
        let other = speak(&line("one two three four five", "2", 1.0)).unwrap();
        assert_ne!(other.rate, 0);
        assert_ne!(other.samples.len(), normal.samples.len());
    }
}
