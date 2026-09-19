use super::*;

/// WCAG 2.1 relative luminance of a straight (non-premultiplied) colour.
fn luminance(color: Rgba) -> f32 {
    let channel = |c: f32| {
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)
}

fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// `foreground` (with its own alpha) painted over an opaque `background` —
/// which is how every text token in this design is specified.
fn composite(foreground: Rgba, background: Rgba) -> Rgba {
    let mix = |f: f32, b: f32| f * foreground.a + b * (1. - foreground.a);
    Rgba {
        r: mix(foreground.r, background.r),
        g: mix(foreground.g, background.g),
        b: mix(foreground.b, background.b),
        a: 1.,
    }
}

use gpui_kit::component::Size;

/// Every surface a label can land on in this design.
const SURFACES: [Named; 6] = [
    ("black", black),
    ("dock", dock),
    ("chrome", chrome),
    ("field-select", field_select),
    ("tile", tile),
    ("menu-bar", menu_bar),
];

/// The design's text colours are alphas over dark surfaces, which is
/// exactly the setup where "looks fine to me" goes wrong. This is the check
/// that keeps them readable: every label carrying meaning clears WCAG AA on
/// every surface it can sit on.
#[test]
fn meaningful_text_clears_aa_on_every_surface() {
    let tokens: [Named; 5] = [
        ("text-full", text_full),
        ("text-strong", text_strong),
        ("text-label", text_label),
        ("text-muted", text_muted),
        ("text-placeholder", text_placeholder),
    ];

    for (name, token) in tokens {
        for (surface_name, surface) in SURFACES {
            let ratio = contrast(composite(token(), surface()), surface());
            assert!(
                ratio >= 4.5,
                "{name} on {surface_name} is {ratio:.2}:1, below WCAG AA 4.5:1"
            );
        }
    }
}

/// A read-only property's *name* is dimmed on purpose (WCAG exempts
/// disabled controls), but it still has to be *there*: dim enough to read
/// as unavailable, not so dim it looks like a rendering fault.
///
/// Its *value* is not dimmed to match — that is real data, and it goes
/// through `text_muted`, which the AA test above covers.
#[test]
fn disabled_text_stays_visible_without_reading_as_available() {
    for (name, surface) in SURFACES {
        let ratio = contrast(composite(text_disabled(), surface()), surface());
        assert!(
            ratio > 2.,
            "text-disabled on {name} is {ratio:.2}:1 — invisible"
        );
        assert!(
            ratio < 4.5,
            "text-disabled on {name} is {ratio:.2}:1 — indistinguishable from an editable value"
        );
    }
}

