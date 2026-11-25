//! Touch gestures, our own from the creator docs: the fingers a client's
//! touches put down become `UserInputService`'s `TouchTap`,
//! `TouchTapInWorld`, `TouchLongPress`, `TouchPan`, `TouchDrag`,
//! `TouchSwipe`, `TouchPinch` and `TouchRotate`, and the same gestures (less
//! the processed flag) on the GuiObject the first finger came down on.
//! Positions count from below the top bar, as the touches' own
//! `InputObject`s do. The thresholds are ours (no golden yet): a finger
//! that moves under [`SLOP`] pixels and lifts within [`TAP`] seconds taps,
//! one held still for [`LONG_PRESS`] seconds long-presses, and a pan that
//! ends faster than [`SWIPE_SPEED`] pixels a second swipes.

use std::collections::BTreeMap;

use rbx_dom::Ref;
use rbx_lua::server::Arg;

use crate::input::Phase;

const SLOP: f32 = 10.0;
const TAP: f64 = 0.4;
const LONG_PRESS: f64 = 0.5;
const SWIPE_SPEED: f32 = 600.0;

struct Finger {
    start: [f32; 2],
    at: [f32; 2],
}

/// Where a two-finger gesture started, and where it was last.
struct Pair {
    distance: f32,
    angle: f32,
    began: bool,
    scale: f32,
    rotation: f32,
}

/// One client's fingers and the gesture they make.
#[derive(Default)]
pub(crate) struct Touches {
    fingers: BTreeMap<u32, Finger>,
    started: f64,
    /// The GuiObject the first finger came down on, and whether the GUI
    /// took the touch.
    target: Option<Ref>,
    processed: bool,
    most: usize,
    long: Option<bool>,
    pan: bool,
    origin: [f32; 2],
    last: ([f32; 2], f64),
    velocity: [f32; 2],
    pair: Option<Pair>,
}

