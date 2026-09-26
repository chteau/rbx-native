//! Themes: the editor's whole look as data. A theme is a folder with a
//! `manifest.json` (who made it, what it is, a preview), and optionally a
//! `theme.json` (the palette, the size/type scale, window and hover
//! effects, a background image), a `widgets.json` (a GPUI Kit `ThemeSet` for
//! the toolkit's own widgets) and an `icons/` folder (an icon pack).
//!
//! The editor's own look is the built-in `Default` theme, shipped in
//! `assets/themes/default` and embedded. Its `theme.json` holds *every*
//! token, which makes it both the source of truth `tokens` reads and the
//! template a theme author copies. Every other theme is layered over it:
//! whatever a theme leaves out is Default's, so a theme that recolours one
//! accent is a valid theme, and a theme without icons draws Default's.
//!
//! `tokens` reads the active [`Palette`] on every call through [`color`] and
//! [`size`]. It is process-wide state for the same reason `tokens`' UI scale
//! is: the readers are styling callbacks with no `App` to hand.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, RwLock};

use gpui_kit::{BoxShadow, ObjectFit, Rgba, WindowBackgroundAppearance};

mod apply;
mod github;
mod overrides;
mod pack;
mod palette;
mod watch;

pub(crate) use apply::{apply, startup};
pub(crate) use overrides::Overrides;
pub(crate) use pack::{installed, themes_dir, ThemePack};
pub(crate) use watch::{Watch, POLL_INTERVAL};

/// The folder name, and `appearance.json` value, of the built-in theme. No
/// installed theme may take it, which is what makes Default impossible to
/// uninstall or shadow.
pub(crate) const DEFAULT_ID: &str = "default";

/// Everything a theme decides, resolved: every token has a value, every
/// `@reference` has been followed, every path points inside the theme.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Palette {
    colors: HashMap<String, Rgba>,
    sizes: HashMap<String, f32>,
    pub(crate) effects: Effects,
}

/// What a theme can do beyond recolouring and resizing.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Effects {
    /// Transparent and blurred windows only show the desktop through
    /// surfaces the theme also gave some transparency; blur needs a
    /// compositor that offers it and falls back to plain transparency.
    pub(crate) window: WindowBackgroundAppearance,
    pub(crate) background: Option<Background>,
    /// Added to every hover state the chrome draws, on top of its fill.
    pub(crate) hover_glow: Option<BoxShadow>,
}

/// An image under (or over) the whole editor window.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Background {
    pub(crate) path: PathBuf,
    pub(crate) opacity: f32,
    pub(crate) fit: Fit,
    /// `true` paints it over the chrome (it never takes a click); `false`,
    /// the default, under it, where it shows through translucent surfaces.
    pub(crate) over: bool,
}

/// How a background image fills the window — CSS's `object-fit`, minus
/// the values that leave part of the window uncovered by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fit {
    Cover,
    Contain,
    Fill,
}

impl From<Fit> for ObjectFit {
    fn from(fit: Fit) -> Self {
        match fit {
            Fit::Cover => ObjectFit::Cover,
            Fit::Contain => ObjectFit::Contain,
            Fit::Fill => ObjectFit::Fill,
        }
    }
}

static ACTIVE: LazyLock<RwLock<Arc<Palette>>> =
    LazyLock::new(|| RwLock::new(Arc::new(Palette::builtin().clone())));

/// The active palette. Cheap: an `Arc` clone under an uncontended read lock.
pub(crate) fn active() -> Arc<Palette> {
    ACTIVE
        .read()
        .map(|palette| palette.clone())
        .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
}

fn set_active(palette: Palette) {
    let palette = Arc::new(palette);
    match ACTIVE.write() {
        Ok(mut active) => *active = palette,
        Err(poisoned) => *poisoned.into_inner() = palette,
    }
}

impl Palette {
    /// One colour token of this palette, which need not be the active one.
    pub(crate) fn color(&self, name: &str) -> Rgba {
        self.colors.get(name).copied().unwrap_or_default()
    }
}

/// A colour token by name. Every name `tokens` asks for is in Default's
/// `theme.json` (its tests check both directions), and every palette is
/// built over Default, so a miss is a bug in this crate, not in a theme.
pub(crate) fn color(name: &str) -> Rgba {
    active()
        .colors
        .get(name)
        .copied()
        .unwrap_or_else(|| panic!("colour token {name:?} is missing from the default theme"))
}

/// A size token's unscaled value, in pixels — see [`color`].
pub(crate) fn size(name: &str) -> f32 {
    active()
        .sizes
        .get(name)
        .copied()
        .unwrap_or_else(|| panic!("size token {name:?} is missing from the default theme"))
}

/// The theme's hover glow, when it has one.
pub(crate) fn hover_glow() -> Option<BoxShadow> {
    active().effects.hover_glow.clone()
}

#[cfg(test)]
#[path = "theme/tests.rs"]
mod tests;
