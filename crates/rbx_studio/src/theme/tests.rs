use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use gpui_kit::component::ThemeMode;
use gpui_kit::{rgb, rgba, Rgba, WindowBackgroundAppearance};

use super::pack::{self, inside, Manifest, ThemePack};
use super::palette::{self, build};
use super::{Palette, DEFAULT_ID, HIGH_CONTRAST_ID, LIGHT_ID};
use crate::accent::contrast;

fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rbx-native-theme-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, relative: &str, bytes: &[u8]) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

const MANIFEST: &str = r#"{"name":"Dusk","author":"@someone","description":"Warm.","version":"1.0.0","preview":"preview.png"}"#;

/// A valid theme folder `themes/<id>` with the given `theme.json`.
fn theme(themes: &Path, id: &str, theme_json: Option<&str>) -> PathBuf {
    let dir = themes.join(id);
    write(&dir, "manifest.json", MANIFEST.as_bytes());
    write(&dir, "preview.png", b"png");
    if let Some(json) = theme_json {
        write(&dir, "theme.json", json.as_bytes());
    }
    dir
}

fn palette_of(json: &str, dir: Option<&Path>) -> Result<(Palette, Vec<String>), String> {
    build(Some(&palette::parse(json)?), dir)
}

// --------------------------------------------------------------- Default

/// `tokens` asks for tokens by name; a name Default lacks would panic on
/// first draw, and a name nothing asks for is a dead template entry that
/// would mislead theme authors. Both directions, straight from the source.
#[test]
fn tokens_and_the_default_theme_name_exactly_the_same_tokens() {
    let source = include_str!("../tokens.rs");
    let asked = |call: &str| -> BTreeSet<String> {
        source
            .match_indices(call)
            .map(|(at, _)| {
                let rest = &source[at + call.len()..];
                rest[..rest.find('"').unwrap()].to_owned()
            })
            .collect()
    };
    let (colors, sizes) = palette::default_names();
    assert_eq!(
        asked("theme::color(\""),
        colors.into_iter().collect::<BTreeSet<_>>()
    );
    assert_eq!(
        asked("theme::size(\""),
        sizes.into_iter().collect::<BTreeSet<_>>()
    );
}

/// The values `tokens.rs` held as literals before they moved into
/// `assets/themes/default/theme.json`, including the derived ones: the
/// Default theme has to be the editor's look to the bit, not approximately.
#[test]
fn the_default_palette_is_bit_identical_to_the_old_literals() {
    let palette = Palette::builtin();
    let color = |name: &str| palette.colors[name];
    assert_eq!(color("black"), rgb(0x0A0A0B));
    assert_eq!(color("dock"), rgb(0x121213));
    assert_eq!(color("tile"), rgb(0x121213));
    assert_eq!(color("chrome"), rgb(0x121213));
    assert_eq!(color("menu_bar"), rgb(0x0A0A0B));
    assert_eq!(color("selection"), rgba(0x6C7FDBB8));
    assert_eq!(color("hover"), rgba(0xFFFFFF0D));
    assert_eq!(color("tab_active_bar"), rgb(0x6C7FDB));
    assert_eq!(
        color("accent_soft"),
        Rgba {
            a: 0.12,
            ..rgb(0x6C7FDB)
        }
    );
    assert_eq!(
        color("accent_line"),
        Rgba {
            a: 0.55,
            ..rgb(0x6C7FDB)
        }
    );
    assert_eq!(
        color("error_soft"),
        Rgba {
            a: 0.12,
            ..rgb(0xE06C6C)
        }
    );
    assert_eq!(color("text_disabled"), rgb(0x5C5C63));
    assert_eq!(color("shadow"), rgba(0x00000066));
    assert_eq!(color("wash_faint"), rgba(0xFFFFFF0A));
    assert_eq!(color("veil"), rgba(0x0000008C));
    assert_eq!(color("avatar"), rgb(0x2C3050));
    assert_eq!(color("avatar_on_accent"), rgb(0x2A2D4A));
    assert_eq!(color("slide_backdrop"), rgb(0x131314));
    assert_eq!(color("secondary_hover"), rgb(0x202123));
    assert_eq!(palette.sizes["select_inset"], 5.5);
    assert_eq!(palette.sizes["text_sm"], 11.5);
    assert_eq!(palette.effects, super::Effects::default());
}

