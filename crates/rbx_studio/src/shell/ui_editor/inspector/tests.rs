use rbx_dom::Variant;

use super::spec::{Form, On, Spec};
use super::value::{numbers_of, parse, read, show, write};

const NUMBER: Form = Form::Number {
    step: 1.0,
    whole: true,
};

fn spec(form: Form, parts: &'static [usize]) -> Spec {
    Spec {
        on: On::Own,
        properties: &["Position"],
        parts,
        form,
    }
}

#[test]
fn a_transparency_reads_and_writes_as_the_opacity_figma_shows() {
    assert_eq!(read(Form::Percent, 0, &[0.25]), Some(vec![75.0]));
    let percent = spec(Form::Percent, &[0]);
    assert_eq!(write(percent, vec![0.0], &[40.0]), "0.6");
    // Past either end is clamped, not stored as a transparency Roblox
    // would clamp itself.
    assert_eq!(write(percent, vec![0.0], &[140.0]), "0");
}

#[test]
fn a_udim2_field_writes_only_its_own_number() {
    let offset_x = spec(NUMBER, &[1]);
    assert_eq!(
        write(offset_x, vec![0.5, 10.0, 0.25, 4.0], &[32.0]),
        "0.5, 32, 0.25, 4"
    );
    let both_offsets = spec(NUMBER, &[1, 3]);
    assert_eq!(
        write(both_offsets, vec![0.0, 0.0, 0.0, 0.0], &[6.0]),
        "0, 6, 0, 6"
    );
}

#[test]
fn a_colour_is_written_as_fractions_so_a_dark_one_is_not_read_as_0_to_1() {
    let hex = spec(Form::Hex, &[0]);
    assert_eq!(
        write(hex, vec![0.0; 3], &[1.0, 0.0, 0.0]),
        format!("{}, 0, 0", 1.0_f32 / 255.0)
    );
    assert_eq!(
        read(Form::Hex, 0, &[13.0, 71.0, 179.0, 9.0]),
        Some(vec![13.0, 71.0, 179.0])
    );
}

#[test]
fn hex_reads_back_as_six_digits_and_parses_with_or_without_its_hash() {
    assert_eq!(show(Form::Hex, &[13.0, 71.0, 179.0]), "0D47B3");
    assert_eq!(parse(Form::Hex, "#0D47B3"), Some(vec![13.0, 71.0, 179.0]));
    assert_eq!(parse(Form::Hex, "0d47b3"), Some(vec![13.0, 71.0, 179.0]));
    assert_eq!(parse(Form::Hex, "fff"), Some(vec![255.0; 3]));
    assert_eq!(parse(Form::Hex, "12345"), None);
}

#[test]
fn numbers_read_back_without_noise_and_parse_past_their_units() {
    assert_eq!(show(NUMBER, &[12.0]), "12");
    assert_eq!(show(NUMBER, &[0.12345]), "0.1235");
    assert_eq!(show(NUMBER, &[-0.00001]), "0");
    assert_eq!(parse(NUMBER, " 45° "), Some(vec![45.0]));
    assert_eq!(parse(Form::Percent, "80%"), Some(vec![80.0]));
    assert_eq!(parse(NUMBER, "12px"), Some(vec![12.0]));
    assert_eq!(parse(NUMBER, "Mixed"), None);
}

#[test]
fn a_value_s_numbers_are_the_ones_the_properties_panel_spells() {
    let udim2 = Variant::UDim2(rbx_dom::UDim2 {
        x: rbx_dom::UDim {
            scale: 0.5,
            offset: -4,
        },
        y: rbx_dom::UDim {
            scale: 0.0,
            offset: 12,
        },
    });
    assert_eq!(numbers_of(&udim2), Some(vec![0.5, -4.0, 0.0, 12.0]));
    assert_eq!(numbers_of(&Variant::Bool(true)), None);
}
