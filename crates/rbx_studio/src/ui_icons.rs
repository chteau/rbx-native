//! The editor's own UI icon kit: outline glyphs on a 24x24 canvas, stroked
//! in `currentColor`, embedded from `assets/icons/ui`.
//!
//! Two kits, two jobs, and they don't overlap:
//!
//! - **This one** is for *affordances* — a tool, a command, a menu entry, a
//!   chevron. An affordance has states (idle, hover, active, disabled), and
//!   each state wants a different colour, so these have to be monochrome
//!   `currentColor` line art that GPUI's own `svg()` element can re-tint on
//!   the fly. That is what makes `shell::ribbon`'s state matrices possible
//!   at all.
//! - **[`crate::class_icons`]** is for *identity* — what a `Part` or a
//!   `Folder` or a `Script` is. Those are flat, multi-colour, and
//!   deliberately never re-tinted, because the colour is the identity.
//!
//! So: an Explorer row uses `class_icons`; a ribbon tile uses this. A ribbon
//! tile that inserts a `Part` still uses *this* kit, because the tile is an
//! affordance that happens to insert a Part — it goes grey when disabled,
//! and a class icon can't.
//!
//! Stroke widths are authored so the rendered stroke lands where the design
//! spec wants it once GPUI scales the asset down: 2.0 on the 24-unit canvas
//! renders as 1.5px at the kit's dominant 18px drawing size, and the
//! chevrons' 3.0 renders as 1.25px at the 10px they're drawn at. A single
//! asset can't hold both, so an icon drawn at a size far from its intended
//! one will read slightly heavier or lighter than the spec's nominal width.

use gpui_kit::component::Icon;

#[derive(rust_embed::RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/../../assets/icons/ui"]
struct UiIcons;

/// The icon named `name` (a bare filename stem under `assets/icons/ui`),
/// ready to be sized and tinted by the caller.
///
/// A name with no file renders as nothing rather than panicking — but that
/// is a bug, not a supported path, and this module's tests fail on it
/// before it can reach a window.
pub(crate) fn icon(name: &str) -> Icon {
    match UiIcons::get(&format!("{name}.svg")) {
        Some(file) => Icon::default().data(&file.data),
        None => Icon::empty(),
    }
}

#[cfg(test)]
#[path = "ui_icons/tests.rs"]
mod tests;
