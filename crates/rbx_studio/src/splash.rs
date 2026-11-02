//! The splash a bare `rbxstudio` shows while start-up reads the keychain:
//! the logo's two halves spin in from far away and settle into the README
//! mark. `main` opens it before loading starts, awaits [`Splash::finished`]
//! once loading is done, and the splash closes itself as soon as the next
//! window's root view exists, so the app never has zero windows (GPUI's
//! Linux back ends quit then).

use std::cell::Cell;
use std::f32::consts::PI;
use std::rc::Rc;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use gpui_kit::component::{Icon, Root};
use gpui_kit::*;

use crate::tokens;

const LOGO: &[u8] = include_bytes!("../../../assets/icons/brand/rbxnative-logo.svg");
/// The mark's own viewBox, so a half can be placed in logo units.
const LOGO_SIZE: (f32, f32) = (23.8, 17.);
/// Each half's bounding-box centre in logo units, what it spins around.
const HALF_CENTRES: [(f32, f32); 2] = [(7.65, 9.222), (16.102, 8.115)];
/// README's 100px width, nudged up so the mark reads at splash distance.
const LOGO_WIDTH: f32 = 112.;
const WINDOW_SIZE: (f32, f32) = (300., 220.);

/// The motion, then a short rest on the finished mark so it registers
/// before the window goes. Together they stay under Chee's one second.
const MOTION: Duration = Duration::from_millis(820);
const REST: Duration = Duration::from_millis(120);
/// A splash that never painted (no compositor frame, a hung GPU) must not
/// hold start-up hostage.
const GIVE_UP: Duration = Duration::from_secs(2);

/// The two `<path>`s of the logo, each in an SVG of its own with the full
/// viewBox, so both draw in the same box and line up exactly at rest.
static HALVES: LazyLock<[Vec<u8>; 2]> = LazyLock::new(|| split(LOGO));

fn split(logo: &[u8]) -> [Vec<u8>; 2] {
    let logo = std::str::from_utf8(logo).expect("the logo is UTF-8");
    let start = logo.find("<path").expect("the logo has paths");
    let end = logo.rfind("</g>").expect("the logo's paths are grouped");
    let (head, tail) = (&logo[..start], &logo[end..]);
    let paths: Vec<&str> = logo[start..end]
        .split_inclusive("/>")
        .map(str::trim)
        .filter(|path| path.starts_with("<path"))
        .collect();
    let [left, right] = paths[..] else {
        panic!("the logo has {} paths, the splash expects two", paths.len());
    };
    [left, right].map(|path| format!("{head}{path}\n{tail}").into_bytes())
}

/// Cubic ease-in-out: slow away from the distance, slow into place.
fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}

/// Where half `index` is `t` of the way through the motion. GPUI turns and
/// scales an SVG about its box centre, which is the whole logo's; shifting
/// by `d - s·R·d` (d = half centre − logo centre) makes it turn about the
/// half's own centre instead.
fn pose(index: usize, t: f32, unit: f32) -> Transformation {
    let e = ease_in_out(t);
    // Opposite turns and opposite sides, so the halves visibly meet.
    let side = if index == 0 { -1. } else { 1. };
    let scale = 0.08 + 0.92 * e;
    let angle = side * 1.25 * 2. * PI * (1. - e);
    let (cx, cy) = HALF_CENTRES[index];
    let d = (
        (cx - LOGO_SIZE.0 / 2.) * unit,
        (cy - LOGO_SIZE.1 / 2.) * unit,
    );
    let (sin, cos) = angle.sin_cos();
    let turned = (
        scale * (d.0 * cos - d.1 * sin),
        scale * (d.0 * sin + d.1 * cos),
    );
    let reach = (1. - e) * LOGO_WIDTH * 0.55;
    let fly = (side * reach, side * reach * 0.35);
    Transformation::scale(size(scale, scale))
        .with_rotation(radians(angle))
        .with_translation(point(
            px(fly.0 + d.0 - turned.0),
            px(fly.1 + d.1 - turned.1),
        ))
}

struct SplashView {
    /// Set on the first paint, not at open: mapping the window takes a
    /// moment, and the motion should start where it can be seen.
    started: Option<Instant>,
    done: Rc<Cell<bool>>,
    _close_on_next_window: Subscription,
}

