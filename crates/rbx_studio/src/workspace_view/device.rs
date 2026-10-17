//! The device the view simulates (see `shell::device_sim`): the safe-area
//! insets its `ScreenGui`s keep clear of, a bezel around its screen, and the
//! badge naming it.
//!
//! In Edit the screen is the letterboxed box `frame::Viewport::letterboxed`
//! scales to fit. During Play the embedded client is a foreign window that
//! can be sized but never scaled, so the screen is the device's own size at
//! 1:1 centred in the panel (`device_sim::centred`), with a plain backdrop
//! around it — the same rect `Shell::play_embed_target` gives the client.
//! Display scaling applies to Edit only; Play stays 1:1.
//!
//! A keyboard cover (`keyboard`) can sit over the bottom of the screen, and
//! during Play the badge also carries what the client reports about itself.

mod keyboard;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_viewer::SafeInsets;

use super::frame::Viewport;
use super::WorkspaceView;
use crate::device_sim::{self, Rect};
use crate::tokens;

/// How far the bezel outline stands off the screen, in logical pixels.
const BEZEL: f32 = 6.0;
/// The corner ring's width (`border_4`), inside the bezel's 2-pixel outline.
const MASK: f32 = 4.0;

/// What `Shell` tells the view about the simulated device.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Simulated {
    /// "iPhone 15 Pro · 852×393 Landscape", shown in the badge.
    pub(crate) label: SharedString,
    pub(crate) insets: SafeInsets,
    /// Whether the Play client can take the device's shape at all (a
    /// headset cannot: Sober has no VR).
    pub(crate) plays: bool,
    /// Editor logical pixels per device point in Edit
    /// (`device_sim::DisplayScaling`); `None` fits the panel.
    pub(crate) logical_per_point: Option<f32>,
    /// The on-screen keyboard cover's height in device points; 0 hides it.
    pub(crate) keyboard: u32,
    /// What the Play client reports (`device_sim::report::summary`), and
    /// whether its viewport differs from the screen.
    pub(crate) client: Option<(SharedString, bool)>,
}

impl WorkspaceView {
    /// Simulates `device` on the screen `set_screen` gave the view, or stops
    /// with `None`. Call it after `set_screen`, which forgets the device.
    pub(crate) fn set_device(&mut self, device: Option<Simulated>, cx: &mut Context<Self>) {
        if device == self.device {
            return;
        }
        self.pump
            .gui_safe_insets(device.as_ref().map(|d| d.insets).unwrap_or_default());
        self.device = device;
        cx.notify();
    }

    /// How far the letterboxed screen stays off the panel's edges in Edit,
    /// in logical pixels: room for the bezel.
    pub(super) fn device_margin(&self) -> f32 {
        match self.device.is_some() && self.screen.is_some() {
            true => BEZEL * 2.0,
            false => 0.0,
        }
    }

    /// Whether a Play client is embedded in the view, which then shows the
    /// device's screen at 1:1 rather than scaled.
    pub(crate) fn set_playing(&mut self, playing: bool, cx: &mut Context<Self>) {
        if playing != self.playing {
            self.playing = playing;
            cx.notify();
        }
    }

    /// Where the Play client goes in a panel at `panel` (window pixels):
    /// the device's screen centred at 1:1, or the whole panel when no
    /// device is simulated, the device cannot play, or it does not fit.
    pub(crate) fn play_rect(&self, panel: Rect) -> Rect {
        self.play_screen(panel).unwrap_or(panel)
    }

    fn play_screen(&self, panel: Rect) -> Option<Rect> {
        let screen = self.screen?;
        self.device
            .as_ref()
            .filter(|device| device.plays)
            .and_then(|_| device_sim::centred(panel, screen))
    }

