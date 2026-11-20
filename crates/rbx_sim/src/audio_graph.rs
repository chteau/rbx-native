//! The Audio API's node graph as sound. A peer's [`Graph`] (players,
//! effects, emitters and the listeners hearing them, device outputs and
//! analyzers, and the wires between their pins) renders blocks of stereo
//! frames at [`RATE`] through a [`Renderer`], which keeps every effect's
//! state from block to block. Only `AudioFilter` (and so `GetGainAt`) is
//! fitted to a real server's numbers; the other effects follow the creator
//! docs' description of what they do.

use std::collections::{HashMap, HashSet};
use std::f32::consts::PI;
use std::sync::Arc;

use rbx_dom::Ref;

use crate::audio::Clip;

pub const RATE: f32 = 48_000.0;

pub type Frame = [f32; 2];

const IN: &[&str] = &["Input"];
const OUT: &[&str] = &["Output"];
const LEFT: &[&str] = &["Left", "BackLeft", "SurroundLeft", "TopLeft", "TopBackLeft"];
const RIGHT: &[&str] = &[
    "Right",
    "BackRight",
    "SurroundRight",
    "TopRight",
    "TopBackRight",
];
const MIXER_IN: &[&str] = &[
    "BackLeft",
    "BackRight",
    "Center",
    "Input",
    "Left",
    "Right",
    "Sub",
    "SurroundLeft",
    "SurroundRight",
    "TopBackLeft",
    "TopBackRight",
    "TopLeft",
    "TopRight",
];
const SPLITTER_OUT: &[&str] = &[
    "BackLeft",
    "BackRight",
    "Center",
    "Left",
    "Output",
    "Right",
    "Sub",
    "SurroundLeft",
    "SurroundRight",
    "TopBackLeft",
    "TopBackRight",
    "TopLeft",
    "TopRight",
];

/// A node class's input and output pins (`GetInputPins`/`GetOutputPins`),
/// `None` for a class that isn't one.
pub fn pins(class: &str) -> Option<(&'static [&'static str], &'static [&'static str])> {
    Some(match class {
        "AudioPlayer" | "AudioDeviceInput" | "AudioListener" => (&[], OUT),
        "AudioEmitter" | "AudioDeviceOutput" | "AudioAnalyzer" | "AudioStreamReader" => (IN, &[]),
        "AudioStreamWriter" => (&[], OUT),
        "AudioCompressor" => (&["Input", "Sidechain"], OUT),
        "AudioChannelMixer" => (MIXER_IN, OUT),
        "AudioChannelSplitter" => (IN, SPLITTER_OUT),
        "AudioEqualizer" | "AudioReverb" | "AudioEcho" | "AudioDistortion" | "AudioChorus"
        | "AudioFader" | "AudioPitchShifter" | "AudioFlanger" | "AudioTremolo" | "AudioFilter"
        | "AudioLimiter" => (IN, OUT),
        _ => return None,
    })
}

/// One biquad section, normalised so `a0` is 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
}

impl Biquad {
    fn new(b: [f32; 3], a: [f32; 3]) -> Biquad {
        Biquad {
            b: b.map(|x| x / a[0]),
            a: [a[1] / a[0], a[2] / a[0]],
        }
    }

    /// Its gain at `freq` Hz, as a ratio.
    fn magnitude(&self, freq: f32) -> f32 {
        let w = 2.0 * PI * freq / RATE;
        let (c1, s1, c2, s2) = (w.cos(), w.sin(), (2.0 * w).cos(), (2.0 * w).sin());
        let ([b0, b1, b2], [a1, a2]) = (self.b, self.a);
        let (nr, ni) = (b0 + b1 * c1 + b2 * c2, -(b1 * s1 + b2 * s2));
        let (dr, di) = (1.0 + a1 * c1 + a2 * c2, -(a1 * s1 + a2 * s2));
        ((nr * nr + ni * ni) / (dr * dr + di * di)).sqrt()
    }
}

