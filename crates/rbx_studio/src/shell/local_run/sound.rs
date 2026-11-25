//! Play (local)'s sound: one stereo stream on the default output, mixing
//! the voices the viewed peer's sim hears (`Simulation::voices`). The sim
//! keeps each sound's clock; the mix follows it, resampling by
//! `PlaybackSpeed`, and jumps only when the two drift apart (a seek, a
//! restart). The Audio API's graph arrives already rendered
//! (`Simulation::take_audio`) and plays as it comes. Silent, and never
//! playing ahead, while the run is paused or the editor isn't playing. No
//! output device: nothing is heard and nothing fails.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use rbx_dom::Ref;
use rbx_sim::audio::{Clip, Voice};

use crate::audio::{self, Source};

const RATE: u32 = 48_000;
/// How far the mix may wander from the sim's clock before it jumps back.
const DRIFT: f64 = 0.12;
/// The most rendered graph audio kept waiting, in frames.
const BACKLOG: usize = RATE as usize / 4;

struct Track {
    clip: Arc<Clip>,
    /// Where in the clip, in seconds.
    position: f64,
    speed: f64,
    gain: f32,
    pan: f32,
    looped: bool,
}

#[derive(Default)]
struct Mix {
    tracks: HashMap<Ref, Track>,
    /// The graph's frames still to play.
    graph: VecDeque<[f32; 2]>,
    /// The play bar's volume, 0 to 1.
    volume: f32,
    muted: bool,
}

impl Mix {
    /// Adds the next `out.len() / 2` stereo frames of every track into
    /// `out` (which it clears first), moving each track along.
    fn render(&mut self, out: &mut [f32]) {
        out.fill(0.);
        if self.muted || self.volume <= 0. {
            return;
        }
        let master = self.volume;
        self.tracks.retain(|_, track| {
            let clip = &track.clip;
            if clip.frames() == 0 {
                return false;
            }
            let length = clip.seconds();
            let step = track.speed / f64::from(RATE);
            // Equal-power pan.
            let angle = (track.pan.clamp(-1., 1.) + 1.) * std::f32::consts::FRAC_PI_4;
            let (left, right) = (angle.cos(), angle.sin());
            let gain = track.gain * master * std::f32::consts::SQRT_2;
            for frame in out.as_chunks_mut::<2>().0 {
                if track.position >= length {
                    if !track.looped || length <= 0. {
                        return false;
                    }
                    track.position %= length;
                }
                let [l, r] = clip.frame_at(track.position);
                frame[0] += l * left * gain;
                frame[1] += r * right * gain;
                track.position += step;
            }
            true
        });
        let ready = self.graph.len().min(out.len() / 2);
        for (frame, [l, r]) in out
            .as_chunks_mut::<2>()
            .0
            .iter_mut()
            .zip(self.graph.drain(..ready))
        {
            frame[0] += l * master;
            frame[1] += r * master;
        }
        for s in out.iter_mut() {
            *s = s.clamp(-1., 1.);
        }
    }

    /// The sim's voices now: new ones join, gone ones stop, and a track
    /// whose clock wandered from the sim's jumps to it.
    fn update(&mut self, voices: Vec<Voice>) {
        let live: std::collections::HashSet<Ref> = voices.iter().map(|v| v.sound).collect();
        self.tracks.retain(|sound, _| live.contains(sound));
        for voice in voices {
            match self.tracks.get_mut(&voice.sound) {
                Some(track) if Arc::ptr_eq(&track.clip, &voice.clip) => {
                    if (track.position - voice.position).abs() > DRIFT {
                        track.position = voice.position;
                    }
                    track.speed = voice.speed;
                    track.gain = voice.gain;
                    track.pan = voice.pan;
                    track.looped = voice.looped;
                }
                _ => {
                    self.tracks.insert(
                        voice.sound,
                        Track {
                            clip: voice.clip,
                            position: voice.position,
                            speed: voice.speed,
                            gain: voice.gain,
                            pan: voice.pan,
                            looped: voice.looped,
                        },
                    );
                }
            }
        }
    }
}

