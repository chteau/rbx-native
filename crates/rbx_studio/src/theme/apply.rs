//! Making a loaded [`ThemePack`] the one on screen: the palette `tokens`
//! reads, the toolkit's widget theme, the design fonts, and every open
//! window's background mode.

use std::borrow::Cow;
use std::collections::HashSet;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Mutex;
use std::time::SystemTime;

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
    let mode = pack.palette.mode;
    let theme = Theme::global_mut(cx);
    match mode {
        ThemeMode::Dark => theme.dark_theme = Rc::new(config),
        ThemeMode::Light => theme.light_theme = Rc::new(config),
    }
    design_fonts(pack, cx);
    Theme::change(mode, None, cx);
    // GPUI Kit only swaps its syntax palette for the one a theme file's
    // `highlight` block defines; a theme without one keeps whichever
    // palette was there before, whatever its mode — the kit's *light* one
    // at startup, which put navy keywords on the editor's near black.
    let theme = Theme::global(cx);
    let own = match mode {
        ThemeMode::Dark => &theme.dark_theme,
        ThemeMode::Light => &theme.light_theme,
    };
    if own.highlight.is_none() {
        Theme::global_mut(cx).highlight_theme = match mode {
            ThemeMode::Dark => HighlightTheme::default_dark(),
            ThemeMode::Light => HighlightTheme::default_light(),
        };
    }
}

/// Points the toolkit at the design system's families, for whichever of
/// them the text system has.
///
/// The theme takes one family name, not a CSS-style stack with fallbacks,
/// and a name that isn't installed is used as-is rather than falling
/// through — so the fallback has to happen here, by asking the text system
/// what exists before naming anything.
fn design_fonts(pack: &ThemePack, cx: &mut App) {
    let wanted = &pack.palette.effects.fonts;
    register(&wanted.files, cx);
    let installed = cx.text_system().all_font_names();
    let has = |family: &str| installed.iter().any(|name| name == family);
    let pick = |theme_family: &Option<String>, default: &str| -> Option<String> {
        theme_family
            .as_deref()
            .filter(|family| has(family))
            .or(has(default).then_some(default))
            .map(str::to_string)
    };
    let ui = pick(&wanted.ui, tokens::FONT_FAMILY_UI);
    let mono = pick(&wanted.mono, tokens::FONT_FAMILY_MONO);
    let theme = Theme::global_mut(cx);
    if let Some(ui) = ui {
        theme.font_family = ui.into();
    }
    if let Some(mono) = mono {
        theme.mono_font_family = mono.into();
    }
    let (ui, mono) = (theme.font_family.clone(), theme.mono_font_family.clone());
    tokens::set_fonts(ui, mono);
}

/// Adds font files the text system has not seen, keyed by path, size and
/// mtime so an edited file is picked up and an unchanged one is not added
/// twice on every theme switch.
fn register(files: &[PathBuf], cx: &mut App) {
    static SEEN: Mutex<Option<HashSet<(PathBuf, u64, SystemTime)>>> = Mutex::new(None);
    let mut seen = SEEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let seen = seen.get_or_insert_with(HashSet::new);
    let mut fresh = Vec::new();
    for path in files {
        let key = std::fs::metadata(path)
            .and_then(|meta| Ok((meta.len(), meta.modified()?)))
            .map(|(len, mtime)| (path.clone(), len, mtime));
        let key = match key {
            Ok(key) => key,
            Err(err) => {
                eprintln!("theme font {}: {err}", path.display());
                continue;
            }
        };
        if seen.contains(&key) {
            continue;
        }
        match std::fs::read(path) {
            Ok(bytes) => {
                seen.insert(key);
                fresh.push(Cow::Owned(bytes));
            }
            Err(err) => eprintln!("theme font {}: {err}", path.display()),
        }
    }
    if fresh.is_empty() {
        return;
    }
    if let Err(err) = cx.text_system().add_fonts(fresh) {
        eprintln!("theme fonts: {err}");
    }
}