/// `Enum.AudioFilterType` (by value) as the sections it cascades: the
/// cookbook biquads at 48 kHz in `f32`, the 24 and 48 dB slopes two and
/// four 12 dB sections each with the n-th root of `q` (so the cascade
/// still has `q`'s gain at `freq`), the 6 dB lowpass a one-pole. Fitted to
/// the AudioApi golden's `gain_*` (within 0.002 dB, the notch's -122.57 dB
/// floor included).
pub fn filter(kind: u32, freq: f32, q: f32, gain: f32) -> Vec<Biquad> {
    let w0 = 2.0 * PI * freq.clamp(1.0, RATE * 0.49) / RATE;
    let q = q.max(0.01);
    let a = 10f32.powf(gain / 40.0);
    let section = |kind: u32, q: f32| {
        let (c, s) = (w0.cos(), w0.sin());
        let alpha = s / (2.0 * q);
        let shelf = 2.0 * a.sqrt() * alpha;
        match kind {
            1 => Biquad::new(
                [
                    a * ((a + 1.0) - (a - 1.0) * c + shelf),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                    a * ((a + 1.0) - (a - 1.0) * c - shelf),
                ],
                [
                    (a + 1.0) + (a - 1.0) * c + shelf,
                    -2.0 * ((a - 1.0) + (a + 1.0) * c),
                    (a + 1.0) + (a - 1.0) * c - shelf,
                ],
            ),
            2 => Biquad::new(
                [
                    a * ((a + 1.0) + (a - 1.0) * c + shelf),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                    a * ((a + 1.0) + (a - 1.0) * c - shelf),
                ],
                [
                    (a + 1.0) - (a - 1.0) * c + shelf,
                    2.0 * ((a - 1.0) - (a + 1.0) * c),
                    (a + 1.0) - (a - 1.0) * c - shelf,
                ],
            ),
            3 => Biquad::new(
                [(1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0],
                [1.0 + alpha, -2.0 * c, 1.0 - alpha],
            ),
            6 => Biquad::new(
                [(1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0],
                [1.0 + alpha, -2.0 * c, 1.0 - alpha],
            ),
            9 => Biquad::new([alpha, 0.0, -alpha], [1.0 + alpha, -2.0 * c, 1.0 - alpha]),
            10 => Biquad::new([1.0, -2.0 * c, 1.0], [1.0 + alpha, -2.0 * c, 1.0 - alpha]),
            _ => Biquad::new(
                [1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a],
                [1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a],
            ),
        }
    };
    let cascade = |kind: u32, n: u8| vec![section(kind, q.powf(1.0 / f32::from(n))); n.into()];
    match kind {
        3 | 6 => cascade(kind, 1),
        4 => cascade(3, 2),
        5 => cascade(3, 4),
        7 => cascade(6, 2),
        8 => cascade(6, 4),
        11 => {
            let p = (-w0).exp();
            vec![Biquad::new([1.0 - p, 0.0, 0.0], [1.0, -p, 0.0])]
        }
        _ => vec![section(kind, q)],
    }
}

/// `AudioFilter:GetGainAt`: the cascade's gain at `freq`, in dB.
pub fn gain_at(sections: &[Biquad], freq: f32) -> f64 {
    sections
        .iter()
        .map(|s| 20.0 * f64::from(s.magnitude(freq)).log10())
        .sum()
}

/// An `AudioPlayer` as it sounds this block.
#[derive(Clone, Default)]
pub struct Playback {
    pub clip: Option<Arc<Clip>>,
    /// Seconds into the clip at the block's start.
    pub position: f64,
    pub speed: f64,
    pub volume: f32,
    /// Where it stops (or, looping, where it jumps back to `loop_start`).
    pub end: f64,
    pub loop_start: f64,
    pub looping: bool,
}

/// What a node does to its input, with the properties it reads (levels
/// as ratios, times in seconds).
#[derive(Clone)]
pub enum Kind {
    Player(Playback),
    /// `AudioDeviceInput`: no microphone here.
    Silence,
    Fader(f32),
    /// `AudioFilter` and `AudioEqualizer`.
    Filter(Vec<Biquad>),
    Echo {
        delay: f32,
        feedback: f32,
        dry: f32,
        wet: f32,
    },
    Reverb {
        decay: f32,
        pre_delay: f32,
        high_cut: f32,
        dry: f32,
        wet: f32,
    },
    Distortion(f32),
    /// `AudioChorus` and `AudioFlanger`: a delay swept by a sine.
    Modulation {
        center: f32,
        swing: f32,
        rate: f32,
        mix: f32,
        feedback: f32,
    },
    Tremolo {
        depth: f32,
        duty: f32,
        frequency: f32,
        square: f32,
    },
    Compressor {
        threshold: f32,
        ratio: f32,
        attack: f32,
        release: f32,
        makeup: f32,
    },
    Limiter {
        max: f32,
        release: f32,
    },
    PitchShifter {
        pitch: f32,
        window: f32,
    },
    Mixer,
    Splitter,
    Emitter,
    Listener,
    Output,
    Analyzer {
        window: usize,
        spectrum: bool,
    },
}

#[derive(Clone)]
pub struct Node {
    pub kind: Kind,
    pub bypass: bool,
}

/// A connected `Wire`.
#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    pub wire: Ref,
    pub source: Ref,
    pub source_pin: String,
    pub target: Ref,
    pub target_pin: String,
}

/// An emitter as a listener hears it.
#[derive(Clone, Debug)]
pub struct Hearing {
    pub emitter: Ref,
    pub listener: Ref,
    pub gain: f32,
    /// -1 (left) to 1 (right).
    pub pan: f32,
}

#[derive(Clone, Default)]
pub struct Graph {
    pub nodes: HashMap<Ref, Node>,
    pub wires: Vec<Wire>,
    pub hearing: Vec<Hearing>,
}

/// An analyzer's reading of its last block.
#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub rms: f32,
    pub peak: f32,
    pub spectrum: Vec<f32>,
}

