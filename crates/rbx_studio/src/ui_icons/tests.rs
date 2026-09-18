use resvg::usvg::{Options, Tree};

use super::*;

/// Every name the editor actually asks [`icon`] for. A name added to a call
/// site without a file behind it renders as an invisible gap, which is
/// exactly the kind of thing a screenshot review misses — so it fails here
/// instead.
const NAMES: &[&str] = &[
    // Tools (shell::toolbar)
    "select",
    "align",
    "move",
    "scale",
    "rotate",
    // Clipboard
    "copy",
    "paste",
    "cut",
    "duplicate",
    // Insert triggers and their menu items
    "part",
    "block",
    "sphere",
    "wedge",
    "corner-wedge",
    "cylinder",
    "script",
    "local-script",
    "module-script",
    "gui",
    "screen-gui",
    "surface-gui",
    "ad-gui",
    "billboard-gui",
    "toolbox",
    // File / Edit
    "import",
    "material",
    "color",
    "group",
    "ungroup",
    "lock",
    "anchor",
    // Test
    "play",
    "run",
    "resume",
    "stop",
    "team",
    "exit",
    // Viewport settings
    "game-settings",
    "device",
    "show-ui",
    // Chrome
    "check",
    "more",
    "chevron-down",
    "chevron-right",
    "chevron-up",
];

#[test]
fn every_icon_the_editor_names_is_embedded() {
    for name in NAMES {
        assert!(
            UiIcons::get(&format!("{name}.svg")).is_some(),
            "no UI icon file for {name:?}"
        );
    }
}

/// A malformed path or a stray attribute makes GPUI draw nothing at all,
/// silently. Parsing every file here turns that into a test failure.
#[test]
fn every_embedded_icon_parses_as_svg() {
    for file in UiIcons::iter() {
        let data = UiIcons::get(&file).expect("the file just listed").data;
        assert!(
            Tree::from_data(&data, &Options::default()).is_ok(),
            "{file} is not parseable SVG"
        );
    }
}

/// The whole point of this kit is that GPUI re-tints it per state. An icon
/// that hardcodes a colour would keep it through hover, disabled and
/// selected alike — and a `fill` that isn't `none` would paint a silhouette
/// where line art was intended.
#[test]
fn every_embedded_icon_is_tintable_line_art() {
    for file in UiIcons::iter() {
        let data = UiIcons::get(&file).expect("the file just listed").data;
        let source = String::from_utf8(data.to_vec()).expect("icons are UTF-8 text");
        assert!(
            source.contains(r#"stroke="currentColor""#),
            "{file} does not stroke in currentColor, so it can't be tinted per state"
        );
        assert!(
            source.contains(r#"fill="none""#),
            "{file} is not fill=\"none\" line art"
        );
        assert!(
            !source.contains("#"),
            "{file} hardcodes a colour, which would survive every state change"
        );
    }
}
