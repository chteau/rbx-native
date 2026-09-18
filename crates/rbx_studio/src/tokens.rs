//! The editor's design tokens: every colour, radius, spacing step,
//! elevation, duration, easing curve and text style the chrome is allowed to
//! use, in one place.
//!
//! The discipline here is borrowed from Vercel's Geist design system — a
//! fixed radius scale, a base-4 spacing scale, and "shadow-as-border"
//! elevation (a 1px ring layer plus a soft blur layer, instead of a hard
//! `border`). What is deliberately *not* borrowed is Geist's light,
//! monochrome, tight-and-flat look: this palette is dark and desaturated,
//! the radii are rounder, the shadows are softer and blurrier, and the
//! motion is elastic rather than minimal.
//!
//! **Nothing outside this module may invent a colour, radius, spacing step
//! or shadow.** A literal `rgb(0x…)`/`px(13.)` in panel chrome is a bug: it
//! drifts the moment a token changes. Reach for a token, and if none fits,
//! add one here with a name that says what it is for.
//!
//! Tokens with nothing to bind to were left out rather than parked here as
//! decoration: GPUI has no property transitions, no element transform and
//! no text-transform, so the spec's press curve, hover durations and
//! letter-spacing have no expression to encode — and this editor has no
//! modal surface for a modal elevation to describe yet. `UX_GUIDELINES.md`
//! §10 lists every one of those, so re-adding a token means implementing
//! what it is for.

use std::time::Duration;

use gpui_kit::{px, rgb, rgba, BoxShadow, FontWeight, Pixels, Rgba};

// ---------------------------------------------------------------- 0.1 colour

/// App shell background — the outermost surface, behind every panel.
pub(crate) fn bg_0() -> Rgba {
    rgb(0x16171A)
}

/// Dock/panel background: Explorer, Properties, Output, the ribbon strip.
pub(crate) fn bg_1() -> Rgba {
    rgb(0x1C1D21)
}

/// Raised surface — hover backgrounds, popovers, menu containers.
pub(crate) fn bg_2() -> Rgba {
    rgb(0x22232A)
}

/// Pressed/active background, and the fill behind numeric inputs.
pub(crate) fn bg_3() -> Rgba {
    rgb(0x2A2B33)
}

/// The 3D viewport's own background — darker than any panel, so the render
/// reads as a window cut into the shell rather than another panel.
pub(crate) fn bg_viewport() -> Rgba {
    rgb(0x101114)
}

/// Panel separators. Deliberately near-invisible: panels are told apart by
/// the luminance step between `bg_0`/`bg_1` and by elevation, not by lines
/// (see `UX_GUIDELINES.md` §3 for why this is not a contrast bug).
pub(crate) fn border_soft() -> Rgba {
    rgba(0xFFFFFF0F)
}

/// Input outlines and menu container edges — a functional boundary, so it
/// reads a step stronger than [`border_soft`].
pub(crate) fn border_mid() -> Rgba {
    rgba(0xFFFFFF1A)
}

pub(crate) fn text_primary() -> Rgba {
    rgb(0xE8E8EA)
}

pub(crate) fn text_secondary() -> Rgba {
    rgb(0x9A9AA0)
}

pub(crate) fn text_disabled() -> Rgba {
    rgb(0x5A5A60)
}

pub(crate) fn text_error() -> Rgba {
    rgb(0xFF6B6B)
}

/// The one saturated hue in the whole editor: active states, selection,
/// focus. Everything else is neutral.
pub(crate) fn accent() -> Rgba {
    rgb(0x6C8CFF)
}

/// The tint behind a committed active state — an active tab's pill, a
/// selected row.
pub(crate) fn accent_soft_bg() -> Rgba {
    rgba(0x6C8CFF1F)
}

/// [`accent_soft_bg`] at the 1.08x opacity the spec asks for when an
/// already-active element is also hovered.
pub(crate) fn accent_soft_bg_hover() -> Rgba {
    rgba(0x6C8CFF21)
}

