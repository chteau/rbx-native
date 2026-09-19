use super::*;
use gpui_kit::px;

fn scrub(kind: FieldKind, start: f32) -> Scrub {
    Scrub {
        property: "Position".to_owned(),
        index: 0,
        kind,
        origin: px(100.),
        start,
    }
}

fn plain() -> Modifiers {
    Modifiers::default()
}

fn with(shift: bool, alt: bool) -> Modifiers {
    Modifiers {
        shift,
        alt,
        ..Modifiers::default()
    }
}

/// An integer field steps through whole units and never lands between them
/// — dragging a `Vector3int16` walks cells.
#[test]
fn an_integer_field_only_ever_lands_on_whole_numbers() {
    let scrub = scrub(FieldKind::Integer, 0.);

    for offset in 0..40 {
        let value = scrub
            .value_at(px(100. + offset as f32), plain())
            .expect("an integer field is draggable");
        assert_eq!(value, value.round(), "{value} is not whole");
    }
    // Six pixels to a unit, so sixty pixels is ten units.
    assert_eq!(scrub.value_at(px(160.), plain()), Some(10.));
}

/// A decimal field moves in hundredths and is rounded to what the panel
/// prints, so a drag never leaves `0.30000001` behind.
#[test]
fn a_decimal_field_keeps_only_the_precision_it_displays() {
    let scrub = scrub(FieldKind::Decimal, 0.);
    let value = scrub.value_at(px(107.), plain()).expect("draggable");

    assert_eq!(value, 0.35);
    assert_eq!((value * 1000.).round() / 1000., value);
}

/// Dragging left goes down, and the value is always measured from where the
/// drag *started* — never accumulated, or a long drag drifts.
#[test]
fn dragging_back_past_the_origin_returns_to_where_it_began() {
    let scrub = scrub(FieldKind::Decimal, 5.);

    assert_eq!(scrub.value_at(px(100.), plain()), Some(5.));
    assert_eq!(scrub.value_at(px(120.), plain()), Some(6.));
    assert_eq!(scrub.value_at(px(80.), plain()), Some(4.));
}

#[test]
fn shift_coarsens_and_alt_refines() {
    let scrub = scrub(FieldKind::Decimal, 0.);

    assert_eq!(scrub.value_at(px(120.), plain()), Some(1.));
    assert_eq!(scrub.value_at(px(120.), with(true, false)), Some(10.));
    assert_eq!(scrub.value_at(px(120.), with(false, true)), Some(0.1));
}

/// A `Font`'s family name is a field like any other to look at, and must
/// not respond to a drag at all.
#[test]
fn a_text_field_cannot_be_dragged() {
    assert_eq!(FieldKind::Text.step_per_pixel(), None);
    assert_eq!(scrub(FieldKind::Text, 0.).value_at(px(200.), plain()), None);
}

/// What the field reads back afterwards.
#[test]
fn a_dragged_value_is_written_the_way_its_type_is_spelled() {
    assert_eq!(format(4., FieldKind::Integer), "4");
    assert_eq!(format(-12., FieldKind::Integer), "-12");
    assert_eq!(
        format(4., FieldKind::Decimal),
        "4",
        "a decimal that landed on a whole number shows no trailing zeros"
    );
    assert_eq!(format(0.35, FieldKind::Decimal), "0.35");
    assert_eq!(format(-0.5, FieldKind::Decimal), "-0.5");
    assert_eq!(
        format(0.0001, FieldKind::Decimal),
        "0",
        "below the displayed precision reads as zero, not as an empty field"
    );
}
