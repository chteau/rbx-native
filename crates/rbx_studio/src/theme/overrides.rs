//! What the user's appearance lays over whichever theme is active: an
//! accent of their own, and per-tool colours. The user's accent wins over a
//! theme's, so it is applied to the resolved palette and widget theme after
//! the theme is loaded, wherever the theme used its own accent.

use gpui_kit::component::ThemeConfig;
use gpui_kit::Rgba;

use super::Palette;
use crate::accent;

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Overrides {
    pub(crate) accent: Option<Rgba>,
    /// `(token, colour)`: `tool_select`, `tool_move`, …
    pub(crate) tools: Vec<(String, Rgba)>,
}

/// Two colours with the same RGB, whatever their alpha.
fn same_rgb(a: Rgba, b: Rgba) -> bool {
    let byte = |c: f32| (c * 255.).round() as i32;
    byte(a.r) == byte(b.r) && byte(a.g) == byte(b.g) && byte(a.b) == byte(b.b)
}

impl Palette {
    pub(crate) fn overridden(&self, overrides: &Overrides) -> Palette {
        let mut palette = self.clone();
        if let Some(to) = overrides.accent {
            let from = self.colors["check_on"];
            for color in palette.colors.values_mut() {
                if same_rgb(*color, from) {
                    *color = Rgba { a: color.a, ..to };
                }
            }
            // Hover is the accent a twentieth of the way to white, as the
            // default theme's pair is.
            let lift = |c: f32| c + (1. - c) * 0.05;
            palette.colors.insert(
                "accent_hover".to_owned(),
                Rgba {
                    r: lift(to.r),
                    g: lift(to.g),
                    b: lift(to.b),
                    a: 1.,
                },
            );
            let max = self.colors["selection"].a;
            palette.colors.insert(
                "selection".to_owned(),
                Rgba {
                    a: accent::selection_alpha(to, max),
                    ..to
                },
            );
        }
        for (token, color) in &overrides.tools {
            if palette.colors.contains_key(token) {
                palette.colors.insert(token.clone(), *color);
            }
        }
        palette
    }
}

/// `config` with every colour the theme drew in its own accent `from`
/// redrawn in `to`, keeping each one's alpha. The toolkit's theme is
/// walked as the JSON it was read from, so a colour field added to it later
/// is covered without naming it here.
pub(crate) fn recolour(config: &ThemeConfig, from: Rgba, to: Rgba) -> ThemeConfig {
    fn walk(value: &mut serde_json::Value, from: Rgba, to: Rgba) {
        match value {
            serde_json::Value::String(text) => {
                let digits = text.trim_start_matches('#');
                if !text.starts_with('#') || !(digits.len() == 6 || digits.len() == 8) {
                    return;
                }
                let Some(color) = accent::parse_hex(&digits[..6]) else {
                    return;
                };
                if same_rgb(color, from) {
                    *text = format!("{}{}", accent::hex(to), &digits[6..]);
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(|v| walk(v, from, to)),
            serde_json::Value::Object(map) => map.values_mut().for_each(|v| walk(v, from, to)),
            _ => {}
        }
    }
    let Ok(mut value) = serde_json::to_value(config) else {
        return config.clone();
    };
    walk(&mut value, from, to);
    serde_json::from_value(value).unwrap_or_else(|_| config.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemePack;

    #[test]
    fn an_accent_recolours_every_accent_token_and_nothing_else() {
        let pack = ThemePack::builtin();
        let azure = accent::rgb(0x4C9BE8);
        let palette = pack.palette.overridden(&Overrides {
            accent: Some(azure),
            ..Overrides::default()
        });
        let hex = |name: &str| accent::hex(palette.colors[name]);
        assert_eq!(hex("check_on"), "#4C9BE8");
        assert_eq!(hex("tab_active_bar"), "#4C9BE8");
        assert_eq!(hex("accent_soft"), "#4C9BE8");
        assert!((palette.colors["accent_soft"].a - 0.12).abs() < 1e-3);
        assert!((palette.colors["accent_line"].a - 0.55).abs() < 1e-3);
        assert_eq!(hex("selection"), "#4C9BE8");
        assert!(palette.colors["selection"].a < pack.palette.colors["selection"].a);
        assert_eq!(
            hex("text_error"),
            accent::hex(pack.palette.colors["text_error"])
        );
        assert_eq!(hex("dock"), accent::hex(pack.palette.colors["dock"]));
    }

    #[test]
    fn no_overrides_is_the_theme_itself() {
        let pack = ThemePack::builtin();
        assert_eq!(pack.palette.overridden(&Overrides::default()), pack.palette);
    }

    #[test]
    fn the_widget_theme_follows_the_accent_keeping_alpha() {
        let pack = ThemePack::builtin();
        let from = pack.palette.colors["check_on"];
        let recoloured = recolour(&pack.widgets, from, accent::rgb(0x3FB3C9));
        let json = serde_json::to_string(&recoloured).unwrap().to_uppercase();
        assert!(
            !json.contains("#6C7FDB"),
            "an accent colour was left behind"
        );
        assert!(json.contains("\"#3FB3C9\""));
        assert!(
            json.contains("#3FB3C91F"),
            "list.active.background keeps its alpha"
        );
    }

    #[test]
    fn a_tool_colour_replaces_only_its_own_token() {
        let pack = ThemePack::builtin();
        let palette = pack.palette.overridden(&Overrides {
            tools: vec![("tool_move".to_owned(), accent::rgb(0x112233))],
            ..Overrides::default()
        });
        assert_eq!(accent::hex(palette.colors["tool_move"]), "#112233");
        assert_eq!(
            palette.colors["tool_scale"],
            pack.palette.colors["tool_scale"]
        );
    }
}
