//! §6 — the hover label an icon-only button needs to be more than a
//! pictogram.
//!
//! A thin wrapper over the toolkit's own tooltip rather than a hand-rolled
//! overlay: hover timing, anchoring, flipping when there's no room above,
//! and layering above everything else in the window are all already solved
//! there, and none of them are what this editor has anything new to say
//! about. What that costs is the spec's 4px triangle pointer, which the
//! stock tooltip doesn't draw, and control over the exact hover delay.
//!
//! The text doubles as the accessible label for the control it belongs to
//! (§7.3), which is why every icon-only button in the shell has one.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;

pub(crate) fn text(label: impl Into<SharedString>, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::new(label.into()).build(window, cx)
}