    /// The backdrop, bezel and badge over the panel, in its own logical
    /// pixels; nothing while no device is simulated.
    pub(super) fn device_overlay(&self, scale: f32) -> Option<AnyElement> {
        let device = self.device.as_ref()?;
        let panel = self.panel.get();
        let local = |device_px: i64| px(device_px as f32 / scale);
        let (screen, note) = if self.playing {
            let whole = to_rect(panel);
            match self.play_screen(whole) {
                Some(rect) => (Some(rect), None),
                None if !device.plays => {
                    (None, Some("Play: not available \u{2014} Sober has no VR"))
                }
                None => (
                    None,
                    Some("Doesn\u{2019}t fit \u{2014} enlarge the 3D view"),
                ),
            }
        } else {
            (Some(to_rect(self.viewport.get())), None)
        };
        let bezel = screen.map(|rect| {
            div()
                .absolute()
                .left(local(i64::from(rect.x) - i64::from(panel.origin.0)) - px(BEZEL))
                .top(local(i64::from(rect.y) - i64::from(panel.origin.1)) - px(BEZEL))
                .w(local(i64::from(rect.width)) + px(BEZEL * 2.0))
                .h(local(i64::from(rect.height)) + px(BEZEL * 2.0))
                .border_2()
                .border_color(tokens::check_off_border())
                .rounded(px(BEZEL * 3.0))
        });
        // The screen's own rounded corners, concentric with the bezel: a
        // ring of the backdrop colour whose inner edge has the screen's
        // radius, painted over the scene's (and the cover's) square corners.
        // During Play the client window's corners are cut to match
        // (`device_sim::corner_holes`), so this ring shows through there.
        let corners = screen.map(|rect| {
            div()
                .absolute()
                .left(local(i64::from(rect.x) - i64::from(panel.origin.0)) - px(MASK))
                .top(local(i64::from(rect.y) - i64::from(panel.origin.1)) - px(MASK))
                .w(local(i64::from(rect.width)) + px(MASK * 2.0))
                .h(local(i64::from(rect.height)) + px(MASK * 2.0))
                .border_4()
                .border_color(tokens::viewport_background())
                .rounded(px(device_sim::SCREEN_RADIUS + MASK))
        });
        let cover = match (screen, self.screen) {
            (Some(rect), Some(points)) if device.keyboard > 0 && points.1 > 0 => {
                let height = (i64::from(device.keyboard) * i64::from(rect.height)
                    / i64::from(points.1))
                .min(i64::from(rect.height));
                Some(keyboard::cover(
                    local(i64::from(rect.x) - i64::from(panel.origin.0)),
                    local(
                        i64::from(rect.y) + i64::from(rect.height)
                            - height
                            - i64::from(panel.origin.1),
                    ),
                    local(i64::from(rect.width)),
                    local(height),
                    self.playing,
                ))
            }
            _ => None,
        };
        let mut label = match note {
            Some(note) => format!("{} \u{b7} {note}", device.label),
            None => device.label.to_string(),
        };
        let mut mismatch = false;
        if let Some((summary, differs)) = &device.client {
            label.push_str(&format!(" \u{b7} {summary}"));
            mismatch = *differs;
            if let (true, Some((w, h))) = (mismatch, self.screen) {
                label.push_str(&format!(" \u{b7} expected {w}\u{d7}{h}"));
            }
        }
        let badge_text = if mismatch {
            tokens::warning()
        } else {
            tokens::viewport_badge_text()
        };
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                // During Play the scene behind the client is the edit-time
                // one, laid out for another shape: a plain backdrop instead.
                .when(self.playing && screen.is_some(), |this| {
                    this.bg(tokens::viewport_background())
                })
                .children(bezel)
                .children(cover)
                .children(corners)
                .child(
                    div()
                        .absolute()
                        .top_2()
                        .left_2()
                        .px_2()
                        .py_0p5()
                        .bg(tokens::viewport_badge())
                        .text_size(tokens::viewport_text(12.))
                        .text_color(badge_text)
                        .child(label)
                        // Cut out of the Play client, which covers the badge
                        // whenever it fills the panel (a device that does
                        // not fit, a headset).
                        .child(gpui_base::occlusion::probe()),
                )
                .into_any_element(),
        )
    }
}

fn to_rect(viewport: Viewport) -> Rect {
    Rect {
        x: i32::try_from(viewport.origin.0).unwrap_or(i32::MAX),
        y: i32::try_from(viewport.origin.1).unwrap_or(i32::MAX),
        width: viewport.size.0,
        height: viewport.size.1,
    }
}