impl Render for SplashView {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let started = *self.started.get_or_insert_with(Instant::now);
        let elapsed = started.elapsed();
        let reduced = tokens::reduced_motion();
        let t = if reduced {
            1.
        } else {
            elapsed.as_secs_f32() / MOTION.as_secs_f32()
        };
        if reduced || elapsed >= MOTION + REST {
            self.done.set(true);
        } else {
            window.request_animation_frame();
        }

        let unit = LOGO_WIDTH / LOGO_SIZE.0;
        let (w, h) = (px(LOGO_WIDTH), px(LOGO_SIZE.1 * unit));
        let mark = |data: &[u8], alpha: f32| {
            Icon::empty()
                .data(data)
                .w(w)
                .h(h)
                .text_color(tokens::text_full().opacity(alpha))
        };
        let logo = if t >= 1. {
            // At rest, the README file itself rather than its two halves.
            div().size_full().child(mark(LOGO, 1.)).into_any_element()
        } else {
            // A faint start, so nothing pops in at the first frame.
            let alpha = (t / 0.3).min(1.);
            let half = |index: usize| {
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .child(mark(&HALVES[index], alpha).transform(pose(index, t, unit)))
            };
            div()
                .relative()
                .size_full()
                .child(half(0))
                .child(half(1))
                .into_any_element()
        };
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(tokens::dock())
            .border_1()
            .border_color(tokens::border())
            .rounded(tokens::radius())
            .child(div().w(w).h(h).child(logo))
    }
}

/// The open splash. [`open`] gives `None` if the window could not open,
/// and start-up simply goes on without it.
pub(crate) struct Splash {
    done: Rc<Cell<bool>>,
}

impl Splash {
    /// Resolves once the logo has landed (at once under reduced motion).
    pub(crate) async fn finished(self, cx: &AsyncApp) {
        let deadline = Instant::now() + GIVE_UP;
        while !self.done.get() && Instant::now() < deadline {
            cx.background_executor()
                .timer(Duration::from_millis(16))
                .await;
        }
    }
}

pub(crate) fn open(cx: &mut App) -> Option<Splash> {
    let size = gpui_kit::size(
        tokens::scaled_width(WINDOW_SIZE.0),
        tokens::scaled_width(WINDOW_SIZE.1),
    );
    let options = WindowOptions {
        app_id: Some(crate::APP_ID.to_owned()),
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(None, size, cx))),
        titlebar: None,
        is_resizable: false,
        is_movable: false,
        window_decorations: Some(WindowDecorations::Client),
        window_background: crate::theme::active().effects.window,
        ..Default::default()
    };
    let done = Rc::new(Cell::new(false));
    let shared = done.clone();
    cx.open_window(options, move |window, cx| {
        let splash = window.window_handle();
        // Every window after this one is built on a `Root`; deferred so the
        // new window is fully open before this, the last other one, goes.
        let close = cx.observe_new(move |_: &mut Root, _, cx| {
            cx.defer(move |cx| {
                let _ = splash.update(cx, |_, window, _| window.remove_window());
            });
        });
        cx.new(|_| SplashView {
            started: None,
            done: shared,
            _close_on_next_window: close,
        })
    })
    .ok()?;
    Some(Splash { done })
}

#[cfg(test)]
mod tests {
    use super::{ease_in_out, pose, split, LOGO, LOGO_SIZE, LOGO_WIDTH, MOTION, REST};
    use gpui_kit::Transformation;
    use std::time::Duration;

    #[test]
    fn the_halves_are_the_logos_two_paths() {
        for (half, id) in split(LOGO)
            .iter()
            .zip([r#"id="Vector""#, r#"id="Vector_2""#])
        {
            let half = std::str::from_utf8(half).unwrap();
            assert_eq!(half.matches("<path").count(), 1);
            assert!(half.contains(id));
            assert!(half.contains(r#"viewBox="0 0 23.8 17""#));
        }
    }

    #[test]
    fn the_motion_lands_exactly_on_the_logo() {
        for index in 0..2 {
            let rest = pose(index, 1., LOGO_WIDTH / LOGO_SIZE.0);
            assert_eq!(rest, Transformation::default());
        }
        assert_eq!(ease_in_out(0.), 0.);
        assert_eq!(ease_in_out(0.5), 0.5);
        assert_eq!(ease_in_out(1.), 1.);
    }

    #[test]
    fn it_lasts_under_a_second() {
        assert!(MOTION + REST <= Duration::from_secs(1));
    }
}
