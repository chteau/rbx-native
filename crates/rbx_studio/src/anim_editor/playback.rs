//! Playing the clip and moving the playhead.

use super::AnimEditor;

impl AnimEditor {
    /// Advances playback by `dt`. A looping clip wraps; any other stops at
    /// its last key. Returns false once playback has stopped.
    pub(crate) fn tick(&mut self, dt: f32) -> bool {
        if !self.playing {
            return false;
        }
        let length = self.clip.length();
        if length <= 0.0 {
            self.playing = false;
            return false;
        }
        self.time += dt;
        if self.time > length {
            if self.clip.looped {
                self.time = self.time.rem_euclid(length);
            } else {
                self.time = length;
                self.playing = false;
            }
        }
        self.playing
    }

    pub(crate) fn toggle_play(&mut self) {
        if !self.playing && self.time >= self.clip.length() {
            self.time = 0.0;
        }
        self.playing = !self.playing && self.clip.length() > 0.0;
    }

    pub(crate) fn step_frames(&mut self, frames: i64) {
        self.playing = false;
        let fps = self.fps.max(1) as f32;
        self.time = ((self.frame() + frames) as f32 / fps).max(0.0);
    }

    /// Jumps to the next (or previous) keyframe time from the playhead.
    pub(crate) fn jump_key(&mut self, forward: bool) {
        self.playing = false;
        let now = self.time;
        let times = self.clip.keyframes.iter().map(|k| k.time);
        let target = if forward {
            times.filter(|t| *t > now + 1e-4).reduce(f32::min)
        } else {
            times.filter(|t| *t < now - 1e-4).reduce(f32::max)
        };
        if let Some(t) = target {
            self.time = t;
        }
    }
}
