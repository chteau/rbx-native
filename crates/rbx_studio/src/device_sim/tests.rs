use std::collections::HashSet;

use serde_json::json;

use super::*;

fn named(name: &str) -> &'static Device {
    devices().iter().find(|d| d.name == name).unwrap()
}

#[test]
fn every_device_leaves_a_usable_area() {
    for d in devices() {
        for o in [Orientation::Portrait, Orientation::Landscape] {
            let (w, h) = d.screen(o);
            let i = d.insets(o);
            assert!(w > 0 && h > 0, "{}", d.name);
            assert!(i.left + i.right < w, "{} {o:?}", d.name);
            assert!(i.top + i.bottom < h, "{} {o:?}", d.name);
        }
    }
}

#[test]
fn names_are_unique_and_only_phones_have_tiers() {
    let names: HashSet<_> = devices().iter().map(|d| d.name).collect();
    assert_eq!(names.len(), devices().len());
    for d in devices() {
        assert_eq!(
            d.tier.is_some(),
            d.platform == Platform::Phone,
            "{}",
            d.name
        );
    }
}

#[test]
fn orientation_swaps_only_rotating_platforms() {
    let phone = named("iPhone 15 Pro");
    assert_eq!(phone.screen(Orientation::Landscape), (852, 393));
    assert_eq!(phone.screen(Orientation::Portrait), (393, 852));
    let desktop = named("Desktop 1080p");
    assert_eq!(
        desktop.screen(Orientation::Portrait),
        desktop.screen(Orientation::Landscape)
    );
    assert_eq!(
        Orientation::Portrait.rotated().rotated(),
        Orientation::Portrait
    );
}

#[test]
fn iphone_landscape_insets() {
    let i = named("iPhone 15 Pro").insets(Orientation::Landscape);
    assert_eq!((i.left, i.top, i.right, i.bottom), (59, 0, 59, 21));
}

#[test]
fn centred_fits_or_refuses() {
    let panel = Rect {
        x: 10,
        y: 20,
        width: 1000,
        height: 800,
    };
    assert_eq!(
        centred(panel, (852, 393)),
        Some(Rect {
            x: 84,
            y: 223,
            width: 852,
            height: 393
        })
    );
    assert_eq!(centred(panel, (1920, 1080)), None);
    assert_eq!(centred(panel, (1000, 800)), Some(panel));
}

#[test]
fn sober_keys_per_platform() {
    let keys = |t: &str, c: bool| {
        vec![
            ("touch_mode".to_string(), json!(t)),
            ("use_console_experience".to_string(), json!(c)),
        ]
    };
    assert_eq!(sober_keys(Platform::Phone), keys("on", false));
    assert_eq!(sober_keys(Platform::Tablet), keys("on", false));
    assert_eq!(sober_keys(Platform::Console), keys("off", true));
    assert_eq!(sober_keys(Platform::Desktop), keys("off", false));
    assert_eq!(sober_keys(Platform::Headset), keys("off", false));
}

#[test]
fn restart_truth_table() {
    assert!(!needs_restart(
        Some(Platform::Phone),
        Some(Platform::Tablet)
    ));
    assert!(!needs_restart(None, None));
    assert!(needs_restart(None, Some(Platform::Desktop)));
    assert!(needs_restart(Some(Platform::Desktop), None));
    assert!(needs_restart(
        Some(Platform::Desktop),
        Some(Platform::Console)
    ));
    assert!(needs_restart(
        Some(Platform::Phone),
        Some(Platform::Desktop)
    ));
}

#[test]
fn custom_device_clamps_and_round_trips() {
    let c = CustomDevice::new(0, 99999, Platform::Tablet);
    assert_eq!((c.width, c.height), (1, 4320));
    let c = CustomDevice::new(800, 600, Platform::Console);
    let value = json!({ "device_custom": c.json() });
    assert_eq!(CustomDevice::read(&value), c);
    assert_eq!(CustomDevice::read(&json!({})), CustomDevice::default());
    let odd = json!({ "device_custom": { "platform": "toaster" } });
    assert_eq!(CustomDevice::read(&odd).platform, Platform::Desktop);
    assert_eq!(c.device().name, "Custom");
}

#[test]
fn labels() {
    assert_eq!(
        named("iPhone 15 Pro").label(Orientation::Landscape),
        "iPhone 15 Pro · 852×393 Landscape"
    );
    assert_eq!(
        named("Desktop 1080p").label(Orientation::Landscape),
        "Desktop 1080p · 1920×1080"
    );
}