#[test]
fn the_default_theme_is_complete_and_ships_its_preview() {
    let pack = ThemePack::builtin();
    assert_eq!(pack.id, DEFAULT_ID);
    assert_eq!(pack.manifest.author, "@chteau");
    assert!(pack.icons.is_none(), "Default draws the built-in icon kit");
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/themes/default");
    Manifest::parse(
        &fs::read_to_string(assets.join("manifest.json")).unwrap(),
        Some(&assets),
    )
    .expect("the preview image exists beside the manifest");
    let png = fs::read(assets.join(&pack.manifest.preview)).unwrap();
    assert_eq!(&png[1..4], b"PNG");
}

// ------------------------------------------------------------ layering

#[test]
fn a_theme_overrides_only_what_it_names_and_references_follow_it() {
    let (palette, warnings) = palette_of(
        r##"{"colors":{"dock":"#201810"},"sizes":{"text_md":14}}"##,
        None,
    )
    .unwrap();
    assert!(warnings.is_empty());
    assert_eq!(palette.colors["dock"], rgb(0x201810));
    // Default defines both as `@dock`.
    assert_eq!(palette.colors["tile"], rgb(0x201810));
    assert_eq!(palette.colors["chrome"], rgb(0x201810));
    assert_eq!(palette.colors["black"], rgb(0x0A0A0B));
    assert_eq!(palette.sizes["text_md"], 14.);
    assert_eq!(palette.sizes["text_sm"], 11.5);
}

#[test]
fn references_can_change_alpha_and_chain() {
    let (palette, _) = palette_of(
        r##"{"colors":{"check_on":"#FF0000","selection":"@accent_line/0.25","knob":"@tab_active_bar"}}"##,
        None,
    )
    .unwrap();
    assert_eq!(
        palette.colors["selection"],
        Rgba {
            a: 0.25,
            ..rgb(0xFF0000)
        }
    );
    assert_eq!(palette.colors["knob"], rgb(0xFF0000));
}

#[test]
fn unknown_tokens_warn_but_bad_values_refuse_the_theme() {
    let (_, warnings) = palette_of(
        r##"{"colors":{"sparkle":"#FFFFFF"},"sizes":{"huge":1}}"##,
        None,
    )
    .unwrap();
    assert_eq!(warnings.len(), 2);

    for bad in [
        r#"{"colors":{"dock":"orange"}}"#,
        r##"{"colors":{"dock":"#12345"}}"##,
        r##"{"colors":{"dock":"#GGGGGG"}}"##,
        r#"{"colors":{"dock":"@nothing"}}"#,
        r#"{"colors":{"dock":"@tile"}}"#,
        r#"{"colors":{"dock":"@black/1.5"}}"#,
        r#"{"sizes":{"text_md":-1}}"#,
        r#"{"sizes":{"text_md":99999}}"#,
        r#"{"window":"glass"}"#,
        r#"{"hover":{"glow":"@nothing"}}"#,
        r#"{"colors":[1,2]}"#,
    ] {
        assert!(palette_of(bad, None).is_err(), "{bad}");
    }
}

// -------------------------------------------------------------- effects

