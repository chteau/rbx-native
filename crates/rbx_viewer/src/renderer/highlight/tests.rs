use super::*;

#[test]
fn the_first_highlight_is_drawn_last_so_it_wins_a_shared_pixel() {
    let claims = [
        (1, DepthMode::AlwaysOnTop),
        (2, DepthMode::AlwaysOnTop),
        (1, DepthMode::AlwaysOnTop),
        (3, DepthMode::Occluded),
    ];
    assert_eq!(
        draw_order(claims.into_iter()),
        vec![
            (2, DepthMode::AlwaysOnTop),
            (1, DepthMode::AlwaysOnTop),
            (3, DepthMode::Occluded),
        ]
    );
}