/// A signal to fire: on `UserInputService` when `owner` is `None`, else on
/// that GuiObject.
pub(crate) type Gesture = (Option<Ref>, &'static str, Vec<Arg>);

fn state(item: &str) -> Arg {
    Arg::Enum {
        enum_name: "UserInputState".into(),
        item: item.into(),
    }
}

fn v2(p: [f32; 2]) -> Arg {
    Arg::Vector2(p)
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn length(v: [f32; 2]) -> f32 {
    v[0].hypot(v[1])
}

/// Up, down, left or right: where `v` mostly points (screen y grows down).
fn direction(v: [f32; 2]) -> Arg {
    let item = if v[0].abs() >= v[1].abs() {
        if v[0] >= 0.0 {
            "Right"
        } else {
            "Left"
        }
    } else if v[1] >= 0.0 {
        "Down"
    } else {
        "Up"
    };
    Arg::Enum {
        enum_name: "SwipeDirection".into(),
        item: item.into(),
    }
}

impl Touches {
    fn positions(&self) -> Arg {
        Arg::List(self.fingers.values().map(|f| v2(f.at)).collect())
    }

    fn centroid(&self) -> [f32; 2] {
        let n = self.fingers.len().max(1) as f32;
        let sum = self
            .fingers
            .values()
            .fold([0.0, 0.0], |s, f| [s[0] + f.at[0], s[1] + f.at[1]]);
        [sum[0] / n, sum[1] / n]
    }

    /// The first two fingers' spread and angle (degrees).
    fn spread(&self) -> Option<(f32, f32)> {
        let mut it = self.fingers.values();
        let (a, b) = (it.next()?, it.next()?);
        let d = sub(b.at, a.at);
        Some((length(d), d[1].atan2(d[0]).to_degrees()))
    }

    /// Fires on `UserInputService` (with the processed flag) and on the
    /// target GuiObject.
    fn both(&self, out: &mut Vec<Gesture>, name: &'static str, args: Vec<Arg>) {
        let mut with_flag = args.clone();
        with_flag.push(Arg::Bool(self.processed));
        out.push((None, name, with_flag));
        if let Some(target) = self.target {
            out.push((Some(target), name, args));
        }
    }

    fn rebase_pair(&mut self) {
        self.pair = self.spread().map(|(distance, angle)| Pair {
            distance,
            angle,
            began: false,
            scale: 1.0,
            rotation: 0.0,
        });
    }

    fn end_pair(&mut self, out: &mut Vec<Gesture>, positions: &Arg) {
        if let Some(pair) = self.pair.take().filter(|p| p.began) {
            for (name, value) in [("TouchPinch", pair.scale), ("TouchRotate", pair.rotation)] {
                let args = vec![
                    positions.clone(),
                    Arg::Number(f64::from(value)),
                    Arg::Number(0.0),
                    state("End"),
                ];
                self.both(out, name, args);
            }
        }
    }

    /// A finger came down, moved or lifted at `now`; `target` and
    /// `processed` are what the GUI made of it.
    pub(crate) fn touch(
        &mut self,
        id: u32,
        phase: Phase,
        at: [f32; 2],
        now: f64,
        target: Option<Ref>,
        processed: bool,
    ) -> Vec<Gesture> {
        let mut out = Vec::new();
        match phase {
            Phase::Began => {
                if self.fingers.is_empty() {
                    *self = Touches {
                        started: now,
                        target,
                        processed,
                        long: Some(false),
                        last: (at, now),
                        origin: at,
                        ..Touches::default()
                    };
                }
                self.fingers.insert(id, Finger { start: at, at });
                self.most = self.most.max(self.fingers.len());
                if self.fingers.len() > 1 {
                    if self.long == Some(true) {
                        self.both(
                            &mut out,
                            "TouchLongPress",
                            vec![self.positions(), state("End")],
                        );
                    }
                    self.long = None;
                    self.rebase_pair();
                }
                if self.pan {
                    // The centroid jumps with a new finger; the pan carries on.
                    self.origin = sub(self.centroid(), sub(self.last.0, self.origin));
                    self.last.0 = self.centroid();
                }
            }
            Phase::Moved => {
                let Some(finger) = self.fingers.get_mut(&id) else {
                    return out;
                };
                finger.at = at;
                let moved = length(sub(at, finger.start)) > SLOP;
                let centroid = self.centroid();
                let dt = (now - self.last.1).max(1e-3) as f32;
                let step = sub(centroid, self.last.0);
                self.velocity = [step[0] / dt, step[1] / dt];
                self.last = (centroid, now);
                match self.long {
                    Some(true) => {
                        self.both(
                            &mut out,
                            "TouchLongPress",
                            vec![self.positions(), state("Change")],
                        );
                    }
                    Some(false) if moved => self.long = None,
                    _ => {}
                }
                if !self.pan && moved && self.long != Some(true) {
                    self.pan = true;
                    let direction = direction(sub(centroid, self.origin));
                    out.push((
                        None,
                        "TouchDrag",
                        vec![
                            direction,
                            Arg::Number(self.fingers.len() as f64),
                            Arg::Bool(self.processed),
                        ],
                    ));
                    self.pan_event(&mut out, "Begin");
                } else if self.pan {
                    self.pan_event(&mut out, "Change");
                }
                if let (Some((distance, angle)), Some(pair)) = (self.spread(), &mut self.pair) {
                    let scale = distance / pair.distance.max(1.0);
                    let mut rotation = angle - pair.angle;
                    rotation = (rotation + 540.0) % 360.0 - 180.0;
                    let (scale_v, rotation_v) =
                        ((scale - pair.scale) / dt, (rotation - pair.rotation) / dt);
                    let item = if pair.began { "Change" } else { "Begin" };
                    pair.began = true;
                    pair.scale = scale;
                    pair.rotation = rotation;
                    let positions = self.positions();
                    for (name, value, speed) in [
                        ("TouchPinch", scale, scale_v),
                        ("TouchRotate", rotation, rotation_v),
                    ] {
                        let args = vec![
                            positions.clone(),
                            Arg::Number(f64::from(value)),
                            Arg::Number(f64::from(speed)),
                            state(item),
                        ];
                        self.both(&mut out, name, args);
                    }
                }
            }
            Phase::Ended => {
                let positions = self.positions();
                let Some(finger) = self.fingers.remove(&id) else {
                    return out;
                };
                self.end_pair(&mut out, &positions);
                if self.fingers.len() == 1 {
                    self.rebase_pair();
                }
                if !self.fingers.is_empty() {
                    return out;
                }
                if self.long == Some(true) {
                    self.both(
                        &mut out,
                        "TouchLongPress",
                        vec![positions.clone(), state("End")],
                    );
                }
                if self.pan {
                    let total = sub(self.last.0, self.origin);
                    let args = vec![positions, v2(total), v2(self.velocity), state("End")];
                    self.both(&mut out, "TouchPan", args);
                    if length(self.velocity) >= SWIPE_SPEED {
                        let args = vec![direction(self.velocity), Arg::Number(self.most as f64)];
                        self.both(&mut out, "TouchSwipe", args);
                    }
                } else if self.long == Some(false) && now - self.started <= TAP {
                    self.both(&mut out, "TouchTap", vec![positions]);
                    out.push((
                        None,
                        "TouchTapInWorld",
                        vec![v2(finger.at), Arg::Bool(self.processed)],
                    ));
                }
                self.pan = false;
                self.long = None;
            }
        }
        out
    }

    fn pan_event(&self, out: &mut Vec<Gesture>, item: &str) {
        let args = vec![
            self.positions(),
            v2(sub(self.last.0, self.origin)),
            v2(self.velocity),
            state(item),
        ];
        self.both(out, "TouchPan", args);
    }

    /// Time passing with fingers down: a still one becomes a long press.
    pub(crate) fn tick(&mut self, now: f64) -> Vec<Gesture> {
        let mut out = Vec::new();
        if self.long == Some(false) && !self.fingers.is_empty() && now - self.started >= LONG_PRESS
        {
            self.long = Some(true);
            self.both(
                &mut out,
                "TouchLongPress",
                vec![self.positions(), state("Begin")],
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(gestures: &[Gesture]) -> Vec<&'static str> {
        gestures
            .iter()
            .filter(|g| g.0.is_none())
            .map(|g| g.1)
            .collect()
    }

    #[test]
    fn a_quick_still_touch_taps() {
        let mut t = Touches::default();
        assert!(t
            .touch(0, Phase::Began, [10.0, 10.0], 0.0, None, false)
            .is_empty());
        let out = t.touch(0, Phase::Ended, [10.0, 10.0], 0.1, None, false);
        assert_eq!(names(&out), ["TouchTap", "TouchTapInWorld"]);
        assert_eq!(out[1].2[0], Arg::Vector2([10.0, 10.0]));
    }

    #[test]
    fn a_held_finger_long_presses_and_a_dragged_one_pans_then_swipes() {
        let mut t = Touches::default();
        let target = Ref::new(7);
        t.touch(0, Phase::Began, [0.0, 0.0], 0.0, Some(target), true);
        assert!(t.tick(0.3).is_empty());
        let out = t.tick(0.6);
        assert_eq!(names(&out), ["TouchLongPress"]);
        assert_eq!(out[1].0, Some(target));
        let out = t.touch(0, Phase::Ended, [0.0, 0.0], 0.7, None, false);
        assert_eq!(names(&out), ["TouchLongPress"]);

        t.touch(1, Phase::Began, [0.0, 0.0], 1.0, None, false);
        let out = t.touch(1, Phase::Moved, [40.0, 0.0], 1.05, None, false);
        assert_eq!(names(&out), ["TouchDrag", "TouchPan"]);
        let out = t.touch(1, Phase::Ended, [40.0, 0.0], 1.06, None, false);
        assert_eq!(names(&out), ["TouchPan", "TouchSwipe"]);
        assert_eq!(out[0].2[1], Arg::Vector2([40.0, 0.0]));
        assert_eq!(
            out[1].2[0],
            Arg::Enum {
                enum_name: "SwipeDirection".into(),
                item: "Right".into()
            }
        );
    }

    #[test]
    fn two_fingers_pinch_and_rotate() {
        let mut t = Touches::default();
        t.touch(0, Phase::Began, [0.0, 0.0], 0.0, None, false);
        t.touch(1, Phase::Began, [100.0, 0.0], 0.0, None, false);
        let out = t.touch(1, Phase::Moved, [0.0, 200.0], 0.1, None, false);
        let pinch = out.iter().find(|g| g.1 == "TouchPinch").unwrap();
        assert_eq!(pinch.2[1], Arg::Number(2.0));
        let rotate = out.iter().find(|g| g.1 == "TouchRotate").unwrap();
        assert_eq!(rotate.2[1], Arg::Number(90.0));
    }
}
