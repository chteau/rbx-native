use gpui_kit::px;

use super::emoji::{character, twemoji, CATEGORIES, EMOJI};
use super::rows::{letter_rows, number_rows, symbol_rows};
use super::{KeyboardLayout, Metrics, Placement, Style};

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

#[test]
fn emoji_names_are_their_characters() {
    assert_eq!(character("1f602"), Some('\u{1f602}'));
    assert_eq!(character("2764"), Some('\u{2764}'));
    assert_eq!(character("zz"), None);
}

#[test]
fn every_style_fits_its_keys_in_the_catalog_heights() {
    // (style, screen width, keyboard height) from the device catalog.
    let cases = [
        (Style::Iphone, 393.0, 336),
        (Style::Iphone, 852.0, 209),
        (Style::Ipad, 820.0, 313),
        (Style::Ipad, 1180.0, 398),
        (Style::Android, 412.0, 300),
        (Style::Android, 915.0, 230),
    ];
    for (style, width, points) in cases {
        let place = Placement {
            left: px(0.),
            top: px(0.),
            width: px(width),
            height: px(points as f32),
            points,
        };
        let m = Metrics::new(&place, style);
        assert!(m.key_h >= 30.0, "{style:?} {points}: keys {}", m.key_h);
        assert!(m.strip >= 0.0, "{style:?} {points}: strip {}", m.strip);
        let total = m.suggest + m.rows_height() + m.strip;
        assert!(
            (total - points as f32).abs() < 0.01,
            "{style:?} {points}: {total}"
        );
    }
}
