//! Keypoints along a timeline: the time↔pixel mapping, frame snapping, ruler
//! ticks and key hit-testing of a horizontal timeline, kept apart from any
//! window so it can be tested on its own. The Animation Editor's dope sheet
//! is built on it; `paint` holds the strokes it and the sequence editor's
//! plot both draw with.

pub(crate) mod paint;

/// How close (in pixels) the pointer must come to a key to grab it.
pub(crate) const GRAB_RADIUS: f32 = 6.0;

/// The narrowest and widest a second may be drawn.
const SCALE_RANGE: (f32, f32) = (12.0, 6000.0);

/// Minimum room between two labelled ticks, so frame numbers never overlap.
const LABEL_GAP: f32 = 56.0;

/// Steps (in frames) a labelled tick may fall on, smallest first.
const STEPS: [u32; 12] = [1, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Timeline {
    /// The time (seconds) at the left edge.
    pub(crate) start: f32,
    /// Pixels per second.
    pub(crate) scale: f32,
    pub(crate) fps: u32,
}

/// One mark on the ruler.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tick {
    pub(crate) x: f32,
    pub(crate) frame: i64,
    /// Labelled ticks are major; the rest are the lighter subdivisions.
    pub(crate) major: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline {
            start: -0.25,
            scale: 160.0,
            fps: 30,
        }
    }
}

impl Timeline {
    pub(crate) fn x_of(&self, time: f32) -> f32 {
        (time - self.start) * self.scale
    }

    pub(crate) fn time_at(&self, x: f32) -> f32 {
        self.start + x / self.scale
    }

    fn fps(&self) -> f32 {
        self.fps.max(1) as f32
    }

    pub(crate) fn time_of_frame(&self, frame: i64) -> f32 {
        frame as f32 / self.fps()
    }

    /// The ruler for a timeline `width` pixels wide: labelled ticks at the
    /// smallest frame step that keeps their labels apart, and up to four
    /// subdivisions between them while those stay a few pixels apart.
    pub(crate) fn ticks(&self, width: f32) -> Vec<Tick> {
        let frame_px = self.scale / self.fps();
        let step = STEPS
            .into_iter()
            .find(|s| *s as f32 * frame_px >= LABEL_GAP)
            .unwrap_or(*STEPS.last().unwrap_or(&1));
        let sub = [5, 2, 1]
            .into_iter()
            .find(|d| step % d == 0 && (step / d) as f32 * frame_px >= 6.0)
            .map_or(step, |d| step / d)
            .max(1);
        let first = (self.time_at(0.0) * self.fps()).floor() as i64;
        let last = (self.time_at(width) * self.fps()).ceil() as i64;
        let sub = i64::from(sub);
        let mut out = Vec::new();
        let mut frame = first.div_euclid(sub) * sub;
        while frame <= last {
            let x = self.x_of(self.time_of_frame(frame));
            if (0.0..=width).contains(&x) {
                out.push(Tick {
                    x,
                    frame,
                    major: frame.rem_euclid(i64::from(step)) == 0,
                });
            }
            frame += sub;
        }
        out
    }

    /// Zooms by `factor` keeping the time under `x` where it is.
    pub(crate) fn zoom_about(&mut self, x: f32, factor: f32) {
        let anchor = self.time_at(x);
        self.scale = (self.scale * factor).clamp(SCALE_RANGE.0, SCALE_RANGE.1);
        self.start = anchor - x / self.scale;
    }

    pub(crate) fn pan(&mut self, dx: f32) {
        self.start -= dx / self.scale;
    }

    /// Shows `0..length` across `width` with a little room either side.
    pub(crate) fn fit(&mut self, length: f32, width: f32) {
        let length = length.max(1.0 / self.fps());
        let margin = 24.0;
        self.scale = ((width - margin * 2.0).max(1.0) / length).clamp(SCALE_RANGE.0, SCALE_RANGE.1);
        self.start = -margin / self.scale;
    }

    /// The key in `times` nearest `x`, if any lies within the grab radius.
    pub(crate) fn hit(&self, times: &[f32], x: f32) -> Option<usize> {
        times
            .iter()
            .enumerate()
            .map(|(i, t)| (i, (self.x_of(*t) - x).abs()))
            .filter(|(_, d)| *d <= GRAB_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }
}

/// `frame` as Blender's dope sheet labels it, or as seconds+frames
/// (`1:15`) the way Roblox's Animation Editor does.
pub(crate) fn label(frame: i64, fps: u32, seconds: bool) -> String {
    if !seconds {
        return frame.to_string();
    }
    let fps = i64::from(fps.max(1));
    let sign = if frame < 0 { "-" } else { "" };
    let frame = frame.abs();
    format!("{sign}{}:{:02}", frame / fps, frame % fps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_time_and_pixels_both_ways() {
        let t = Timeline {
            start: -0.5,
            scale: 100.0,
            fps: 30,
        };
        assert_eq!(t.x_of(0.0), 50.0);
        assert_eq!(t.time_at(150.0), 1.0);
        assert_eq!(t.time_of_frame(15), 0.5);
    }

    #[test]
    fn ticks_keep_labels_apart() {
        let t = Timeline {
            start: 0.0,
            scale: 300.0,
            fps: 60,
        };
        let ticks = t.ticks(600.0);
        let majors: Vec<&Tick> = ticks.iter().filter(|k| k.major).collect();
        assert!(majors.len() >= 2);
        for pair in majors.windows(2) {
            assert!(pair[1].x - pair[0].x >= LABEL_GAP - 1e-3);
        }
        assert!(ticks.len() > majors.len(), "subdivisions between labels");
        assert_eq!(majors[0].frame, 0);
    }

    #[test]
    fn zoom_keeps_the_anchor_and_clamps() {
        let mut t = Timeline::default();
        let before = t.time_at(200.0);
        t.zoom_about(200.0, 2.0);
        assert!((t.time_at(200.0) - before).abs() < 1e-5);
        t.zoom_about(0.0, 1e9);
        assert_eq!(t.scale, SCALE_RANGE.1);
        let left = t.time_at(0.0);
        t.pan(t.scale);
        assert!((t.time_at(0.0) - (left - 1.0)).abs() < 1e-4);
    }

    #[test]
    fn fit_shows_the_whole_clip() {
        let mut t = Timeline::default();
        t.fit(2.0, 448.0);
        assert!(t.x_of(0.0) > 0.0 && t.x_of(2.0) < 448.0);
        assert!((t.x_of(2.0) - 424.0).abs() < 1e-3);
    }

    #[test]
    fn hit_picks_the_nearest_key_within_reach() {
        let t = Timeline {
            start: 0.0,
            scale: 100.0,
            fps: 30,
        };
        assert_eq!(t.hit(&[0.0, 0.5, 0.52], 50.5), Some(1));
        assert_eq!(t.hit(&[0.0, 0.5, 0.52], 51.5), Some(2));
        assert_eq!(t.hit(&[0.0, 0.5], 80.0), None);
    }

    #[test]
    fn labels_in_frames_or_seconds() {
        assert_eq!(label(75, 30, false), "75");
        assert_eq!(label(75, 30, true), "2:15");
        assert_eq!(label(-5, 30, true), "-0:05");
    }
}