/// A stereo delay line.
#[derive(Default)]
struct Line {
    buf: Vec<Frame>,
    at: usize,
}

impl Line {
    fn fit(&mut self, len: usize) {
        if self.buf.len() < len + 2 {
            self.buf.resize(len + 2, [0.0; 2]);
        }
    }

    fn push(&mut self, x: Frame) {
        self.at = (self.at + 1) % self.buf.len();
        self.buf[self.at] = x;
    }

    /// What was pushed `delay` pushes ago (1 is the newest), between
    /// samples for a fractional delay.
    fn tap(&self, delay: f32) -> Frame {
        let len = self.buf.len();
        let delay = delay.clamp(1.0, (len - 2) as f32) - 1.0;
        let whole = delay as usize;
        let t = delay - whole as f32;
        let a = self.buf[(self.at + len - whole) % len];
        let b = self.buf[(self.at + 2 * len - whole - 1) % len];
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
    }
}

/// What a node keeps between blocks.
#[derive(Default)]
struct State {
    /// Each biquad section's two memories, per channel.
    sections: Vec<[[f32; 2]; 2]>,
    lines: Vec<Line>,
    phase: f32,
    level: f32,
    smooth: Frame,
    history: Vec<f32>,
}

/// One block's evaluation: outputs already rendered, nodes on the stack.
struct Pass<'a> {
    graph: &'a Graph,
    frames: usize,
    done: HashMap<(Ref, String), Vec<Frame>>,
    busy: HashSet<Ref>,
}

#[derive(Default)]
pub struct Renderer {
    states: HashMap<Ref, State>,
    pub analysis: HashMap<Ref, Analysis>,
}

impl Renderer {
    /// The next `frames` frames of what the device outputs play, every
    /// analyzer reading its input on the way.
    pub fn render(&mut self, graph: &Graph, frames: usize) -> Vec<Frame> {
        self.states.retain(|r, _| graph.nodes.contains_key(r));
        self.analysis.retain(|r, _| graph.nodes.contains_key(r));
        let mut pass = Pass {
            graph,
            frames,
            done: HashMap::new(),
            busy: HashSet::new(),
        };
        let mut sinks: Vec<_> = graph
            .nodes
            .iter()
            .map(|(r, n)| (*r, n.kind.clone()))
            .collect();
        sinks.sort_by_key(|(r, _)| *r);
        let mut out = vec![[0.0; 2]; frames];
        for (node, kind) in sinks {
            match kind {
                Kind::Output => mix(&mut out, &self.input(&mut pass, node, "Input")),
                Kind::Analyzer { window, spectrum } => {
                    let input = self.input(&mut pass, node, "Input");
                    let state = self.states.entry(node).or_default();
                    self.analysis
                        .insert(node, analyze(state, &input, spectrum.then_some(window)));
                }
                _ => {}
            }
        }
        out
    }

    /// The sum of what the wires into `node`'s `pin` carry.
    fn input(&mut self, pass: &mut Pass, node: Ref, pin: &str) -> Vec<Frame> {
        let mut sum = vec![[0.0; 2]; pass.frames];
        let sources: Vec<_> = pass
            .graph
            .wires
            .iter()
            .filter(|w| w.target == node && w.target_pin == pin)
            .map(|w| (w.source, w.source_pin.clone()))
            .collect();
        for (source, source_pin) in sources {
            let block = self.output(pass, source, &source_pin);
            mix(&mut sum, &block);
        }
        sum
    }