#[test]
fn window_modes_and_the_hover_glow_parse() {
    let (palette, _) = palette_of(
        r#"{"window":"blurred","hover":{"glow":"@check_on/0.5","glow_radius":6}}"#,
        None,
    )
    .unwrap();
    assert_eq!(palette.effects.window, WindowBackgroundAppearance::Blurred);
    let glow = palette.effects.hover_glow.unwrap();
    assert_eq!(f32::from(glow.blur_radius), 6.);
    assert_eq!(
        glow.color,
        Rgba {
            a: 0.5,
            ..rgb(0x6C7FDB)
        }
        .into()
    );
    let (palette, _) = palette_of(r#"{"window":"transparent"}"#, None).unwrap();
    assert_eq!(
        palette.effects.window,
        WindowBackgroundAppearance::Transparent
    );
}

#[test]
fn a_background_image_must_be_an_image_inside_the_theme() {
    let dir = scratch();
    write(&dir, "art/bg.png", b"png");
    write(&dir, "notes.txt", b"x");
    let (palette, _) = palette_of(
        r#"{"background":{"image":"art/bg.png","opacity":0.3,"fit":"contain","layer":"over"}}"#,
        Some(&dir),
    )
    .unwrap();
    let background = palette.effects.background.unwrap();
    assert_eq!(background.path, dir.join("art/bg.png"));
    assert_eq!(background.opacity, 0.3);
    assert_eq!(background.fit, super::Fit::Contain);
    assert!(background.over);

    for bad in [
        r#"{"background":{"image":"missing.png"}}"#,
        r#"{"background":{"image":"notes.txt"}}"#,
        r#"{"background":{"image":"../bg.png"}}"#,
        r#"{"background":{"image":"/etc/bg.png"}}"#,
        r#"{"background":{"image":"art/bg.png","opacity":2}}"#,
        r#"{"background":{"image":"art/bg.png","fit":"tile"}}"#,
        r#"{"background":{"image":"art/bg.png","layer":"sideways"}}"#,
    ] {
        assert!(palette_of(bad, Some(&dir)).is_err(), "{bad}");
    }
}

// ---------------------------------------------------------------- packs

#[test]
fn a_manifest_needs_every_field_and_a_preview_inside_the_theme() {
    let dir = scratch();
    write(&dir, "preview.png", b"png");
    assert!(Manifest::parse(MANIFEST, Some(&dir)).is_ok());

    for bad in [
        r#"{"name":"Dusk"}"#,
        r#"{"name":" ","author":"a","description":"d","version":"1","preview":"preview.png"}"#,
        r#"{"name":"Dusk","author":"a","description":"d","version":"1","preview":"none.png"}"#,
        r#"{"name":"Dusk","author":"a","description":"d","version":"1","preview":"../preview.png"}"#,
    ] {
        assert!(Manifest::parse(bad, Some(&dir)).is_err(), "{bad}");
    }
    let long = MANIFEST.replace("Dusk", &"x".repeat(65));
    assert!(Manifest::parse(&long, Some(&dir)).is_err());
}

#[test]
fn an_installed_theme_loads_its_palette_widgets_and_icons() {
    let themes = scratch();
    let dir = theme(&themes, "dusk", Some(r##"{"colors":{"dock":"#201810"}}"##));
    write(&dir, "icons/Part.svg", b"<svg/>");
    write(
        &dir,
        "widgets.json",
        br#"{"name":"d","author":"a","themes":[{"name":"Dawn","mode":"light"},{"name":"Dusk Dark","mode":"dark"}]}"#,
    );

    let pack = ThemePack::load_from(&themes, "dusk").unwrap();
    assert_eq!(pack.manifest.name, "Dusk");
    assert_eq!(pack.palette.colors["tile"], rgb(0x201810));
    assert_eq!(pack.widgets.name.as_ref(), "Dusk Dark");
    assert!(pack.icons.unwrap().svg("Part", None).is_some());
}

#[test]
fn a_theme_without_icons_or_widgets_falls_back_to_defaults() {
    let themes = scratch();
    theme(&themes, "bare", None);
    let pack = ThemePack::load_from(&themes, "bare").unwrap();
    let builtin = ThemePack::builtin();
    assert!(pack.icons.is_none());
    assert_eq!(pack.widgets.name, builtin.widgets.name);
    assert_eq!(pack.palette, builtin.palette);
}

#[test]
fn a_theme_without_a_manifest_does_not_load() {
    let themes = scratch();
    write(&themes, "nomanifest/theme.json", b"{}");
    assert!(ThemePack::load_from(&themes, "nomanifest")
        .unwrap_err()
        .contains("manifest.json"));
    assert!(ThemePack::load_from(&themes, "absent").is_err());
    assert!(ThemePack::load_from(&themes, "../escape").is_err());
}

/// A widgets-only `themes/<name>.json`, the format before theme folders.
#[test]
fn a_legacy_theme_file_still_loads_as_widgets_only() {
    let themes = scratch();
    write(
        &themes,
        "Old.json",
        br#"{"name":"o","author":"a","themes":[{"name":"Old Dark","mode":"dark"}]}"#,
    );
    let pack = ThemePack::load_from(&themes, "Old").unwrap();
    assert_eq!(pack.widgets.name.as_ref(), "Old Dark");
    assert_eq!(pack.palette, *Palette::builtin());
}

#[test]
fn default_cannot_be_shadowed_or_uninstalled() {
    let themes = scratch();
    let dir = theme(
        &themes,
        DEFAULT_ID,
        Some(r##"{"colors":{"dock":"#FF0000"}}"##),
    );
    assert_eq!(
        ThemePack::load_from(&themes, DEFAULT_ID).unwrap().palette,
        *Palette::builtin()
    );
    assert!(pack::uninstall(&themes, DEFAULT_ID).is_err());
    assert!(dir.exists());
}

#[test]
fn installed_lists_the_built_in_themes_first_then_valid_themes_by_name() {
    let themes = scratch();
    theme(&themes, "zeta", None);
    let alpha = theme(&themes, "alpha", None);
    fs::write(
        alpha.join("manifest.json"),
        MANIFEST.replace("Dusk", "Aardvark"),
    )
    .unwrap();
    write(&themes, "broken/manifest.json", b"{}");
    write(&themes, ".hidden/manifest.json", MANIFEST.as_bytes());

    let ids: Vec<String> = pack::installed(&themes)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(
        ids,
        [DEFAULT_ID, HIGH_CONTRAST_ID, LIGHT_ID, "alpha", "zeta"]
    );
}

#[test]
fn uninstall_removes_only_the_named_theme() {
    let themes = scratch();
    theme(&themes, "a", None);
    theme(&themes, "b", None);
    pack::uninstall(&themes, "a").unwrap();
    assert!(!themes.join("a").exists());
    assert!(themes.join("b").exists());
    assert!(pack::uninstall(&themes, "a").is_err());
    assert!(pack::uninstall(&themes, "../b").is_err());
}

#[test]
fn inside_refuses_every_way_out_of_the_folder() {
    let dir = scratch();
    write(&dir, "a/b.png", b"x");
    assert_eq!(inside(&dir, "a/b.png"), Some(dir.join("a/b.png")));
    for bad in ["", "..", "../x", "a/../../x", "/etc/passwd", "./a/b.png"] {
        assert_eq!(inside(&dir, bad), None, "{bad:?}");
    }
    #[cfg(unix)]
    {
        let outside = scratch();
        write(&outside, "secret.png", b"x");
        std::os::unix::fs::symlink(outside.join("secret.png"), dir.join("link.png")).unwrap();
        assert_eq!(inside(&dir, "link.png"), None);
    }
}

// ------------------------------------------------------------- built-ins

/// `foreground` (with its own alpha) over an opaque `background`.
fn composite(foreground: Rgba, background: Rgba) -> Rgba {
    let mix = |f: f32, b: f32| f * foreground.a + b * (1. - foreground.a);
    Rgba {
        r: mix(foreground.r, background.r),
        g: mix(foreground.g, background.g),
        b: mix(foreground.b, background.b),
        a: 1.,
    }
}

/// What lights a surface under a label: a hover, an active tab, and (for
/// text, but not for the accent and status colours, which are never drawn
/// on it) a selection.
const WASHES: [&str; 4] = ["hover", "hover_subtle", "ribbon_tab_active", "selection"];
const UNSELECTED: &[&str] = &["hover", "hover_subtle", "ribbon_tab_active"];

/// Each of `names` at `min` or better on every surface a label can sit
/// on, plain and lit by each of `washes`.
fn assert_legible(pack: &ThemePack, names: &[&str], min: f32, washes: &[&str]) {
    let color = |name: &str| pack.palette.color(name);
    let mut grounds = Vec::new();
    for surface in [
        "black",
        "dock",
        "chrome",
        "field_select",
        "tile",
        "menu_bar",
    ] {
        let under = color(surface);
        grounds.push((surface.to_owned(), under));
        for wash in washes {
            grounds.push((
                format!("{wash} on {surface}"),
                composite(color(wash), under),
            ));
        }
    }
    for name in names {
        for (ground, under) in &grounds {
            let ratio = contrast(composite(color(name), *under), *under);
            assert!(
                ratio >= min,
                "{}: {name} on {ground} is {ratio:.2}:1, below {min}:1",
                pack.id
            );
        }
    }
}

/// Every label that carries meaning, primary and secondary.
const TEXT: [&str; 6] = [
    "text",
    "text_full",
    "text_strong",
    "text2",
    "text_label",
    "text_muted",
];
/// The colours drawn as text or glyphs besides the text ramp.
const SIGNALS: [&str; 4] = ["check_on", "warning", "text_error", "diff_add"];

/// WCAG 1.4.6 (AAA): 7:1 for body text — secondary labels included, as
/// they are body-sized here — and 4.5:1 for the rest: placeholders, the
/// accent as text, the status colours.
#[test]
fn high_contrast_clears_aaa() {
    let pack = ThemePack::embedded(HIGH_CONTRAST_ID).unwrap();
    assert_legible(&pack, &TEXT, 7., &WASHES);
    let placeholder = ["text3", "text_placeholder", "text_disabled"];
    assert_legible(&pack, &placeholder, 4.5, &WASHES);
    assert_legible(&pack, &SIGNALS, 4.5, UNSELECTED);
    let color = |name: &str| pack.palette.color(name);
    let on_accent = contrast(color("black"), color("check_on"));
    assert!(
        on_accent >= 7.,
        "button text on the accent is {on_accent:.2}:1"
    );
}

/// AA (1.4.3) for every label that carries meaning; the placeholder and
/// disabled tier is exempt, as it is in Default.
#[test]
fn light_clears_aa() {
    let pack = ThemePack::embedded(LIGHT_ID).unwrap();
    assert_legible(&pack, &TEXT, 4.5, &WASHES);
    assert_legible(&pack, &SIGNALS, 4.5, UNSELECTED);
    let color = |name: &str| pack.palette.color(name);
    let on_accent = contrast(color("black"), color("check_on"));
    assert!(
        on_accent >= 4.5,
        "button text on the accent is {on_accent:.2}:1"
    );
}

/// The non-text floor (1.4.11) both keep: the focus ring, an unticked
/// toggle's edge, and each tool's colour on the ribbon tile it lights.
#[test]
fn the_built_in_themes_keep_three_to_one_for_controls() {
    for id in [LIGHT_ID, HIGH_CONTRAST_ID] {
        let pack = ThemePack::embedded(id).unwrap();
        let color = |name: &str| pack.palette.color(name);
        for (name, on) in [
            ("check_on", "dock"),
            ("check_on", "field_select"),
            ("check_off_border", "black"),
            ("check_off_border", "dock"),
            ("tool_select", "tile"),
            ("tool_move", "tile"),
            ("tool_scale", "tile"),
            ("tool_rotate", "tile"),
            ("tool_transform", "tile"),
            ("tool_align", "tile"),
            ("tool_local", "tile"),
            ("tool_sun", "tile"),
            ("tool_pivot", "tile"),
        ] {
            let ratio = contrast(color(name), color(on));
            assert!(ratio >= 3., "{id}: {name} on {on} is {ratio:.2}:1");
        }
    }
}

#[test]
fn the_built_in_themes_load_in_their_own_mode_and_ship_a_preview() {
    for (id, mode) in [
        (DEFAULT_ID, ThemeMode::Dark),
        (HIGH_CONTRAST_ID, ThemeMode::Dark),
        (LIGHT_ID, ThemeMode::Light),
    ] {
        let pack = ThemePack::load(id).unwrap();
        assert_eq!(pack.id, id);
        assert!(pack.warnings.is_empty(), "{id}: {:?}", pack.warnings);
        assert_eq!(pack.palette.mode, mode, "{id}");
        assert_eq!(pack.widgets.mode, mode, "{id}");
        let assets = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/themes")
            .join(id);
        let png = fs::read(assets.join(&pack.manifest.preview)).unwrap();
        assert_eq!(&png[1..4], b"PNG", "{id}");
    }
}

#[test]
fn a_light_theme_without_widgets_gets_light_widgets() {
    let themes = scratch();
    theme(&themes, "dawn", Some(r#"{"mode":"light"}"#));
    let pack = ThemePack::load_from(&themes, "dawn").unwrap();
    assert_eq!(pack.widgets.mode, ThemeMode::Light);
    assert!(palette_of(r#"{"mode":"sepia"}"#, None).is_err());
}

#[test]
fn the_built_in_themes_cannot_be_shadowed_or_uninstalled() {
    let themes = scratch();
    for id in [LIGHT_ID, HIGH_CONTRAST_ID] {
        let dir = theme(&themes, id, Some(r##"{"colors":{"dock":"#FF0000"}}"##));
        assert_eq!(
            ThemePack::load_from(&themes, id).unwrap().palette,
            ThemePack::embedded(id).unwrap().palette
        );
        assert!(pack::uninstall(&themes, id).is_err());
        assert!(dir.exists());
    }
}

/// Like Default's (see `tokens`' own test), each built-in theme's
/// `widgets.json` is a second copy of its palette, checked key by key so
/// the toolkit's widgets never drift from the chrome around them.
#[test]
fn the_built_in_widgets_paint_their_theme_s_palette() {
    let hex = |color: Rgba| {
        let byte = |c: f32| (c * 255.).round() as u8;
        let rgb = format!(
            "#{:02X}{:02X}{:02X}",
            byte(color.r),
            byte(color.g),
            byte(color.b)
        );
        if color.a >= 1. {
            rgb
        } else {
            format!("{rgb}{:02X}", byte(color.a))
        }
    };
    for id in [LIGHT_ID, HIGH_CONTRAST_ID] {
        let pack = ThemePack::embedded(id).unwrap();
        let widgets = serde_json::to_value(&pack.widgets).unwrap();
        for (key, token) in [
            ("background", "black"),
            ("foreground", "text"),
            ("popover.background", "field_select"),
            ("border", "border"),
            ("muted.background", "field_select"),
            ("muted.foreground", "text3"),
            ("sidebar.background", "dock"),
            ("secondary.foreground", "text2"),
            ("primary.background", "check_on"),
            ("ring", "check_on"),
            ("selection.background", "selection"),
            ("accent.background", "hover"),
            ("tab_bar.background", "black"),
            ("title_bar.background", "black"),
            ("danger.background", "text_error"),
        ] {
            let listed = widgets["colors"][key]
                .as_str()
                .unwrap_or_else(|| panic!("{id}: widgets.json has no {key}"));
            assert_eq!(
                listed.to_ascii_uppercase(),
                hex(pack.palette.color(token)),
                "{id}: widgets.json's {key} has drifted from {token}"
            );
        }
    }
}

/// High contrast's Script Editor: the code area (and the gutter, which
/// follows it) paints the theme's black, and every syntax colour, comments
/// included, holds 7:1 (WCAG 1.4.6) on it, plain and under the selection
/// wash. Without its own `highlight` block the theme took GPUI Kit's
/// default dark one, whose editor background never parses (see
/// `editor_background` in `widgets.json`), so the code area fell back to
/// the kit's mid-grey input fill.
#[test]
fn high_contrast_code_clears_aaa() {
    let pack = ThemePack::embedded(HIGH_CONTRAST_ID).unwrap();
    let highlight = pack.widgets.highlight.as_ref().expect("a highlight block");
    let color = |name: &str| pack.palette.color(name);
    let background = Rgba::from(highlight.editor_background.expect("an editor background"));
    assert_eq!(
        background,
        color("black"),
        "the code area is the theme's black"
    );
    assert_eq!(
        highlight.editor_gutter_background, None,
        "the gutter follows"
    );
    // The gutter's other line numbers, in the widgets' muted foreground; a
    // selection never reaches the gutter.
    let numbers = contrast(color("text3"), background);
    assert!(numbers >= 7., "line numbers are {numbers:.2}:1, below 7:1");
    let selected = composite(color("selection"), background);
    // Identifiers and the current line's number: the widgets' foreground.
    let mut inks = vec![("foreground".to_owned(), color("text"))];
    // Read through serde, which names each colour as the theme file does.
    let syntax = serde_json::to_value(&highlight.syntax).unwrap();
    for (name, style) in syntax.as_object().unwrap() {
        if let Some(value) = style.get("color").filter(|value| !value.is_null()) {
            let ink: gpui_kit::Hsla = serde_json::from_value(value.clone()).unwrap();
            inks.push((name.clone(), ink.into()));
        }
    }
    for token in ["comment", "keyword", "string", "number", "function", "type"] {
        assert!(
            inks.iter().any(|(name, _)| name == token),
            "no colour for {token}"
        );
    }
    for (name, ink) in inks {
        for (ground, under) in [("the editor", background), ("a selection", selected)] {
            let ratio = contrast(ink, under);
            assert!(ratio >= 7., "{name} on {ground} is {ratio:.2}:1, below 7:1");
        }
    }
}