/// The outer glow of the keyboard focus halo — see [`focus_ring`], which is
/// what call sites actually use.
pub(crate) fn focus_ring_color() -> Rgba {
    rgba(0x6C8CFF73)
}

// ---------------------------------------------------------------- 0.2 radius

/// Checkboxes, small chips, an input's spinner buttons.
pub(crate) const RADIUS_XS: Pixels = px(4.);
/// Buttons, input fields, menu items.
pub(crate) const RADIUS_SM: Pixels = px(8.);
/// Ribbon groups, tab pills, popovers.
pub(crate) const RADIUS_MD: Pixels = px(12.);
/// Dock panel outer corners, dropdown menu containers.
pub(crate) const RADIUS_LG: Pixels = px(16.);
/// As round as the shape allows — the active tab's pill.
pub(crate) const RADIUS_PILL: Pixels = px(999.);

// --------------------------------------------------------------- 0.3 spacing

pub(crate) const SPACE_1: Pixels = px(4.);
pub(crate) const SPACE_2: Pixels = px(8.);
pub(crate) const SPACE_3: Pixels = px(12.);
pub(crate) const SPACE_4: Pixels = px(16.);

// ------------------------------------------------------------- 0.4 elevation

/// Which way an elevation's blur layer is thrown. A panel casts its shadow
/// *away* from the surface it sits against: the left column throws right,
/// the right column throws left, a bottom dock throws up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cast {
    Down,
    Up,
    Left,
    Right,
}

impl Cast {
    fn offset(self, distance: f32) -> (Pixels, Pixels) {
        match self {
            Cast::Down => (px(0.), px(distance)),
            Cast::Up => (px(0.), px(-distance)),
            Cast::Left => (px(-distance), px(0.)),
            Cast::Right => (px(distance), px(0.)),
        }
    }
}

/// A hairline ring drawn *as* a shadow rather than a border, so it costs no
/// layout box and can't shift the element it outlines — the Geist trick this
/// whole elevation scale is built on.
fn ring(color: Rgba, width: f32) -> BoxShadow {
    BoxShadow {
        color: color.into(),
        offset: gpui_kit::point(px(0.), px(0.)),
        blur_radius: px(0.),
        spread_radius: px(width),
        inset: false,
    }
}

fn blur(color: Rgba, cast: Cast, distance: f32, blur_radius: f32) -> BoxShadow {
    let (x, y) = cast.offset(distance);
    BoxShadow {
        color: color.into(),
        offset: gpui_kit::point(x, y),
        blur_radius: px(blur_radius),
        spread_radius: px(0.),
        inset: false,
    }
}

/// The ribbon strip against the shell behind it.
pub(crate) fn elevation_1() -> Vec<BoxShadow> {
    vec![
        ring(border_soft(), 1.),
        blur(rgba(0x00000040), Cast::Down, 1., 2.),
    ]
}

/// Dropdown menus and popovers — off the surface, but still attached to the
/// control that opened them.
pub(crate) fn elevation_2() -> Vec<BoxShadow> {
    vec![
        ring(border_mid(), 1.),
        blur(rgba(0x00000059), Cast::Down, 4., 16.),
    ]
}

/// Dock panels against the viewport. `cast` points away from the viewport,
/// so the panel reads as sitting over it.
pub(crate) fn elevation_3(cast: Cast) -> Vec<BoxShadow> {
    vec![
        ring(border_soft(), 1.),
        blur(rgba(0x00000066), cast, 8., 24.),
    ]
}

// --------------------------------------------------------------- 0.5 motion

/// Menu and popover open/close.
pub(crate) const DURATION_MENU: Duration = Duration::from_millis(220);

/// `cubic-bezier(0.16, 1, 0.3, 1)` — the default "liquid" ease-out.
pub(crate) fn easing_soft(delta: f32) -> f32 {
    cubic_bezier(0.16, 1., 0.3, 1., delta)
}

