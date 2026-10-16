//! rbx-native addition: hover styles that ease in and out.
//!
//! Upstream applies an element's `hover` (and `group_hover`) refinement all
//! at once the frame the pointer crosses its edge. With a [`HoverTransition`]
//! global set, the colours that refinement changes — fill, border, text and
//! shadow — instead blend from the element's resting style to its hovered one
//! over the global's duration, and back when the pointer leaves, reversing
//! from wherever the blend had got to; so does a width or height given in
//! pixels at both ends. Everything else in the refinement (a cursor, an
//! opacity, a fractional size) still applies at once, for as long as the
//! element is hovered.
//!
//! Only elements with an id ease: the blend's progress lives in the element
//! state an id keys. With no global, a zero duration, or
//! [`App::reduce_motion`](crate::App::reduce_motion) set, hover snaps as
//! upstream's does.

use std::time::{Duration, Instant};

use crate::{
    AbsoluteLength, BoxShadow, DefiniteLength, Fill, Global, Hsla, Length, Rgba, Style,
    StyleRefinement, px,
};

/// How long a hover style takes to fade fully in or out. Install it with
/// `cx.set_global(HoverTransition { duration })`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoverTransition {
    /// Time from the resting style to the fully hovered one. A partial
    /// reversal takes the matching share of it.
    pub duration: Duration,
}

impl Global for HoverTransition {}

/// One element's hover blend: which way it is heading and since when.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HoverFade {
    hovered: bool,
    from: f32,
    since: Option<Instant>,
}

impl HoverFade {
    /// The blend at `now` (0 = resting, 1 = hovered) once the element is
    /// `hovered` or not, restarting from the current blend when that flips.
    pub fn progress(&mut self, hovered: bool, now: Instant, duration: Duration) -> f32 {
        let current = self.sample(now, duration);
        if hovered != self.hovered {
            *self = Self {
                hovered,
                from: current,
                since: Some(now),
            };
        }
        current
    }

    fn sample(&self, now: Instant, duration: Duration) -> f32 {
        let to = if self.hovered { 1. } else { 0. };
        let Some(since) = self.since else {
            return to;
        };
        let span = duration.as_secs_f32() * (to - self.from).abs();
        if span <= 0. {
            return to;
        }
        let t = (now.saturating_duration_since(since).as_secs_f32() / span).min(1.);
        self.from + (to - self.from) * ease_out(t)
    }
}

fn ease_out(t: f32) -> f32 {
    1. - (1. - t).powi(3)
}

/// `a` blended toward `b` by `t`, with premultiplied alpha so that fading in
/// from transparent never passes through some other hue.
pub fn mix_colors(a: Hsla, b: Hsla, t: f32) -> Hsla {
    let (a, b) = (Rgba::from(a), Rgba::from(b));
    let alpha = a.a + (b.a - a.a) * t;
    if alpha <= 0. {
        return Hsla::transparent_black();
    }
    let channel = |x: f32, xa: f32, y: f32, ya: f32| (x * xa + (y * ya - x * xa) * t) / alpha;
    Hsla::from(Rgba {
        r: channel(a.r, a.a, b.r, b.a),
        g: channel(a.g, a.a, b.g, b.a),
        b: channel(a.b, a.a, b.b, b.a),
        a: alpha,
    })
}

/// The refinement to lay over `base` at blend `t` toward `hover`: its colours
/// and pixel sizes blended, the rest of it only while `hovered`.
pub fn blend_refinement(
    base: &Style,
    hover: &StyleRefinement,
    t: f32,
    hovered: bool,
) -> StyleRefinement {
    if hovered && t >= 1. {
        return hover.clone();
    }
    let mut out = if hovered {
        hover.clone()
    } else {
        StyleRefinement::default()
    };
    // A colour the base leaves unset fades from the hovered one made clear;
    // one inherited from a parent is unknown here, so it switches halfway.
    let half = t >= 0.5;

    out.background = match hover.background.as_ref().map(solid) {
        Some(Some(to)) => {
            let from = base
                .background
                .as_ref()
                .and_then(solid)
                .unwrap_or(to.opacity(0.));
            Some(mix_colors(from, to, t).into())
        }
        Some(None) => hover.background.clone().filter(|_| half),
        None => None,
    };
    out.border_color = hover.border_color.map(|to| {
        let from = base.border_color.unwrap_or(to.opacity(0.));
        mix_colors(from, to, t)
    });
    out.text.color = hover.text.color.and_then(|to| match base.text.color {
        Some(from) => Some(mix_colors(from, to, t)),
        None => half.then_some(to),
    });
    out.box_shadow = hover
        .box_shadow
        .as_ref()
        .and_then(|to| blend_shadows(&base.box_shadow, to, t, half));
    // A size in pixels at both ends (a hover lift) eases too; a fraction or
    // `auto` is no distance to blend across, so it holds while hovered.
    out.size.width = blend_length(base.size.width, hover.size.width, t, hovered);
    out.size.height = blend_length(base.size.height, hover.size.height, t, hovered);
    out
}

fn blend_length(from: Length, to: Option<Length>, t: f32, hovered: bool) -> Option<Length> {
    let to = to?;
    match (pixels(from), pixels(to)) {
        (Some(a), Some(b)) => Some(px(a + (b - a) * t).into()),
        _ => hovered.then_some(to),
    }
}

fn pixels(length: Length) -> Option<f32> {
    match length {
        Length::Definite(DefiniteLength::Absolute(AbsoluteLength::Pixels(pixels))) => {
            Some(f32::from(pixels))
        }
        _ => None,
    }
}

fn solid(fill: &Fill) -> Option<Hsla> {
    fill.color().and_then(|background| background.as_solid())
}

fn blend_shadows(
    from: &[BoxShadow],
    to: &[BoxShadow],
    t: f32,
    half: bool,
) -> Option<Vec<BoxShadow>> {
    if from.is_empty() {
        return Some(
            to.iter()
                .map(|shadow| BoxShadow {
                    color: shadow.color.opacity(t),
                    ..shadow.clone()
                })
                .collect(),
        );
    }
    if from.len() != to.len() {
        return half.then(|| to.to_vec());
    }
    Some(
        from.iter()
            .zip(to)
            .map(|(a, b)| BoxShadow {
                color: mix_colors(a.color, b.color, t),
                ..b.clone()
            })
            .collect(),
    )
}
