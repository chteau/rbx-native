use super::{letter_rows, number_rows, symbol_rows, twemoji, KeyboardLayout, CATEGORIES, EMOJI};

#[test]
fn every_listed_emoji_ships_and_rasterizes() {
    for name in EMOJI.iter().chain(CATEGORIES) {
        assert!(twemoji(name).is_some(), "assets/twemoji/{name}.svg");
    }
}

#[test]
fn layouts_fill_their_rows() {
    for layout in KeyboardLayout::ALL {
        let [first, second, third] = letter_rows(layout);
        assert_eq!(first.len(), 10);
        assert!(second.len() >= 9);
        assert_eq!(third.len(), 7);
        for rows in [number_rows(layout), symbol_rows(layout)] {
            assert_eq!(rows[0].len(), 10);
            assert_eq!(rows[1].len(), 10);
            assert_eq!(rows[2].len(), 5);
        }
    }
    assert_eq!(letter_rows(KeyboardLayout::Azerty)[0][..2], ["a", "z"]);
}