    fn output(&mut self, pass: &mut Pass, node: Ref, pin: &str) -> Vec<Frame> {
        let key = (node, pin.to_owned());
        if let Some(block) = pass.done.get(&key) {
            return block.clone();
        }
        let silence = vec![[0.0; 2]; pass.frames];
        let Some(n) = pass.graph.nodes.get(&node).cloned() else {
            return silence;
        };
        if !pass.busy.insert(node) {
            return silence;
        }
        let block = match &n.kind {
            Kind::Splitter => split(self.input(pass, node, "Input"), pin),
            Kind::Listener => {
                let heard: Vec<_> = pass
                    .graph
                    .hearing
                    .iter()
                    .filter(|h| h.listener == node)
                    .cloned()
                    .collect();
                let mut sum = silence;
                for h in heard {
                    let input = self.input(pass, h.emitter, "Input");
                    let angle = (h.pan.clamp(-1.0, 1.0) + 1.0) * PI / 4.0;
                    let (l, r) = (angle.cos() * 2f32.sqrt(), angle.sin() * 2f32.sqrt());
                    for (s, x) in sum.iter_mut().zip(&input) {
                        s[0] += x[0] * h.gain * l;
                        s[1] += x[1] * h.gain * r;
                    }
                }
                sum
            }
            Kind::Player(p) => play(p, pass.frames),
            Kind::Mixer => {
                let mut sum = self.input(pass, node, "Input");
                for channel in MIXER_IN.iter().filter(|c| **c != "Input") {
                    let x = self.input(pass, node, channel);
                    let (l, r) = if LEFT.contains(channel) {
                        (1.0, 0.0)
                    } else if RIGHT.contains(channel) {
                        (0.0, 1.0)
                    } else {
                        (0.5, 0.5)
                    };
                    for (s, x) in sum.iter_mut().zip(&x) {
                        let m = (x[0] + x[1]) * 0.5;
                        s[0] += m * l;
                        s[1] += m * r;
                    }
                }
                sum
            }
            Kind::Compressor { .. } => {
                let input = self.input(pass, node, "Input");
                let wired = pass
                    .graph
                    .wires
                    .iter()
                    .any(|w| w.target == node && w.target_pin == "Sidechain");
                let side = wired.then(|| self.input(pass, node, "Sidechain"));
                let state = self.states.entry(node).or_default();
                if n.bypass {
                    input
                } else {
                    effect(state, &n.kind, input, side.as_deref())
                }
            }
            kind => {
                let input = self.input(pass, node, "Input");
                if n.bypass {
                    input
                } else {
                    effect(self.states.entry(node).or_default(), kind, input, None)
                }
            }
        };
        pass.busy.remove(&node);
        pass.done.insert(key, block.clone());
        block
    }
}

fn mix(into: &mut [Frame], block: &[Frame]) {
    for (s, x) in into.iter_mut().zip(block) {
        s[0] += x[0];
        s[1] += x[1];
    }
}

/// One of a splitter's pins: the whole signal, or one side of it.
fn split(input: Vec<Frame>, pin: &str) -> Vec<Frame> {
    let pick = |x: &Frame| -> f32 {
        if LEFT.contains(&pin) {
            x[0]
        } else if RIGHT.contains(&pin) {
            x[1]
        } else if pin == "Center" {
            (x[0] + x[1]) * 0.5
        } else {
            0.0
        }
    };
    if pin == "Output" {
        return input;
    }
    input.iter().map(|x| [pick(x); 2]).collect()
}

/// A player's clip from its position on, resampled by its speed.
fn play(p: &Playback, frames: usize) -> Vec<Frame> {
    let mut out = vec![[0.0; 2]; frames];
    let Some(clip) = &p.clip else {
        return out;
    };
    let mut at = p.position;
    let step = p.speed / f64::from(RATE);
    for frame in &mut out {
        if at >= p.end {
            if !p.looping || p.end <= p.loop_start {
                break;
            }
            at = p.loop_start + (at - p.end);
        }
        let [l, r] = clip.frame_at(at);
        *frame = [l * p.volume, r * p.volume];
        at += step;
    }
    out
}

