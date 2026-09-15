//! `Automatic` quality: the level chosen from the frame rate it produces.
//!
//! # This manager is an approximation
//!
//! Roblox calls its own the Frame Rate Manager (`RenderSettings.EnableFRM`, on
//! by default): the client moves the internal level between 1 and 21 while the
//! game runs to hold its frame-rate target, which is why a place looks blurrier
//! for a moment after a busy scene loads. No algorithm, no thresholds and no
//! step sizes are published anywhere, so everything below — the window length,
//! the two thresholds, the dwell times, the leap of four — is chosen to behave
//! plausibly, not to reproduce the engine.
//!
//! Time is measured in the frame times fed to [`FrameRateManager::record`]
//! rather than off a clock: a host only records frames it actually drew, so an
//! idle view neither ages the dwell timers nor lets a level drift while nothing
//! is being rendered. It also makes every rule here testable without sleeping.
//! The side effect is that a view running well under budget ages its timers
//! slower than the wall clock (the budget it does not spend is not counted),
//! which slows climbing down rather than falling.

use std::collections::VecDeque;
use std::time::Duration;

use super::QualityLevel;

/// Frames measured before the first decision. The level starts at the top and
/// the warm-up is what estimates the machine from there: 20 frames is a third of
/// a second at 60 Hz, long enough for a driver's first-frame costs (pipeline
/// compilation, texture uploads) to fall out of the average.
const WARMUP_FRAMES: u32 = 20;
/// Frames ignored after a level change. Applying one rebinds and, across the
/// multisampling boundary, recompiles pipelines on the same thread that draws, so
/// the frames straight after a step are the step's own cost and say nothing about
/// the new level.
const SETTLE_FRAMES: u32 = 5;
/// How much history the average is taken over. Short enough to react to walking
/// into a dense part of a map, long enough that one stalled frame is not a verdict.
const WINDOW: Duration = Duration::from_secs(1);
/// Over its own budget: the frame rate is already below target, so the level has
/// to come down.
const OVER_BUDGET: f32 = 1.0;
/// The headroom a climb needs. A view at 70 % of budget has a level's worth of
/// cost in hand; one at 90 % would step up only to step straight back down.
const UNDER_BUDGET: f32 = 0.7;
/// Far enough under budget that one level at a time would waste a minute of
/// climbing on a machine that is nowhere near its limit.
const LEAP_BUDGET: f32 = 0.4;
const LEAP_LEVELS: i16 = 4;
/// Falling is deliberately faster than climbing: a level too high is frames the
/// user is already losing, a level too low is detail they may not notice.
const DOWN_AFTER: Duration = Duration::from_millis(500);
const UP_AFTER: Duration = Duration::from_secs(3);
/// Hysteresis: after a step down, no step up for this long, whatever the frames
/// say. Without it a machine sitting exactly on the boundary would trade a level
/// back and forth every few seconds, and a quality level that flickers reads as
/// a bug rather than as a compromise.
const HOLD_AFTER_DOWN: Duration = Duration::from_secs(5);
/// Same clamps as the frame budget elsewhere: keeps the arithmetic clear of zero
/// and of infinity when a caller passes a rate a display never had.
const TARGET_HZ: (f32, f32) = (1.0, 1000.0);

/// Picks a quality level from the frame times it is fed, one step at a time.
///
/// One per rendering loop: the host records every frame it draws and applies
/// [`FrameRateManager::changed`] to its renderer. It never decides while the
/// view is idle, because an idle view records nothing.
pub struct FrameRateManager {
    /// What one frame is allowed to cost to hold the target.
    budget: Duration,
    level: u8,
    /// The frame times the average is taken over, oldest first.
    window: VecDeque<Duration>,
    windowed: Duration,
    /// Frames left to ignore before the next decision: the warm-up, then the
    /// settling after every step.
    skip: u32,
    /// How long the average has stayed over budget, and under the climbing
    /// threshold. Only one of the two is ever non-zero.
    over: Duration,
    under: Duration,
    /// What is left of the post-step-down hysteresis.
    hold: Duration,
    /// A level not yet handed to [`FrameRateManager::changed`].
    pending: Option<u8>,
}