struct Output(Arc<Mutex<Mix>>);

impl Source for Output {
    fn channels(&self) -> u16 {
        2
    }
    fn rate(&self) -> u32 {
        RATE
    }
    fn fill(&mut self, out: &mut [f32]) -> usize {
        match self.0.lock() {
            Ok(mut mix) => mix.render(out),
            Err(_) => out.fill(0.),
        }
        out.len() / 2
    }
}

/// The run's sound, alive as long as the run.
pub(crate) struct Mixer {
    mix: Arc<Mutex<Mix>>,
    _stop: futures::channel::oneshot::Sender<()>,
}

impl Mixer {
    pub(crate) fn new(volume: f32) -> Mixer {
        let mix = Arc::new(Mutex::new(Mix {
            volume,
            muted: true,
            ..Default::default()
        }));
        let (stop, stopped) = futures::channel::oneshot::channel();
        let output = Output(mix.clone());
        let _ = std::thread::Builder::new()
            .name("local-run-sound".into())
            .spawn(move || {
                // No sound server or device: the run stays silent.
                let _ = audio::output_run(output, stopped, "local run sound");
            });
        Mixer { mix, _stop: stop }
    }

    /// What the viewed peer hears this frame (its sounds, and what its
    /// audio graph rendered); `playing` false mutes the mix (paused, or
    /// the editor isn't the focus of a run).
    pub(crate) fn update(&self, voices: Vec<Voice>, graph: Vec<[f32; 2]>, playing: bool) {
        if let Ok(mut mix) = self.mix.lock() {
            mix.muted = !playing;
            if playing {
                mix.update(voices);
                mix.graph.extend(graph);
                let excess = mix.graph.len().saturating_sub(BACKLOG);
                mix.graph.drain(..excess);
            } else {
                mix.graph.clear();
            }
        }
    }

    pub(crate) fn set_volume(&self, volume: f32) {
        if let Ok(mut mix) = self.mix.lock() {
            mix.volume = volume.clamp(0., 1.);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(sound: Ref, clip: &Arc<Clip>, position: f64, pan: f32) -> Voice {
        Voice {
            sound,
            clip: clip.clone(),
            position,
            speed: 1.,
            gain: 1.,
            pan,
            looped: false,
        }
    }

    fn ones(seconds: f64) -> Arc<Clip> {
        Arc::new(Clip {
            samples: vec![0.5; (seconds * f64::from(RATE)) as usize],
            channels: 1,
            rate: RATE,
        })
    }

    #[test]
    fn a_centred_voice_plays_at_its_level_and_ends() {
        let clip = ones(0.01);
        let mut mix = Mix {
            volume: 1.,
            ..Default::default()
        };
        mix.update(vec![voice(Ref::new(1), &clip, 0., 0.)]);
        let mut out = vec![0.; 2 * 1000];
        mix.render(&mut out);
        assert!((out[0] - 0.5).abs() < 1e-3 && (out[1] - 0.5).abs() < 1e-3);
        assert_eq!(mix.tracks.len(), 0, "past its end it's gone");
        assert_eq!(out[2 * 999], 0.);
    }

    #[test]
    fn pan_and_mute_and_drift() {
        let clip = ones(1.);
        let sound = Ref::new(2);
        let mut mix = Mix {
            volume: 1.,
            ..Default::default()
        };
        mix.update(vec![voice(sound, &clip, 0., 1.)]);
        let mut out = vec![0.; 2 * 4];
        mix.render(&mut out);
        assert!(out[0].abs() < 1e-3 && out[1] > 0.6, "hard right: {out:?}");
        mix.update(vec![voice(sound, &clip, 0.05, 1.)]);
        assert!(mix.tracks[&sound].position < 0.01, "small drift kept");
        mix.update(vec![voice(sound, &clip, 0.5, 1.)]);
        assert!(
            (mix.tracks[&sound].position - 0.5).abs() < 1e-9,
            "a seek jumps"
        );
        mix.muted = true;
        mix.render(&mut out);
        assert!(out.iter().all(|s| *s == 0.));
    }
}
