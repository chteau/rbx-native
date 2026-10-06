use super::{ring_color, tag_color, HOVER_ALPHA, SELECTED_ALPHA};
use crate::accent::contrast;
use crate::tokens;

/// A tagged Folder's row is filled with whatever colour the user picked, so
/// the keyboard ring is checked against a sweep of the whole RGB cube, at
/// both of the fills a row can show (hover and selected), for every accent.
#[test]
fn the_focus_ring_clears_three_to_one_over_any_tag_colour() {
    let steps = (0..=255u8).step_by(15);
    for (accent_name, accent) in crate::accent::PRESETS {
        let accent = gpui_kit::rgb(accent);
        for r in steps.clone() {
            for g in steps.clone() {
                for b in steps.clone() {
                    for alpha in [HOVER_ALPHA, SELECTED_ALPHA] {
                        let fill = tag_color((r, g, b), alpha);
                        let ring = ring_color(accent, fill);
                        let behind = tokens::composite(fill, tokens::dock());
                        let ratio = contrast(ring, behind);
                        assert!(
                            ratio >= 3.,
                            "{accent_name}: ring over tag {r},{g},{b} at alpha {alpha} is {ratio:.2}:1"
                        );
                    }
                }
            }
        }
    }
}

/// The accent stays the ring wherever it is legible — an untagged selected
/// row, a brick-red tag — and gives way where it is not: on a pale grey tag
/// it measures under 2:1, so the ring goes white.
#[test]
fn the_ring_keeps_the_accent_only_where_it_is_legible() {
    let accent = tokens::check_on();
    assert_eq!(ring_color(accent, tokens::accent_soft()), accent);
    assert_eq!(
        ring_color(accent, tag_color((200, 60, 40), SELECTED_ALPHA)),
        accent
    );
    assert_eq!(
        ring_color(accent, tag_color((235, 235, 235), SELECTED_ALPHA)),
        gpui_kit::rgb(0xFFFFFF)
    );
}