fn effect(state: &mut State, kind: &Kind, mut x: Vec<Frame>, side: Option<&[Frame]>) -> Vec<Frame> {
    let seconds = |s: f32| s * RATE;
    match kind {
        &Kind::Fader(volume) => {
            for f in &mut x {
                *f = f.map(|s| s * volume);
            }
        }
        Kind::Filter(sections) => {
            state.sections.resize(sections.len(), [[0.0; 2]; 2]);
            for (s, memory) in sections.iter().zip(&mut state.sections) {
                for f in &mut x {
                    for (c, sample) in f.iter_mut().enumerate() {
                        let [z1, z2] = &mut memory[c];
                        let y = s.b[0] * *sample + *z1;
                        *z1 = s.b[1] * *sample - s.a[0] * y + *z2;
                        *z2 = s.b[2] * *sample - s.a[1] * y;
                        *sample = y;
                    }
                }
            }
        }
        &Kind::Echo {
            delay,
            feedback,
            dry,
            wet,
        } => {
            let d = seconds(delay).max(1.0);
            let line = line(&mut state.lines, 0, d as usize);
            for f in &mut x {
                let echo = line.tap(d);
                line.push([f[0] + feedback * echo[0], f[1] + feedback * echo[1]]);
                *f = [dry * f[0] + wet * echo[0], dry * f[1] + wet * echo[1]];
            }
        }
        &Kind::Reverb {
            decay,
            pre_delay,
            high_cut,
            dry,
            wet,
        } => {
            const COMBS: [f32; 4] = [0.0297, 0.0371, 0.0411, 0.0437];
            const ALLPASSES: [f32; 2] = [0.005, 0.0017];
            let pre = seconds(pre_delay).max(1.0);
            line(&mut state.lines, 0, pre as usize);
            for (i, d) in COMBS.iter().chain(&ALLPASSES).enumerate() {
                line(&mut state.lines, i + 1, seconds(*d) as usize);
            }
            let k = 1.0 - (-2.0 * PI * high_cut.min(RATE * 0.49) / RATE).exp();
            for f in &mut x {
                let early = state.lines[0].tap(pre);
                state.lines[0].push(*f);
                let mut s = [0.0; 2];
                for (i, d) in COMBS.iter().enumerate() {
                    let line = &mut state.lines[i + 1];
                    let g = 10f32.powf(-3.0 * d / decay.max(0.01));
                    let y = line.tap(seconds(*d));
                    line.push([early[0] + g * y[0], early[1] + g * y[1]]);
                    s = [s[0] + y[0] * 0.25, s[1] + y[1] * 0.25];
                }
                for (i, d) in ALLPASSES.iter().enumerate() {
                    let line = &mut state.lines[i + 1 + COMBS.len()];
                    let y = line.tap(seconds(*d));
                    line.push([s[0] + 0.7 * y[0], s[1] + 0.7 * y[1]]);
                    s = [y[0] - 0.7 * s[0], y[1] - 0.7 * s[1]];
                }
                for (smooth, s) in state.smooth.iter_mut().zip(s) {
                    *smooth += k * (s - *smooth);
                }
                *f = [
                    dry * f[0] + wet * state.smooth[0],
                    dry * f[1] + wet * state.smooth[1],
                ];
            }
        }
        &Kind::Distortion(level) => {
            let drive = 1.0 + 20.0 * level.clamp(0.0, 1.0);
            for f in &mut x {
                *f = f.map(|s| (drive * s).tanh() / drive.tanh());
            }
        }
        &Kind::Modulation {
            center,
            swing,
            rate,
            mix,
            feedback,
        } => {
            let line = line(&mut state.lines, 0, seconds(center + swing) as usize + 1);
            for f in &mut x {
                state.phase = (state.phase + rate / RATE).fract();
                let lfo = (2.0 * PI * state.phase).sin();
                let delayed = line.tap(seconds(center + swing * lfo));
                line.push([f[0] + feedback * delayed[0], f[1] + feedback * delayed[1]]);
                *f = [
                    (1.0 - mix) * f[0] + mix * delayed[0],
                    (1.0 - mix) * f[1] + mix * delayed[1],
                ];
            }
        }
        &Kind::Tremolo {
            depth,
            duty,
            frequency,
            square,
        } => {
            for f in &mut x {
                state.phase = (state.phase + frequency / RATE).fract();
                let sine = 0.5 + 0.5 * (2.0 * PI * state.phase).cos();
                let pulse = if state.phase < duty { 1.0 } else { 0.0 };
                let lfo = sine + (pulse - sine) * square.clamp(0.0, 1.0);
                let gain = 1.0 - depth.clamp(0.0, 1.0) * (1.0 - lfo);
                *f = f.map(|s| s * gain);
            }
        }
        &Kind::Compressor {
            threshold,
            ratio,
            attack,
            release,
            makeup,
        } => {
            let (att, rel) = (
                (-1.0 / seconds(attack).max(1.0)).exp(),
                (-1.0 / seconds(release).max(1.0)).exp(),
            );
            for (i, f) in x.iter_mut().enumerate() {
                let key = side.map_or(*f, |s| s[i]);
                let level = key[0].abs().max(key[1].abs());
                let k = if level > state.level { att } else { rel };
                state.level = k * state.level + (1.0 - k) * level;
                let over = 20.0 * state.level.max(1e-9).log10() - threshold;
                let db = makeup - over.max(0.0) * (1.0 - 1.0 / ratio.max(1.0));
                let gain = 10f32.powf(db / 20.0);
                *f = f.map(|s| s * gain);
            }
        }
        &Kind::Limiter { max, release } => {
            let rel = (-1.0 / seconds(release).max(1.0)).exp();
            for f in &mut x {
                state.level = f[0].abs().max(f[1].abs()).max(state.level * rel);
                if state.level > max {
                    let gain = max / state.level;
                    *f = f.map(|s| s * gain);
                }
            }
        }
        &Kind::PitchShifter { pitch, window } => {
            let w = seconds(window).max(2.0);
            let line = line(&mut state.lines, 0, w as usize + 1);
            for f in &mut x {
                line.push(*f);
                state.phase = (state.phase + (1.0 - pitch) / w).rem_euclid(1.0);
                let mut y = [0.0; 2];
                for offset in [0.0, 0.5] {
                    let p = (state.phase + offset).fract();
                    let tap = line.tap(1.0 + p * w);
                    let gain = 1.0 - (2.0 * p - 1.0).abs();
                    y = [y[0] + tap[0] * gain, y[1] + tap[1] * gain];
                }
                *f = y;
            }
        }
        _ => {}
    }
    x
}

