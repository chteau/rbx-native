use super::*;

#[test]
fn every_preset_clears_every_bar() {
    for (name, value) in PRESETS {
        for check in checks(rgb(value)) {
            assert!(check.passes(), "{name}: {} {:.2}", check.label, check.ratio);
        }
        assert_eq!(fix(rgb(value)), None, "{name}");
    }
}

#[test]
fn indigo_measures_what_the_page_shows() {
    let ratios = checks(rgb(0x6C7FDB)).map(|check| (check.ratio * 10.).round() / 10.);
    assert_eq!(ratios, [5.4, 5.1, 4.7]);
}

#[test]
fn a_colour_too_dark_is_lifted_to_the_nearest_that_passes() {
    let dark = rgb(0x3A3F8F);
    assert!(!passes(dark));
    let fixed = fix(dark).expect("a lighter indigo passes");
    assert_eq!(hex(fixed), "#7176C5");
    assert!(passes(fixed));
    // Same hue, within what rounding to a hex code moves it.
    assert!((hls(fixed).0 - hls(dark).0).abs() < 0.01);
}

#[test]
fn a_hue_near_a_status_colour_is_flagged_and_a_grey_one_is_not() {
    assert_eq!(near_status(rgb(0xE06C6C)), Some(Status::Error));
    assert_eq!(near_status(rgb(0xE8A04C)), Some(Status::Warning));
    assert_eq!(near_status(rgb(0x6CD08C)), Some(Status::Success));
    for (_, value) in PRESETS {
        assert_eq!(near_status(rgb(value)), None, "{}", hex(rgb(value)));
    }
    // A red hue, but too grey to read as the error colour.
    assert_eq!(near_status(rgb(0x8A7777)), None);
}

#[test]
fn every_preset_keeps_a_selected_row_readable_and_visible() {
    let max = f32::from(0xB8_u8) / 255.;
    for (name, value) in PRESETS {
        let accent = rgb(value);
        let alpha = selection_alpha(accent, max);
        assert!(alpha <= max, "{name}");
        let dock = tokens::dock();
        let mix = |a: f32, b: f32| a * alpha + b * (1. - alpha);
        let row = Rgba {
            r: mix(accent.r, dock.r),
            g: mix(accent.g, dock.g),
            b: mix(accent.b, dock.b),
            a: 1.,
        };
        assert!(contrast(tokens::text_full(), row) >= 4.5, "{name}: label");
        assert!(
            contrast(row, dock) >= 3.,
            "{name}: state {:.2}",
            contrast(row, dock)
        );
    }
    // Indigo keeps the theme's own wash.
    assert_eq!(selection_alpha(rgb(0x6C7FDB), max), max);
}

#[test]
fn hsv_round_trips_through_every_preset() {
    for (name, value) in PRESETS {
        let (h, s, v) = hsv(rgb(value));
        assert_eq!(hex(from_hsv(h, s, v)), hex(rgb(value)), "{name}");
    }
}

#[test]
fn every_default_tool_pastel_clears_the_ribbon_and_a_dark_one_is_lifted() {
    for value in [
        0x8FB8FF, 0x8FE0B0, 0xFF9B9B, 0xFFC98F, 0xC9A8FF, 0x8FE0D8, 0xFFE88F,
    ] {
        assert!(tool_check(rgb(value)).passes(), "{}", hex(rgb(value)));
    }
    let dark = rgb(0x203040);
    let lifted = lighten_until(dark, |c| tool_check(c).passes()).expect("a lighter one passes");
    assert!(tool_check(lifted).passes());
}

#[test]
fn hex_round_trips_and_rejects_junk() {
    assert_eq!(parse_hex("#6c7fdb").map(hex).as_deref(), Some("#6C7FDB"));
    assert_eq!(parse_hex("A0A8B8").map(hex).as_deref(), Some("#A0A8B8"));
    assert_eq!(parse_hex("#12345"), None);
    assert_eq!(parse_hex("#GGGGGG"), None);
}

#[test]
fn hls_matches_pythons_colorsys() {
    let (h, l, s) = hls(rgb(0x3A3F8F));
    assert!((h - 0.656_862_7).abs() < 1e-4, "{h}");
    assert!((l - 0.394_117_6).abs() < 1e-4, "{l}");
    assert!((s - 0.422_885_6).abs() < 1e-4, "{s}");
    assert_eq!(hex(quantize(from_hls(h, l, s))), "#3A3F8F");
}

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

/// A selected row and the active document tab are *states*, and WCAG 1.4.11
/// does not exempt states. The tab's wash cannot carry 3:1 on its own (a
/// dark wash on a dark strip never will), which is exactly why it travels
/// with an accent rule — and this asserts the rule, not the wash.
#[test]
fn a_selection_and_an_open_document_are_visible_as_states() {
    // The selection is the accent at the opacity `accent::selection_alpha`
    // derives for it, up to the default theme's own.
    for (accent, value) in PRESETS {
        let alpha = selection_alpha(rgb(value), tokens::selection().a);
        let selected = composite(
            Rgba {
                a: alpha,
                ..rgb(value)
            },
            tokens::dock(),
        );
        let ratio = contrast(selected, tokens::dock());
        assert!(
            ratio >= 3.,
            "{accent}: a selected Explorer row is {ratio:.2}:1 against the dock, below the 3:1 state floor"
        );
        assert!(
            contrast(tokens::text_full(), selected) >= 4.5,
            "{accent}: a selected row's own label must still clear AA on top of the selection"
        );

        let bar = contrast(rgb(value), tokens::chrome());
        assert!(
            bar >= 3.,
            "{accent}: the open document's accent rule is {bar:.2}:1 against the tab strip"
        );
    }
}

/// A toggle is the only place this UI spends colour on state, so its two
/// states have to be told apart at a glance.
///
/// The accent is the user's to pick, so this holds for every preset rather
/// than for one colour; a custom colour gets the same bar from
/// `accent::checks` before it can be applied.
#[test]
fn a_toggle_reads_differently_on_and_off() {
    for (name, value) in PRESETS {
        let ratio = contrast(rgb(value), tokens::field_select());
        assert!(
            ratio >= 3.,
            "{name}: an on vs an off toggle is {ratio:.2}:1, below the 3:1 non-text floor"
        );
    }
}
