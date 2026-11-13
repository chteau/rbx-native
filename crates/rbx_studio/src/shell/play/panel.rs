//! The in-game dev panel (`rbx_play`'s `probe/panel.luau`): on macOS, where
//! Roblox runs in its own window instead of the 3D view, Play puts the
//! editor's Output, stats and debugger buttons inside the game. It is drawn
//! with the editor's own theme colours, handed over as JSON.

use gpui_kit::Rgba;

use crate::launcher::ui;
use crate::tokens;

/// Whether this OS gets the panel: only where the game can't be embedded.
pub(super) const ENABLED: bool = cfg!(target_os = "macos");

/// `#rrggbbaa`.
fn hex(color: Rgba) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02x}{:02x}{:02x}{:02x}",
        byte(color.r),
        byte(color.g),
        byte(color.b),
        byte(color.a)
    )
}

/// The panel's colours, by the names `panel.luau` asks for, from the theme
/// that is on right now.
pub(super) fn theme() -> String {
    let colors = [
        ("dock", tokens::dock()),
        ("chrome", tokens::chrome()),
        ("field", tokens::field_select()),
        ("border", tokens::border()),
        ("border2", tokens::border2()),
        ("hover", tokens::hover()),
        ("text", tokens::text()),
        ("text2", tokens::text2()),
        ("text3", tokens::text3()),
        ("accent", ui::accent()),
        ("accent_soft", tokens::accent_soft()),
        ("warning", tokens::warning()),
        ("error", tokens::text_error()),
        ("info", tokens::output_info()),
        ("client", tokens::output_client()),
        ("server", tokens::output_server()),
    ];
    let map: serde_json::Map<String, serde_json::Value> = colors
        .into_iter()
        .map(|(name, color)| (name.to_owned(), hex(color).into()))
        .collect();
    serde_json::Value::Object(map).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_are_eight_digit_hex() {
        assert_eq!(
            hex(Rgba {
                r: 1.0,
                g: 0.0,
                b: 0.5,
                a: 1.0
            }),
            "#ff0080ff"
        );
    }

    #[test]
    fn the_theme_names_every_colour_the_panel_reads() {
        let theme: serde_json::Value = serde_json::from_str(&theme()).unwrap();
        let source = rbx_play::inject::PROBE_PANEL;
        for name in [
            "dock",
            "chrome",
            "field",
            "border",
            "border2",
            "hover",
            "text",
            "text2",
            "text3",
            "accent",
            "accent_soft",
            "warning",
            "error",
            "info",
            "client",
            "server",
        ] {
            assert!(theme.get(name).is_some(), "{name} missing from the theme");
            assert!(
                source.contains(&format!("\"{name}\"")),
                "panel never reads {name}"
            );
        }
    }
}
