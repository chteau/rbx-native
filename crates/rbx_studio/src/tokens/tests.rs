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

/// WCAG 2.1 contrast ratio. Both colours must be opaque — a translucent
/// token has to be [`composite`]d onto what it actually sits on first, or
/// the ratio is a fiction.
fn contrast(a: Rgba, b: Rgba) -> f32 {
    let (a, b) = (luminance(a), luminance(b));
    let (lighter, darker) = (a.max(b), a.min(b));
    (lighter + 0.05) / (darker + 0.05)
}

/// `foreground` (with its own alpha) painted over an opaque `background`.
fn composite(foreground: Rgba, background: Rgba) -> Rgba {
    let mix = |f: f32, b: f32| f * foreground.a + b * (1. - foreground.a);
    Rgba {
        r: mix(foreground.r, background.r),
        g: mix(foreground.g, background.g),
        b: mix(foreground.b, background.b),
        a: 1.,
    }
}

/// Body text has to clear WCAG AA (4.5:1) on every surface it can land on.
#[test]
fn primary_text_clears_aa_on_every_surface() {
    for (name, surface) in [
        ("bg-0", bg_0()),
        ("bg-1", bg_1()),
        ("bg-2", bg_2()),
        ("bg-3", bg_3()),
    ] {
        let ratio = contrast(text_primary(), surface);
        assert!(
            ratio >= 4.5,
            "text-primary on {name} is {ratio:.2}:1, below WCAG AA 4.5:1"
        );
    }
}

/// Secondary text is UI text, so 3:1 is the floor it must clear — it
/// comfortably clears body-text AA too, which is why labels stay readable
/// rather than merely "identifiable".
#[test]
fn secondary_text_clears_the_ui_text_floor_on_every_surface() {
    for (name, surface) in [
        ("bg-0", bg_0()),
        ("bg-1", bg_1()),
        ("bg-2", bg_2()),
        ("bg-3", bg_3()),
    ] {
        let ratio = contrast(text_secondary(), surface);
        assert!(
            ratio >= 3.,
            "text-secondary on {name} is {ratio:.2}:1, below the 3:1 UI-text floor"
        );
    }
}

/// The active-tab case the spec calls out by name: accent-coloured label on
/// the translucent accent pill, over whichever shell surface the tab bar
/// sits on.
#[test]
fn accent_text_on_its_own_soft_background_clears_aa() {
    for (name, surface) in [("bg-0", bg_0()), ("bg-1", bg_1())] {
        let pill = composite(accent_soft_bg(), surface);
        let ratio = contrast(accent(), pill);
        assert!(
            ratio >= 4.5,
            "accent on accent-soft-bg over {name} is {ratio:.2}:1, below WCAG AA 4.5:1"
        );
    }
}

/// An input is identified by three things stacked, none of which carries it
/// alone: its fill against the panel behind it (bg-3 on bg-1, 1.20:1), its
/// `border_mid` outline (1.35:1), and — once it matters, at focus — the
/// accent border at 5.48:1. This pins the fill step, the quietest of the
/// three, so a future palette tweak can't flatten the field into its panel
/// and leave only the hairline doing the work.
#[test]
fn an_inputs_fill_stays_distinguishable_from_the_panel_behind_it() {
    let ratio = contrast(bg_3(), bg_1());
    assert!(
        ratio >= 1.15,
        "bg-3 against bg-1 is {ratio:.2}:1 — an input field would vanish into its panel"
    );
}

/// Error text is the one semantic colour that carries meaning on its own,
/// so it has to be readable rather than merely visible.
#[test]
fn error_text_clears_aa() {
    let ratio = contrast(text_error(), bg_1());
    assert!(ratio >= 4.5, "text-error on bg-1 is {ratio:.2}:1");
}

/// A timing function has to start at 0, end at 1, and never go backwards —
/// otherwise an animation driven by it stutters or runs in reverse.
#[test]
fn the_soft_easing_runs_forward_from_zero_to_one() {
    assert!(easing_soft(0.).abs() < 1e-3, "{}", easing_soft(0.));
    assert!((easing_soft(1.) - 1.).abs() < 1e-3, "{}", easing_soft(1.));

    let mut previous = 0.;
    for step in 0..=100 {
        let value = easing_soft(step as f32 / 100.);
        assert!(
            value >= previous - 1e-4,
            "easing_soft went backwards at t={}: {value} < {previous}",
            step as f32 / 100.
        );
        previous = value;
    }
}

/// Both elevation layers have to be present and in the right order: the
/// ring first (it defines the edge), then the blur (it defines the depth).
#[test]
fn an_elevation_is_a_ring_then_a_blur() {
    let shadows = elevation_3(Cast::Right);
    assert_eq!(shadows.len(), 2);
    assert_eq!(
        shadows[0].blur_radius,
        px(0.),
        "the ring layer must be crisp"
    );
    assert_eq!(shadows[0].spread_radius, px(1.), "the ring layer is 1px");
    assert!(
        shadows[1].blur_radius > px(0.),
        "the depth layer must be blurred"
    );
    assert_eq!(
        shadows[1].offset.x,
        px(8.),
        "a right-casting panel throws its shadow rightward"
    );
    assert_eq!(shadows[1].offset.y, px(0.));
}

/// The focus halo is a gap in the surface colour, then the glow — swap the
/// order (or lose the gap) and it reads as a hard outline instead.
#[test]
fn the_focus_ring_puts_a_surface_gap_inside_the_glow() {
    let ring = focus_ring(bg_1());
    assert_eq!(ring.len(), 2);
    assert_eq!(ring[0].spread_radius, px(2.));
    assert_eq!(ring[1].spread_radius, px(4.));
    assert_eq!(Rgba::from(ring[0].color).r, bg_1().r);
}
