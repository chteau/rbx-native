//! Making a loaded [`ThemePack`] the one on screen: the palette `tokens`
//! reads, the toolkit's widget theme, the design fonts, and every open
//! window's background mode.

use std::rc::Rc;

use gpui_kit::component::highlighter::HighlightTheme;
use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::App;

use super::overrides::{recolour, Overrides};
use super::ThemePack;
use crate::tokens;

/// Before the first window opens. `main` registers the bundled fonts first,
/// so [`design_fonts`] can find them.
pub(crate) fn startup(pack: &ThemePack, overrides: &Overrides, cx: &mut App) {
    super::set_active(pack.palette.overridden(overrides));
    widgets(pack, overrides, cx);
}

/// A switch while the editor is running. The caller re-resolves anything it
/// cached from the old theme (the Explorer's icons) and re-renders.
pub(crate) fn apply(pack: &ThemePack, overrides: &Overrides, cx: &mut App) {
    super::set_active(pack.palette.overridden(overrides));
    widgets(pack, overrides, cx);
    let appearance = pack.palette.effects.window;
    for handle in cx.windows() {
        let _ = handle.update(cx, |_, window, _| {
            window.set_background_appearance(appearance);
        });
    }
    cx.refresh_windows();
}

/// In the order startup always ran these: the fonts are named before
/// `Theme::change`, whose own mono-font fallback only steps in while the
/// family is still the platform default.
fn widgets(pack: &ThemePack, overrides: &Overrides, cx: &mut App) {
    let config = match overrides.accent {
        Some(to) => recolour(&pack.widgets, pack.palette.colors["check_on"], to),
        None => pack.widgets.clone(),
    };
    Theme::global_mut(cx).dark_theme = Rc::new(config);
    design_fonts(cx);
    Theme::change(ThemeMode::Dark, None, cx);
    // GPUI Kit only swaps its syntax palette for the one a theme file's
    // `highlight` block defines; a theme without one keeps the kit's *light*
    // palette whatever its mode, which put navy keywords on the editor's
    // near black.
    if Theme::global(cx).dark_theme.highlight.is_none() {
        Theme::global_mut(cx).highlight_theme = HighlightTheme::default_dark();
    }
}

/// Points the toolkit at the design system's families, for whichever of
/// them the text system has.
///
/// The theme takes one family name, not a CSS-style stack with fallbacks,
/// and a name that isn't installed is used as-is rather than falling
/// through — so the fallback has to happen here, by asking the text system
/// what exists before naming anything.
fn design_fonts(cx: &mut App) {
    let installed = cx.text_system().all_font_names();
    let has = |family: &str| installed.iter().any(|name| name == family);
    let (ui, mono) = (has(tokens::FONT_FAMILY_UI), has(tokens::FONT_FAMILY_MONO));
    let theme = Theme::global_mut(cx);
    if ui {
        theme.font_family = tokens::FONT_FAMILY_UI.into();
    }
    if mono {
        theme.mono_font_family = tokens::FONT_FAMILY_MONO.into();
    }
}
