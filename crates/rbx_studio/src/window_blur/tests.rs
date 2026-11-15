use std::time::{Duration, Instant};

use super::capture::{over, stack_below, visible_below, Candidate};
use super::wallpaper::{canvas, parse_color, placement, unquote, Fit, Rect, Settings};
use super::worker::{Throttle, MIN_INTERVAL};
use super::x11::classify;
use super::BlurSupport;

fn names(list: &[&str]) -> Vec<String> {
    list.iter().map(|name| name.to_string()).collect()
}

#[test]
fn kwin_and_picom_blur_themselves() {
    assert_eq!(
        classify("KWin", Some(&names(&["KWin"]))),
        BlurSupport::Native
    );
    assert_eq!(
        classify("i3", Some(&names(&["picom", "picom picom"]))),
        BlurSupport::Native
    );
    assert_eq!(
        classify("Openbox", Some(&names(&["compton"]))),
        BlurSupport::Native
    );
}

#[test]
fn gnome_shell_is_drawn_by_the_editor() {
    assert_eq!(
        classify("GNOME Shell", Some(&names(&["mutter"]))),
        BlurSupport::Drawn
    );
    assert_eq!(classify("Mutter", Some(&[])), BlurSupport::Drawn);
}

#[test]
fn no_compositor_or_an_unknown_one_blurs_nothing() {
    assert_eq!(classify("GNOME Shell", None), BlurSupport::None);
    assert_eq!(
        classify("xfwm4", Some(&names(&["xfwm4"]))),
        BlurSupport::None
    );
}

#[test]
fn gsettings_values_unquote_and_parse() {
    assert_eq!(
        unquote("'file:///usr/share/a b.jpg'"),
        "file:///usr/share/a b.jpg"
    );
    assert_eq!(unquote("'it\\'s'"), "it's");
    assert_eq!(parse_color("#023c88"), Some([0x02, 0x3c, 0x88]));
    assert_eq!(parse_color("023c88"), None);
    assert_eq!(Fit::parse("spanned"), Fit::Spanned);
    assert_eq!(Fit::parse("something new"), Fit::Zoom);
}

const MONITOR: Rect = Rect {
    x: 1920,
    y: 0,
    w: 1920,
    h: 1080,
};

#[test]
fn zoom_covers_the_monitor_and_crops() {
    let at = placement(Fit::Zoom, (1000, 1000), MONITOR).unwrap();
    assert_eq!(
        at,
        Rect {
            x: 1920,
            y: -420,
            w: 1920,
            h: 1920
        }
    );
}

#[test]
fn scaled_fits_inside_with_bars() {
    let at = placement(Fit::Scaled, (1000, 1000), MONITOR).unwrap();
    assert_eq!(
        at,
        Rect {
            x: 2340,
            y: 0,
            w: 1080,
            h: 1080
        }
    );
}

#[test]
fn centered_keeps_its_size_and_stretched_fills() {
    let centered = placement(Fit::Centered, (200, 100), MONITOR).unwrap();
    assert_eq!(
        centered,
        Rect {
            x: 2780,
            y: 490,
            w: 200,
            h: 100
        }
    );
    assert_eq!(
        placement(Fit::Stretched, (200, 100), MONITOR),
        Some(MONITOR)
    );
    assert_eq!(placement(Fit::None, (200, 100), MONITOR), None);
}

#[test]
fn the_canvas_places_the_image_per_monitor() {
    let settings = Settings {
        image: None,
        fit: Fit::Scaled,
        color: [10, 20, 30],
    };
    // A red square, BGRA.
    let image = image::RgbaImage::from_pixel(100, 100, image::Rgba([0, 0, 255, 255]));
    let monitors = [
        Rect {
            x: 0,
            y: 0,
            w: 400,
            h: 200,
        },
        Rect {
            x: 400,
            y: 0,
            w: 400,
            h: 200,
        },
    ];
    let out = canvas(&settings, Some(&image), (800, 200), &monitors, 4);
    assert_eq!(out.dimensions(), (200, 50));
    // Each monitor's middle is the image, its left edge the primary colour.
    assert_eq!(out.get_pixel(50, 25).0, [0, 0, 255, 255]);
    assert_eq!(out.get_pixel(150, 25).0, [0, 0, 255, 255]);
    assert_eq!(out.get_pixel(2, 25).0, [30, 20, 10, 255]);
    assert_eq!(out.get_pixel(102, 25).0, [30, 20, 10, 255]);
}

#[test]
fn premultiplied_pixels_go_over_the_ground() {
    let mut under = [100, 100, 100, 255, 100, 100, 100, 255, 100, 100, 100, 255];
    // Opaque, fully transparent, and half-covered premultiplied white.
    let top = [10, 20, 30, 255, 0, 0, 0, 0, 128, 128, 128, 128];
    over(&mut under, &top);
    assert_eq!(&under[..4], &[10, 20, 30, 255]);
    assert_eq!(&under[4..8], &[100, 100, 100, 255]);
    assert_eq!(&under[8..], &[178, 178, 178, 255]);
}

#[test]
fn the_throttle_waits_for_a_change_and_the_interval() {
    let start = Instant::now();
    let mut throttle = Throttle::default();
    assert!(!throttle.due(start, false));
    assert!(throttle.due(start, true));
    assert!(!throttle.due(start + MIN_INTERVAL / 2, true));
    assert!(throttle.due(start + MIN_INTERVAL, true));
    assert!(!throttle.due(start + MIN_INTERVAL * 3, false));
    assert!(throttle.due(start + MIN_INTERVAL * 3 + Duration::from_millis(1), true));
}

#[test]
fn rects_meet_only_when_they_overlap() {
    let a = Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 10,
    };
    assert!(a.intersects(&Rect {
        x: 9,
        y: 9,
        w: 5,
        h: 5
    }));
    assert!(!a.intersects(&Rect {
        x: 10,
        y: 0,
        w: 5,
        h: 5
    }));
    assert!(!a.intersects(&Rect {
        x: -100,
        y: -100,
        w: 1,
        h: 1
    }));
}

#[test]
fn only_shown_windows_under_ours_and_over_its_rectangle_count() {
    let children = [1, 2, 3, 4, 5, 6];
    assert_eq!(stack_below(&children, 4), Some(&children[..3]));
    assert_eq!(stack_below(&children, 9), None);

    let ours = Rect {
        x: 100,
        y: 100,
        w: 200,
        h: 200,
    };
    let at = |x, y| Rect {
        x,
        y,
        w: 100,
        h: 100,
    };
    let candidate = |window, area, shown| Candidate {
        window,
        area,
        shown,
        format: 0,
    };
    let stack = vec![
        candidate(1, at(150, 150), true),
        candidate(2, at(150, 150), false), // unmapped or input-only
        candidate(3, at(-100, -100), true), // off to the side
        candidate(5, at(250, 250), true),
    ];
    let below: Vec<u32> = visible_below(stack, ours)
        .iter()
        .map(|c| c.window)
        .collect();
    assert_eq!(below, [1, 5]);
}
