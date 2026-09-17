//! Unit tests for [`super`]: the em ratio, which weights and styles a face
//! synthesises, and the dilated bitmap a synthesised bold draws.

use std::sync::Arc;

use super::*;
use crate::fonts::Entry;
use crate::scene::{GuiAlign, GuiTextSpan};

fn plain(face: Face, size: f32) -> GuiText {
    GuiText {
        spans: vec![GuiTextSpan::plain("Ag")],
        color: [1.0; 3],
        alpha: 1.0,
        size,
        scaled: false,
        wrapped: false,
        x_align: GuiAlign::Start,
        y_align: GuiAlign::Start,
        face,
        line_height: 1.0,
        stroke: None,
        truncate: false,
        max_graphemes: None,
        automatic: [false, false],
        size_bounds: None,
    }
}

/// A typesetter that believes `family` has landed with these
/// (weight, italic) faces — with no font file at all, which the
/// attributes and metrics a shape asks for never need.
fn believing(family: &AssetRef, faces: &[(u16, bool)]) -> Typesetter {
    let mut fonts = Typesetter::new();
    fonts.families.insert(
        family.clone(),
        Family {
            name: "Believed".to_string(),
            faces: faces
                .iter()
                .map(|&(weight, italic)| Entry {
                    weight,
                    italic,
                    asset: AssetRef::Native(format!("fonts/Believed-{weight}-{italic}.ttf")),
                })
                .collect(),
        },
    );
    fonts
}

/// The attributes the first span of `face` shapes with.
fn attrs_of(fonts: &mut Typesetter, face: Face) -> cosmic_text::AttrsOwned {
    let buffer = fonts.shape(&plain(face, 24.0), 24.0, None, None);
    cosmic_text::AttrsOwned::new(&buffer.lines[0].attrs_list().get_span(0))
}

// Roblox's docs make `TextSize` the line height; the em is a fifth
// smaller, for a `<font size>` as much as for the base size, and
// `LineHeight` spaces the lines without touching the em.
#[test]
fn text_size_is_the_line_box_and_the_em_a_fifth_smaller() {
    let face = Face::named("Believed", 400, false);
    let mut fonts = believing(&face.family, &[(400, false)]);
    let mut text = plain(face, 24.0);
    text.spans.push(GuiTextSpan {
        size: Some(12.0),
        ..GuiTextSpan::plain("small")
    });
    text.line_height = 1.5;

    let buffer = fonts.shape(&text, 24.0, None, None);
    assert_eq!(buffer.metrics(), Metrics::new(20.0, 36.0));
    let spans = buffer.lines[0].attrs_list();
    assert_eq!(spans.get_span(0).metrics_opt, None, "the buffer's own");
    assert_eq!(
        spans.get_span(2).metrics_opt,
        Some(Metrics::new(10.0, 18.0).into()),
        "a <font size> is a line box too"
    );
}

// A family with a single regular face serves a bold request with that
// face, as Roblox does, rather than with some bold system font — and
// asks for it dilated once the request reaches SemiBold.
#[test]
fn a_weight_the_family_lacks_shapes_in_the_face_that_landed_emboldened() {
    let face = Face::named("Believed", 700, false);
    let mut fonts = believing(&face.family, &[(400, false)]);

    let attrs = attrs_of(&mut fonts, face.clone());
    assert_eq!(
        attrs.family_owned,
        cosmic_text::FamilyOwned::Name("Believed".into())
    );
    assert_eq!(attrs.weight, Weight(400));
    assert_eq!(attrs.cache_key_flags, FAKE_BOLD);

    // Medium is not bold: the regular face as it is.
    let medium = Face::named("Believed", 500, false);
    assert_eq!(
        attrs_of(&mut fonts, medium).cache_key_flags,
        CacheKeyFlags::empty()
    );

    // Only a family that never landed keeps the request as asked, for
    // the fallback to make of what it can.
    let unknown = Face::named("Unknown", 700, false);
    let attrs = attrs_of(&mut fonts, unknown);
    assert_eq!(attrs.family_owned, cosmic_text::FamilyOwned::SansSerif);
    assert_eq!(attrs.weight, Weight(700));
    assert_eq!(attrs.cache_key_flags, CacheKeyFlags::empty());
}