/// Evaluates a CSS `cubic-bezier(x1, y1, x2, y2)` at `t`.
///
/// CSS timing functions are parametric curves, not functions of `t`: the
/// value wanted is `y` at the point whose `x` equals `t`, so the parameter
/// `s` where `x(s) == t` has to be solved for first. Newton-Raphson
/// converges in a handful of steps for the well-behaved (monotonic-x)
/// curves a timing function is allowed to be; the bisection fallback is
/// there for the flat-derivative case Newton can't step out of.
fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    // A cubic Bézier with endpoints fixed at (0,0) and (1,1) reduces to
    // these coefficients on each axis.
    let bezier = |a1: f32, a2: f32, s: f32| {
        let c = 3. * a1;
        let b = 3. * (a2 - a1) - c;
        let a = 1. - c - b;
        ((a * s + b) * s + c) * s
    };
    let slope = |a1: f32, a2: f32, s: f32| {
        let c = 3. * a1;
        let b = 3. * (a2 - a1) - c;
        let a = 1. - c - b;
        (3. * a * s + 2. * b) * s + c
    };

    let mut s = t;
    for _ in 0..8 {
        let x = bezier(x1, x2, s) - t;
        if x.abs() < 1e-5 {
            return bezier(y1, y2, s);
        }
        let d = slope(x1, x2, s);
        if d.abs() < 1e-6 {
            break;
        }
        s -= x / d;
    }

    let (mut low, mut high) = (0., 1.);
    let mut s = t;
    for _ in 0..24 {
        let x = bezier(x1, x2, s);
        if (x - t).abs() < 1e-5 {
            break;
        }
        if x > t {
            high = s;
        } else {
            low = s;
        }
        s = (low + high) / 2.;
    }
    bezier(y1, y2, s)
}

// ----------------------------------------------------------- 0.6 focus ring

/// The keyboard focus halo: a 2px gap of background colour, then a 2px
/// accent glow, so the ring reads as a halo around the control rather than a
/// hard outline welded to its edge.
///
/// `surface` is whatever the focused element sits on — the gap has to be
/// painted in that colour to read as a gap at all.
pub(crate) fn focus_ring(surface: Rgba) -> Vec<BoxShadow> {
    vec![ring(surface, 2.), ring(focus_ring_color(), 4.)]
}

// ----------------------------------------------------------- 0.7 typography

/// Body text on chrome: a panel label, a tab title, a menu item.
pub(crate) const UI_LABEL_SIZE: Pixels = px(13.);
pub(crate) const UI_LABEL_LINE_HEIGHT: Pixels = px(18.2);
pub(crate) const UI_LABEL_WEIGHT: FontWeight = FontWeight::NORMAL;
/// The same text once its element is the committed/active one.
pub(crate) const UI_LABEL_ACTIVE_WEIGHT: FontWeight = FontWeight::SEMIBOLD;

/// A group caption inside the ribbon, a section header in Properties.
/// Rendered uppercase by the call site (GPUI has no `text-transform`).
pub(crate) const SECTION_HEADER_SIZE: Pixels = px(12.);
pub(crate) const SECTION_HEADER_LINE_HEIGHT: Pixels = px(15.6);
pub(crate) const SECTION_HEADER_WEIGHT: FontWeight = FontWeight::SEMIBOLD;

/// One Explorer row.
pub(crate) const TREE_ROW_SIZE: Pixels = px(13.);
pub(crate) const TREE_ROW_LINE_HEIGHT: Pixels = px(19.5);

/// A numeric field's own value — monospaced, so digits don't jitter as they
/// change during a drag.
pub(crate) const INPUT_VALUE_SIZE: Pixels = px(13.);
pub(crate) const INPUT_VALUE_LINE_HEIGHT: Pixels = px(18.2);
pub(crate) const INPUT_VALUE_WEIGHT: FontWeight = FontWeight::MEDIUM;

/// The UI font stack, in preference order. Resolved by the text system at
/// startup (see `main::install_theme`); a family that isn't installed falls
/// through to the next.
pub(crate) const FONT_FAMILY_UI: &str = "Inter";
/// Numeric inputs and the script editor.
pub(crate) const FONT_FAMILY_MONO: &str = "JetBrains Mono";

#[cfg(test)]
#[path = "tokens/tests.rs"]
mod tests;