/// The `i`-th delay line, at least `len` long.
fn line(lines: &mut Vec<Line>, i: usize, len: usize) -> &mut Line {
    if lines.len() <= i {
        lines.resize_with(i + 1, Line::default);
    }
    let line = &mut lines[i];
    line.fit(len);
    line
}

/// RMS and peak of `input`, and with `window` the magnitudes of the
/// last `window` samples' spectrum (a Hann-windowed FFT, `window / 2`
/// bins).
fn analyze(state: &mut State, input: &[Frame], window: Option<usize>) -> Analysis {
    let n = input.len().max(1) as f32;
    let rms = (input
        .iter()
        .map(|f| (f[0] * f[0] + f[1] * f[1]) * 0.5)
        .sum::<f32>()
        / n)
        .sqrt();
    let peak = input
        .iter()
        .fold(0f32, |m, f| m.max(f[0].abs()).max(f[1].abs()));
    let spectrum = window.map_or_else(Vec::new, |window| {
        state
            .history
            .extend(input.iter().map(|f| (f[0] + f[1]) * 0.5));
        let excess = state.history.len().saturating_sub(window);
        state.history.drain(..excess);
        if state.history.len() < window {
            return vec![0.0; window / 2];
        }
        fft_magnitudes(&state.history)
    });
    Analysis {
        rms,
        peak,
        spectrum,
    }
}