// A face the family really has at (or nearest) the weight is never
// synthesised over.
#[test]
fn a_real_bold_face_is_preferred_to_a_synthesised_one() {
    let face = Face::named("Believed", 700, false);
    let mut fonts = believing(&face.family, &[(400, false), (700, false)]);

    let attrs = attrs_of(&mut fonts, face);
    assert_eq!(attrs.weight, Weight(700));
    assert_eq!(attrs.cache_key_flags, CacheKeyFlags::empty());

    // 600 lands on the 700 face, bold enough already.
    let semi = Face::named("Believed", 600, false);
    let attrs = attrs_of(&mut fonts, semi);
    assert_eq!(attrs.weight, Weight(700));
    assert_eq!(attrs.cache_key_flags, CacheKeyFlags::empty());
}

// The requested slant stays on the attributes whether or not the family
// has an italic face: cosmic-text slants an upright face itself when it
// is asked for italic and the face it matched is not.
#[test]
fn an_italic_request_keeps_its_style_for_the_face_to_be_slanted() {
    let face = Face::named("Believed", 400, true);
    let mut fonts = believing(&face.family, &[(400, false)]);
    let attrs = attrs_of(&mut fonts, face.clone());
    assert_eq!(attrs.style, Style::Italic);
    assert_eq!(attrs.weight, Weight(400));

    // With a real italic face, that face at its own weight.
    let mut fonts = believing(&face.family, &[(400, false), (300, true)]);
    let attrs = attrs_of(&mut fonts, face);
    assert_eq!(attrs.style, Style::Italic);
    assert_eq!(attrs.weight, Weight(300));
}

// The dilated bitmap of a glyph is wider and taller than the plain one,
// by about `FAKE_BOLD_EM` of the em each side. Shaped in the system's
// sans-serif, since no Roblox face is on hand; skipped where there is
// no system font either.
#[test]
fn a_fake_bold_glyph_is_dilated_by_the_em_fraction() {
    let mut fonts = Typesetter::new();
    let mut text = plain(Face::named("Unknown", 400, false), 48.0);
    text.spans = vec![GuiTextSpan::plain("H")];
    let buffer = fonts.shape(&text, 48.0, None, None);
    let Some(glyph) = buffer
        .layout_runs()
        .next()
        .and_then(|run| run.glyphs.first().cloned())
    else {
        return;
    };
    let plain_key = glyph.physical((0.0, 0.0), 1.0).cache_key;
    let bold_key = CacheKey {
        flags: plain_key.flags | FAKE_BOLD,
        ..plain_key
    };
    let (Some(plain), Some(bold)) = (fonts.glyph(plain_key), fonts.glyph(bold_key)) else {
        return;
    };
    // A 40 px em (48 / 1.2) dilates 1.67 px a side: 3 px across, give
    // or take the rasteriser's rounding.
    let grown = |bold: u32, plain: u32| (2..=5).contains(&(bold as i32 - plain as i32));
    assert!(grown(bold.width, plain.width), "{plain:?} -> {bold:?}");
    assert!(grown(bold.height, plain.height), "{plain:?} -> {bold:?}");
    assert!(
        bold.left <= plain.left && bold.top >= plain.top,
        "{plain:?} -> {bold:?}"
    );
}

// A face file that will not parse — a truncated download, a cloud asset
// that turned out not to be a font — must not be taken as the family: its
// text keeps shaping in the fallback, and the file is not tried again.
#[test]
fn a_face_that_is_not_a_font_is_never_adopted() {
    let face = Face::named("Shelf", 400, false);
    let asset = AssetRef::Native("fonts/Shelf-Regular.ttf".to_string());
    let mut library = Library::default();
    library.families.insert(
        face.family.clone(),
        Family {
            name: "Shelf".to_string(),
            faces: vec![Entry {
                weight: 400,
                italic: false,
                asset: asset.clone(),
            }],
        },
    );
    library.faces.insert(asset.clone(), Arc::new(vec![1, 2, 3]));
    let mut fonts = Typesetter::new();

    assert!(!fonts.adopt(&library, std::slice::from_ref(&face)));
    assert!(!fonts.knows(&face));
    assert!(fonts.loaded.contains(&asset), "not worth a second try");
    assert!(!fonts.adopt(&library, std::slice::from_ref(&face)));
}
