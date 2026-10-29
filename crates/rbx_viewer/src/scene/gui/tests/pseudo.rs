//! The pseudo-localization preview: the string transform, and which text
//! objects it pushes out of their boxes, against the fixed-advance measure
//! [`super::text::Fixed`].

use super::text::Fixed;
use super::*;
use crate::scene::gui::pseudo::localize_spans;
use crate::scene::gui::{overflows, pseudo_localize, resolve_with, Align, PseudoLocale, TextSpan};

const PLAIN: PseudoLocale = PseudoLocale { mirror: false };
const MIRROR: PseudoLocale = PseudoLocale { mirror: true };

#[test]
fn letters_are_accented_and_the_string_is_bracketed() {
    assert_eq!(pseudo_localize("The"), "[! Ţĥé !]");
    let long = "Buy twenty five gems now";
    let out = pseudo_localize(long);
    assert!(out.starts_with("[!!! ") && out.ends_with(" !!!]"), "{out}");
    let growth = out.chars().count() as f32 / long.chars().count() as f32;
    assert!((1.35..1.6).contains(&growth), "grew by {growth}");
}

#[test]
fn the_transform_is_deterministic_and_idempotent() {
    for text in [
        "Play",
        "Settings menu",
        "Coins: %d",
        "Hi {name}!",
        "<b>Bold</b> move",
    ] {
        let once = pseudo_localize(text);
        assert_eq!(once, pseudo_localize(text));
        assert_eq!(pseudo_localize(&once), once, "{text}");
    }
    assert_eq!(pseudo_localize("   "), "   ", "blank text stays blank");
}

#[test]
fn tags_specifiers_and_placeholders_are_left_whole() {
    let out = pseudo_localize("<b>Score</b> %d of %.2f, {name} &amp; {1} 100%%");
    for kept in ["<b>", "</b>", "%d", "%.2f", "{name}", "&amp;", "{1}", "%%"] {
        assert!(out.contains(kept), "{kept} missing from {out}");
    }
    assert!(out.contains("Šçöŕé"), "{out}");
    assert!(!out.contains("<ƀ>"));
}

#[test]
fn mirroring_reverses_the_spans_and_keeps_placeholders_readable() {
    let mut spans = vec![TextSpan::plain("Hi "), TextSpan::plain("{name}")];
    localize_spans(&mut spans, MIRROR);
    let whole: String = spans.iter().map(|span| span.text.as_str()).collect();
    assert_eq!(whole, "[! {name} îĤ !]");
}

fn label(dom: &mut WeakDom, parent: Ref, text: &str, size: (i32, i32)) -> Ref {
    let referent = dom.new_instance("TextLabel", text, Some(parent));
    dom.set_property(referent, "Text", Variant::String(text.into()))
        .unwrap();
    dom.set_property(referent, "Size", udim2(0.0, size.0, 0.0, size.1))
        .unwrap();
    referent
}

/// The labels that overflow once the place is drawn in `locale`.
fn overflowing(dom: &WeakDom, locale: Option<PseudoLocale>) -> Vec<Ref> {
    let screens: Vec<Screen> = screens(dom)
        .iter()
        .map(|screen| match locale {
            Some(locale) => screen.pseudo_localized(locale),
            None => screen.clone(),
        })
        .collect();
    resolve_with(&screens, VIEWPORT, &mut Fixed)
        .iter()
        .filter(|element| {
            let typeset = element.text.as_ref().unwrap();
            overflows(typeset, &element.rect, &mut Fixed)
        })
        .map(|element| element.referent)
        .collect()
}

#[test]
fn a_label_that_only_fits_its_english_text_overflows_pseudo_localized() {
    let (mut dom, gui) = screen_gui();
    // 14px text is 7px a character here: 100px holds 14 of them.
    let short = label(&mut dom, gui, "Play", (100, 20));
    let tight = label(&mut dom, gui, "Settings menu", (100, 20));
    assert!(overflowing(&dom, None).is_empty());
    assert_eq!(overflowing(&dom, Some(PLAIN)), [tight]);
    assert_eq!(overflowing(&dom, Some(MIRROR)), [tight]);
    let _ = short;
}

#[test]
fn wrapping_overflows_on_height_and_automatic_size_or_scaling_absorbs_it() {
    let (mut dom, gui) = screen_gui();
    let wrapped = label(&mut dom, gui, "Settings menu", (100, 20));
    dom.set_property(wrapped, "TextWrapped", Variant::Bool(true))
        .unwrap();
    let tall = label(&mut dom, gui, "Settings menu", (100, 40));
    dom.set_property(tall, "TextWrapped", Variant::Bool(true))
        .unwrap();
    let grows = label(&mut dom, gui, "Settings menu", (100, 20));
    dom.set_property(grows, "AutomaticSize", Variant::Enum(1))
        .unwrap();
    let scaled = label(&mut dom, gui, "Settings menu", (100, 20));
    dom.set_property(scaled, "TextScaled", Variant::Bool(true))
        .unwrap();
    assert_eq!(overflowing(&dom, Some(PLAIN)), [wrapped]);
}

#[test]
fn a_mirrored_label_swaps_its_alignment_and_the_plan_itself_is_untouched() {
    let (mut dom, gui) = screen_gui();
    let left = label(&mut dom, gui, "Play", (100, 20));
    dom.set_property(left, "TextXAlignment", Variant::Enum(0))
        .unwrap();
    let planned = screens(&dom);
    let mirrored = planned[0].pseudo_localized(MIRROR);
    let text = |screens: &[Screen]| {
        resolve_with(screens, VIEWPORT, &mut Fixed)[0]
            .text
            .clone()
            .unwrap()
            .text
    };
    assert_eq!(text(&[mirrored]).x_align, Align::End);
    let original = text(&planned);
    assert_eq!(original.x_align, Align::Start);
    assert_eq!(original.spans[0].text, "Play");
}

#[test]
fn text_auto_localize_turns_off_is_left_as_written() {
    let (mut dom, gui) = screen_gui();
    let names = frame(
        &mut dom,
        gui,
        udim2(0.0, 0, 0.0, 0),
        udim2(0.0, 200, 0.0, 100),
    );
    dom.set_property(names, "AutoLocalize", Variant::Bool(false))
        .unwrap();
    label(&mut dom, names, "Builderman", (100, 20));
    label(&mut dom, gui, "Play", (100, 20));
    let screens: Vec<Screen> = screens(&dom)
        .iter()
        .map(|screen| screen.pseudo_localized(PLAIN))
        .collect();
    let texts: Vec<String> = resolve_with(&screens, VIEWPORT, &mut Fixed)
        .iter()
        .filter_map(|element| element.text.as_ref())
        .map(|typeset| typeset.text.spans[0].text.clone())
        .collect();
    assert_eq!(texts, ["Builderman", "[! Þļåý !]"]);
}