impl FrameRateManager {
    pub fn new(target_hz: f32) -> Self {
        let hz = if target_hz.is_finite() {
            target_hz.clamp(TARGET_HZ.0, TARGET_HZ.1)
        } else {
            TARGET_HZ.1
        };

        FrameRateManager {
            budget: Duration::from_secs_f32(1.0 / hz),
            // The top level is the honest probe: the first frames have to be
            // drawn at some level, and starting low would mean a machine that
            // can manage everything spends its first seconds looking worse than
            // it has to.
            level: QualityLevel::MAX,
            window: VecDeque::new(),
            windowed: Duration::ZERO,
            skip: WARMUP_FRAMES,
            over: Duration::ZERO,
            under: Duration::ZERO,
            hold: Duration::ZERO,
            pending: None,
        }
    }

    /// One rendered frame, as long as it took the host to produce — everything
    /// the level can change, and nothing the host spent waiting for a clock.
    pub fn record(&mut self, frame: Duration) {
        self.push(frame);
        self.hold = self.hold.saturating_sub(frame);
        if self.skip > 0 {
            self.skip -= 1;
            return;
        }

        let average = self.average();
        if average > self.budget.mul_f32(OVER_BUDGET) {
            self.over += frame;
            self.under = Duration::ZERO;
        } else if average <= self.budget.mul_f32(UNDER_BUDGET) {
            self.under += frame;
            self.over = Duration::ZERO;
        } else {
            self.over = Duration::ZERO;
            self.under = Duration::ZERO;
        }

        if self.over >= DOWN_AFTER {
            self.step(-1);
        } else if self.under >= UP_AFTER && self.hold.is_zero() {
            let leap = if average <= self.budget.mul_f32(LEAP_BUDGET) {
                LEAP_LEVELS
            } else {
                1
            };
            self.step(leap);
        }
    }

    pub fn level(&self) -> u8 {
        self.level
    }

    /// The new level, once, if the last frames moved it. Taken rather than
    /// peeked: the host switches its renderer on the answer, and doing that twice
    /// for one decision pays the switch twice over.
    pub fn changed(&mut self) -> Option<u8> {
        self.pending.take()
    }

    fn push(&mut self, frame: Duration) {
        self.window.push_back(frame);
        self.windowed += frame;
        // One sample is always kept: an average of nothing has no meaning, and a
        // single frame longer than the whole window is exactly the stall the
        // level has to answer.
        while self.windowed > WINDOW && self.window.len() > 1 {
            self.windowed -= self.window.pop_front().unwrap_or_default();
        }
    }

    fn average(&self) -> Duration {
        let frames = u32::try_from(self.window.len()).unwrap_or(u32::MAX);
        self.windowed.checked_div(frames).unwrap_or(Duration::ZERO)
    }

    /// Moves the level by `by`, clamped, and forgets the history: frames drawn
    /// at the level just left describe a renderer that no longer exists.
    fn step(&mut self, by: i16) {
        let next = (i16::from(self.level) + by)
            .clamp(i16::from(QualityLevel::MIN), i16::from(QualityLevel::MAX));
        let next = u8::try_from(next).unwrap_or(QualityLevel::MIN);

        self.window.clear();
        self.windowed = Duration::ZERO;
        self.over = Duration::ZERO;
        self.under = Duration::ZERO;
        self.skip = SETTLE_FRAMES;
        if by < 0 {
            self.hold = HOLD_AFTER_DOWN;
        }

        if next != self.level {
            self.level = next;
            self.pending = Some(next);
        }
    }
}

#[cfg(test)]
#[path = "auto/tests.rs"]
mod tests;