/// A dock has to be visibly a dock. The frame paints them the same black as
/// the window behind them, which makes the three docks and the ground one
/// undifferentiated field — this is the step that separates them, and it
/// has to survive anyone "simplifying" the palette back down.
/// A dock has to be visibly a dock, and a dropdown visibly not a text
/// field. The frame paints every one of these the same black, which makes
/// the docks and the ground one undifferentiated field.
///
/// Measured as a **luminance difference**, not a contrast ratio. Down here
/// a ratio is the wrong instrument: near black it is dominated by WCAG's
/// `+0.05` term, so two surfaces 20 sRGB levels apart and two 4 levels
/// apart score almost the same. The delta is what the eye actually has to
/// find.
#[test]
fn each_surface_is_a_visible_step_above_the_one_below_it() {
    type Step = (&'static str, fn() -> Rgba, fn() -> Rgba);
    let steps: [Step; 5] = [
        ("dock over black", dock, black),
        ("chrome over dock", chrome, dock),
        ("a dropdown over a text field", field_select, chrome),
        ("tile over a dropdown", tile, field_select),
        ("the menu strip over a dock", menu_bar, dock),
    ];

    for (name, over, under) in steps {
        let step = luminance(over()) - luminance(under());
        assert!(
            step >= 0.004,
            "{name} is a step of {step:.4} — the two surfaces read as one"
        );
    }
}

/// A selected row and the active document tab are *states*, and WCAG 1.4.11
/// does not exempt states. The tab's wash cannot carry 3:1 on its own (a
/// dark wash on a dark strip never will), which is exactly why it travels
/// with an accent rule — and this asserts the rule, not the wash.
#[test]
fn a_selection_and_an_open_document_are_visible_as_states() {
    let selected = composite(selection(), dock());
    let ratio = contrast(selected, dock());
    assert!(
        ratio >= 3.,
        "a selected Explorer row is {ratio:.2}:1 against the dock, below the 3:1 state floor"
    );
    assert!(
        contrast(text_full(), selected) >= 4.5,
        "a selected row's own label must still clear AA on top of the selection"
    );

    let bar = contrast(tab_active_bar(), chrome());
    assert!(
        bar >= 3.,
        "the open document's accent rule is {bar:.2}:1 against the tab strip"
    );
}

/// A checkbox is the only place this UI spends colour on state, so its two
/// states have to be told apart at a glance.
#[test]
fn a_checkbox_reads_differently_ticked_and_unticked() {
    let ratio = contrast(check_on(), check_off());
    assert!(
        ratio >= 3.,
        "ticked vs unticked checkbox is {ratio:.2}:1, below the 3:1 non-text floor"
    );
}

/// The active document tab is a *darker* wash, not a lighter one — the open
/// document is recessed into the strip. If this ever inverts, the whole tab
/// strip starts reading upside down.
#[test]
fn the_active_document_tab_is_darker_than_the_strip() {
    let active = luminance(composite(tab_active(), chrome()));
    let strip = luminance(chrome());
    assert!(
        active < strip,
        "the active tab ({active:.4}) is no darker than the tab strip ({strip:.4})"
    );
}

/// The toolkit's own components — the menu bar, the inputs, the buttons in
/// the Output strip — are painted from `assets/themes/dark-soft.json`, not
/// from this module. That is two copies of one palette, which is exactly
/// the arrangement that drifts: a colour is changed here, the JSON keeps
/// the old one, and half the window quietly stops matching the other half.
///
/// So the JSON is checked against the tokens, key by key.
#[test]
fn the_toolkit_theme_paints_the_same_palette_this_module_does() {
    const THEME: &str = include_str!("../../../../assets/themes/dark-soft.json");

    let theme: serde_json::Value =
        serde_json::from_str(THEME).expect("dark-soft.json is valid JSON");
    let colors = &theme["themes"][0]["colors"];

    let mirrored: [Named; 14] = [
        ("background", black),
        ("popover.background", chrome),
        ("border", tab_border),
        ("input.border", divider),
        ("muted.background", chrome),
        ("sidebar.background", dock),
        ("muted.foreground", text_placeholder),
        ("primary.background", check_on),
        ("ring", check_on),
        ("selection.background", selection),
        ("accent.background", hover),
        ("tab_bar.background", chrome),
        ("title_bar.background", black),
        ("danger.background", text_error),
    ];

    for (key, token) in mirrored {
        let listed = colors[key]
            .as_str()
            .unwrap_or_else(|| panic!("dark-soft.json has no {key}"));
        assert_eq!(
            listed.to_ascii_uppercase(),
            hex(token()),
            "dark-soft.json's {key} has drifted from the token it mirrors"
        );
    }

    assert_eq!(
        theme["themes"][0]["radius"].as_f64(),
        Some(f64::from(f32::from(RADIUS))),
        "dark-soft.json's radius has drifted from RADIUS"
    );
}

/// `#RRGGBB`, or `#RRGGBBAA` when the colour is not opaque — the spelling
/// `dark-soft.json` uses.
fn hex(color: Rgba) -> String {
    let byte = |channel: f32| (channel * 255.).round() as u8;
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
}

/// [`field_size`] exists to force a toolkit widget's text to [`text_md`],
/// and the 0.875 factor it compensates for is the toolkit's, not ours — so
/// this is what catches a toolkit upgrade that changes it.
#[test]
fn a_toolkit_field_is_set_at_the_same_size_as_its_label() {
    let Size::Size(size) = field_size() else {
        panic!("field_size must be an explicit size, not one of the toolkit's steps");
    };
    // What `StyleSized::input_text_size` does with a `Size::Size`.
    let rendered = f32::from(size) * 0.875;
    assert!(
        (rendered - f32::from(text_md())).abs() < 0.01,
        "a value field renders at {rendered}px against a {}px label",
        f32::from(text_md())
    );
}

/// WCAG 2.4.13's other half: the ring has to clear 3:1 against the
/// component it outlines *and* against whatever is behind that — otherwise
/// a 2px ring is technically present and practically invisible, which is
/// the failure mode the criterion exists to catch.
#[test]
fn the_focus_ring_clears_three_to_one_on_every_surface_it_can_land_on() {
    for (name, surface) in SURFACES {
        let ratio = contrast(check_on(), surface());
        assert!(
            ratio >= 3.,
            "the focus ring on {name} is {ratio:.2}:1, below the 3:1 non-text floor"
        );
    }
}

/// WCAG 1.4.11 for the per-tool pastels. They only ever appear on a ribbon
/// tile, so that is the surface they are measured against — but an active
/// tool is also identifiable by its icon shape and its border, so this
/// checking out is a floor rather than the whole story (WCAG 1.4.1).
#[test]
fn every_tool_accent_clears_three_to_one_on_a_ribbon_tile() {
    for (name, accent) in TOOL_ACCENTS {
        let ratio = contrast(accent(), tile());
        assert!(
            ratio >= 3.,
            "the {name} tool's accent is {ratio:.2}:1 against a tile, below 3:1"
        );
    }
}

/// An unticked checkbox is a control whose entire job is to show a state,
/// so its outline is "visual information required to identify the
/// component" — 3:1 on both surfaces a dock can put it on. The design frame
/// gives it no border at all; this is the override.
#[test]
fn an_unticked_checkbox_has_a_visible_edge_on_both_dock_surfaces() {
    for (name, surface) in [("black", black as fn() -> Rgba), ("chrome", chrome)] {
        let ratio = contrast(check_off_border(), surface());
        assert!(
            ratio >= 3.,
            "an unticked checkbox's edge on {name} is {ratio:.2}:1, below 3:1"
        );
    }
}

/// WCAG 2.5.8: 24x24 is the floor for a pointer target. These are the three
/// smallest things in the shell, and the frame drew two of them well under
/// it — the checkbox at 10px and the panel buttons at 20px.
/// WCAG 2.5.8: 24x24 is the floor for a pointer target.
///
/// Checked **at every scale**, not just at 1.0x. The previous version of
/// this test ran only at the default, which is the one setting where none
/// of these tokens can fail — at 0.5x they were 13, 14 and 15px, and the
/// test still passed. A floor that is only asserted where it cannot be
/// crossed is not a floor.
#[test]
fn the_smallest_targets_clear_the_minimum_pointer_size_at_every_scale() {
    let _guard = SCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    for scale in [FONT_SCALE_RANGE.0, 0.75, 1., 1.5, FONT_SCALE_RANGE.1] {
        set_font_scale(scale);
        for (name, size) in [
            ("icon button", hit_target()),
            ("checkbox target", checkbox_target()),
            ("input", input_height()),
            ("property row", row_height()),
            ("tree row", tree_row_height()),
        ] {
            assert!(
                f32::from(size) >= 24.,
                "at {scale}x a {name} is {size:?}, under WCAG 2.5.8's 24x24 floor"
            );
        }
    }
    set_font_scale(1.);
}

/// The scale is a process-wide atomic, and `cargo test` runs these on
/// threads of one process — so the two tests that move it take turns.
static SCALE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The other half of the scale's contract: everything that is *not* a
/// pointer target does scale freely, in both directions. A token that
/// quietly ignored `font_scale` would leave the UI half-resized.
#[test]
fn every_scaled_size_actually_follows_the_scale() {
    let _guard = SCALE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let at = |scale: f32| {
        set_font_scale(scale);
        [
            f32::from(text_md()),
            f32::from(text_sm()),
            f32::from(text_xs()),
            f32::from(ribbon_height()),
            f32::from(tab_width()),
            f32::from(tool_border()),
            dock_width(),
        ]
    };

    let small = at(0.5);
    let large = at(2.);
    set_font_scale(1.);

    for (index, (small, large)) in small.iter().zip(large.iter()).enumerate() {
        assert!(
            large > small,
            "token {index} is {small} at 0.5x and {large} at 2.0x — it ignores the scale"
        );
    }
}

/// The UI scale is the app's whole answer to WCAG 1.4.4's 200%, so the top
/// of its range has to actually reach 200% — and the bottom has to stop
/// somewhere usable rather than at zero.
#[test]
fn the_ui_scale_range_reaches_two_hundred_percent() {
    let (low, high) = FONT_SCALE_RANGE;
    assert!(high >= 2., "the scale tops out at {high}x, short of 200%");
    assert!(low > 0., "the scale bottoms out at {low}x");
}

/// The toolkit theme mirrors this palette, and a colour changed in one
/// place and not the other is invisible until someone opens a menu. Both
/// this and `the_toolkit_theme_paints_the_same_palette_this_module_does`
/// are cheap; the drift they catch is not.
#[test]
fn the_type_ramp_descends() {
    assert!(f32::from(text_md()) > f32::from(text_sm()));
    assert!(f32::from(text_sm()) > f32::from(text_xs()));
    assert!(
        f32::from(line_md()) > f32::from(text_md()),
        "a line height at or under its own font size clips descenders"
    );
}