fn fft_magnitudes(samples: &[f32]) -> Vec<f32> {
    let n = samples.len();
    let mut re: Vec<f32> = samples
        .iter()
        .enumerate()
        .map(|(i, s)| s * (0.5 - 0.5 * (2.0 * PI * i as f32 / n as f32).cos()))
        .collect();
    let mut im = vec![0.0; n];
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let half = len / 2;
        for start in (0..n).step_by(len) {
            for k in 0..half {
                let (s, c) = (-2.0 * PI * k as f32 / len as f32).sin_cos();
                let (a, b) = (start + k, start + k + half);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                (re[b], im[b]) = (re[a] - tr, im[a] - ti);
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
    (0..n / 2)
        .map(|k| re[k].hypot(im[k]) * 4.0 / n as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: Ref = Ref::new(1);
    const NODE: Ref = Ref::new(2);
    const SINK: Ref = Ref::new(3);
    const ANALYZER: Ref = Ref::new(4);
    const AMPLITUDE: f32 = 0.5;

    fn sine(hz: f32) -> Arc<Clip> {
        Arc::new(Clip {
            samples: (0..RATE as usize)
                .map(|i| (2.0 * PI * hz * i as f32 / RATE).sin() * AMPLITUDE)
                .collect(),
            channels: 1,
            rate: RATE as u32,
        })
    }

    fn impulse() -> Arc<Clip> {
        let mut samples = vec![0.0; RATE as usize * 2];
        samples[0] = 1.0;
        Arc::new(Clip {
            samples,
            channels: 1,
            rate: RATE as u32,
        })
    }

    fn node(kind: Kind) -> Node {
        Node {
            kind,
            bypass: false,
        }
    }

    fn wire(source: Ref, target: Ref) -> Wire {
        Wire {
            wire: Ref::new(100 + source.value()),
            source,
            source_pin: "Output".into(),
            target,
            target_pin: "Input".into(),
        }
    }

    /// `clip` through `effect` into a device output and an analyzer.
    fn chain(clip: Arc<Clip>, effect: Kind) -> Graph {
        let playback = Playback {
            end: clip.seconds(),
            clip: Some(clip),
            speed: 1.0,
            volume: 1.0,
            ..Playback::default()
        };
        let analyzer = Kind::Analyzer {
            window: 1024,
            spectrum: true,
        };
        Graph {
            nodes: HashMap::from([
                (SOURCE, node(Kind::Player(playback))),
                (NODE, node(effect)),
                (SINK, node(Kind::Output)),
                (ANALYZER, node(analyzer)),
            ]),
            wires: vec![wire(SOURCE, NODE), wire(NODE, SINK), wire(NODE, ANALYZER)],
            hearing: Vec::new(),
        }
    }

    fn rms(block: &[Frame], channel: usize) -> f32 {
        (block.iter().map(|f| f[channel] * f[channel]).sum::<f32>() / block.len() as f32).sqrt()
    }

    fn sine_rms() -> f32 {
        AMPLITUDE / 2f32.sqrt()
    }

    /// The level of `hz` through `effect` once it settles, relative to
    /// the sine's own.
    fn level_through(effect: Kind, hz: f32) -> f32 {
        let block = Renderer::default().render(&chain(sine(hz), effect), RATE as usize / 2);
        rms(&block[RATE as usize / 4..], 0) / sine_rms()
    }

    fn loudest_bin(renderer: &Renderer) -> Option<usize> {
        let spectrum = &renderer.analysis[&ANALYZER].spectrum;
        (0..spectrum.len()).max_by(|a, b| spectrum[*a].total_cmp(&spectrum[*b]))
    }

    #[test]
    fn the_filter_has_the_golden_response() {
        // AudioApi golden rows: lowpass, highpass cascade, the notch's
        // floor, the one-pole, a 6 dB peak an octave off, the bandpass.
        for (kind, at, q, gain, db) in [
            (3, 8000.0, 0.707, 0.0, -25.6923),
            (8, 1000.0, 0.707, 0.0, -45.854),
            (10, 2000.0, 0.707, 0.0, -122.5659),
            (11, 2000.0, 0.707, 0.0, -2.9856),
            (0, 4000.0, 2.0, 6.0, 0.6027),
            (9, 1000.0, 0.707, 0.0, -3.3059),
        ] {
            let got = gain_at(&filter(kind, 2000.0, q, gain), at);
            assert!(
                (got - db).abs() < 0.01,
                "type {kind} at {at}: {got} vs {db}"
            );
        }
    }

    #[test]
    fn a_rendered_filter_has_the_response_it_reports() {
        for (kind, hz) in [(3, 8000.0), (6, 1000.0), (0, 2000.0), (2, 8000.0)] {
            let sections = filter(kind, 2000.0, 0.707, 6.0);
            let want = 10f32.powf(gain_at(&sections, hz) as f32 / 20.0);
            let got = level_through(Kind::Filter(sections), hz);
            assert!(
                (got - want).abs() < 0.02 * want.max(0.05),
                "type {kind}: {got} vs {want}"
            );
        }
    }

    #[test]
    fn a_fader_scales_and_bypass_passes_through() {
        assert!((level_through(Kind::Fader(0.25), 440.0) - 0.25).abs() < 0.01);
        let mut graph = chain(sine(440.0), Kind::Fader(0.0));
        graph.nodes.get_mut(&NODE).unwrap().bypass = true;
        let block = Renderer::default().render(&graph, 4800);
        assert!((rms(&block, 0) - sine_rms()).abs() < 0.01);
    }

    #[test]
    fn an_echo_repeats_after_its_delay_at_its_levels() {
        let echo = Kind::Echo {
            delay: 0.25,
            feedback: 0.5,
            dry: 1.0,
            wet: 0.5,
        };
        let block = Renderer::default().render(&chain(impulse(), echo), RATE as usize);
        let d = (0.25 * RATE) as usize;
        assert!((block[0][0] - 1.0).abs() < 1e-6, "the dry impulse");
        assert!((block[d][0] - 0.5).abs() < 1e-6, "the first echo at {d}");
        assert!((block[2 * d][0] - 0.25).abs() < 1e-6, "the feedback's");
        let elsewhere = block.iter().enumerate().filter(|(i, _)| i % d != 0);
        assert!(elsewhere.map(|(_, f)| f[0].abs()).fold(0.0, f32::max) < 1e-6);
    }

    #[test]
    fn effect_state_carries_into_the_next_block() {
        let echo = Kind::Echo {
            delay: 0.25,
            feedback: 0.0,
            dry: 0.0,
            wet: 1.0,
        };
        let mut graph = chain(impulse(), echo);
        let mut renderer = Renderer::default();
        let first = renderer.render(&graph, 6000);
        if let Kind::Player(p) = &mut graph.nodes.get_mut(&SOURCE).unwrap().kind {
            p.position = 6000.0 / f64::from(RATE);
        }
        let second = renderer.render(&graph, 12000);
        assert!(first.iter().all(|f| f[0] == 0.0));
        assert!(
            (second[6000][0] - 1.0).abs() < 1e-6,
            "the echo lands in the next block"
        );
    }

    #[test]
    fn the_analyzer_reads_a_sine() {
        let mut renderer = Renderer::default();
        renderer.render(&chain(sine(1500.0), Kind::Fader(1.0)), 2048);
        let reading = &renderer.analysis[&ANALYZER];
        assert!((reading.rms - sine_rms()).abs() < 0.005, "{}", reading.rms);
        assert!((reading.peak - AMPLITUDE).abs() < 0.005);
        assert_eq!(reading.spectrum.len(), 512);
        assert_eq!(
            loudest_bin(&renderer),
            Some(32),
            "1500 Hz in 46.875 Hz bins"
        );
    }

    #[test]
    fn the_dynamics_and_modulation_effects_do_what_they_say() {
        let limiter = Kind::Limiter {
            max: 0.25,
            release: 0.01,
        };
        let block = Renderer::default().render(&chain(sine(440.0), limiter), 9600);
        let peak = block[4800..].iter().fold(0f32, |m, f| m.max(f[0].abs()));
        assert!((peak - 0.25).abs() < 0.01, "the limiter holds {peak}");

        // A sine peaking at -6 dB is 15 dB over -21 dB: at 4:1, 11.25 off.
        let compressor = Kind::Compressor {
            threshold: -21.0,
            ratio: 4.0,
            attack: 0.001,
            release: 0.5,
            makeup: 0.0,
        };
        let squeezed = 20.0 * level_through(compressor, 440.0).log10();
        assert!(
            (squeezed + 11.25).abs() < 1.0,
            "compressed by {squeezed} dB"
        );

        let tremolo = Kind::Tremolo {
            depth: 1.0,
            duty: 0.5,
            frequency: 8.0,
            square: 1.0,
        };
        let gated = level_through(tremolo, 440.0);
        assert!(
            (gated - 0.5f32.sqrt()).abs() < 0.05,
            "a half-duty gate: {gated}"
        );

        let mut renderer = Renderer::default();
        let octave = Kind::PitchShifter {
            pitch: 2.0,
            window: 0.05,
        };
        renderer.render(&chain(sine(750.0), octave), 9600);
        assert_eq!(loudest_bin(&renderer), Some(32), "750 Hz an octave up");

        for effect in [
            Kind::Reverb {
                decay: 1.5,
                pre_delay: 0.02,
                high_cut: 20000.0,
                dry: 1.0,
                wet: 0.5,
            },
            Kind::Distortion(0.5),
            Kind::Modulation {
                center: 0.02,
                swing: 0.005,
                rate: 5.0,
                mix: 0.85,
                feedback: 0.0,
            },
        ] {
            let level = level_through(effect, 440.0);
            assert!(level.is_finite() && level > 0.3, "{level}");
        }
    }

    #[test]
    fn a_listener_hears_its_emitters_at_their_gain_and_side() {
        let listener = Ref::new(5);
        let mut graph = chain(sine(440.0), Kind::Emitter);
        graph.nodes.insert(listener, node(Kind::Listener));
        graph.wires = vec![wire(SOURCE, NODE), wire(listener, SINK)];
        graph.hearing.push(Hearing {
            emitter: NODE,
            listener,
            gain: 0.5,
            pan: 1.0,
        });
        let block = Renderer::default().render(&graph, 4800);
        assert!(block.iter().all(|f| f[0].abs() < 1e-6), "hard right");
        let right = rms(&block, 1);
        assert!(
            (right - 0.5 * 2f32.sqrt() * sine_rms()).abs() < 0.01,
            "{right}"
        );
    }

    #[test]
    fn a_splitter_and_a_mixer_route_channels() {
        let mixer = Ref::new(6);
        let mut graph = chain(sine(440.0), Kind::Splitter);
        graph.nodes.insert(mixer, node(Kind::Mixer));
        graph.wires = vec![
            wire(SOURCE, NODE),
            Wire {
                source_pin: "Left".into(),
                target_pin: "Right".into(),
                ..wire(NODE, mixer)
            },
            wire(mixer, SINK),
        ];
        let block = Renderer::default().render(&graph, 4800);
        assert!(block.iter().all(|f| f[0] == 0.0));
        assert!((rms(&block, 1) - sine_rms()).abs() < 0.01);
    }
}
