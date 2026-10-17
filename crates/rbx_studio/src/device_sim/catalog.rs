//! The built-in device list, in menu order.
//!
//! Sizes are logical points (iOS) or dp (Android), which is what Roblox
//! reports as `Camera.ViewportSize`. Sources: Apple "Human Interface
//! Guidelines › Layout › Specifications" for iOS point sizes; Android
//! "Support different screen sizes" (dp = px / (dpi / 160)) with the
//! manufacturers' spec sheets for pixel size and density.
//!
//! iOS insets are UIKit `safeAreaInsets` in a full-screen app with the status
//! bar hidden, as Apple's simulators report them. Android insets are zero:
//! the display-cutout inset depends on the app's cutout mode, and Roblox does
//! not document which one it uses. Desktop and console have none.

use super::{Device, Insets, Platform, Tier};

const NONE: Insets = Insets {
    left: 0,
    top: 0,
    right: 0,
    bottom: 0,
};

const fn insets(left: u32, top: u32, right: u32, bottom: u32) -> Insets {
    Insets {
        left,
        top,
        right,
        bottom,
    }
}

const fn phone(
    name: &'static str,
    tier: Tier,
    size: (u32, u32),
    portrait_insets: Insets,
    landscape_insets: Insets,
) -> Device {
    Device {
        name,
        platform: Platform::Phone,
        tier: Some(tier),
        size,
        portrait_insets,
        landscape_insets,
    }
}

const fn plain(name: &'static str, platform: Platform, size: (u32, u32), i: Insets) -> Device {
    Device {
        name,
        platform,
        tier: None,
        size,
        portrait_insets: i,
        landscape_insets: i,
    }
}

pub(super) const DEVICES: &[Device] = &[
    phone(
        "iPhone 15 Pro",
        Tier::Flagship,
        (393, 852),
        insets(0, 59, 0, 34),
        insets(59, 0, 59, 21),
    ),
    phone(
        "iPhone 15 Pro Max",
        Tier::Flagship,
        (430, 932),
        insets(0, 59, 0, 34),
        insets(59, 0, 59, 21),
    ),
    // 1080×2400 px at 2.625.
    phone("Pixel 8", Tier::Flagship, (412, 915), NONE, NONE),
    // 1080×2340 px at 3.0.
    phone("Galaxy S24", Tier::Flagship, (360, 780), NONE, NONE),
    phone(
        "iPhone 13",
        Tier::MidRange,
        (390, 844),
        insets(0, 47, 0, 34),
        insets(47, 0, 47, 21),
    ),
    // 1080×2400 px at 2.625.
    phone("Pixel 7a", Tier::MidRange, (412, 915), NONE, NONE),
    phone("iPhone SE (3rd gen)", Tier::Entry, (375, 667), NONE, NONE),
    // 720×1600 px at 2.0.
    phone("Galaxy A13 5G", Tier::Entry, (360, 800), NONE, NONE),
    // The bottom 20 is the home indicator on Face ID iPads.
    plain(
        "iPad (10th gen)",
        Platform::Tablet,
        (820, 1180),
        insets(0, 0, 0, 20),
    ),
    plain(
        "iPad Pro 12.9\"",
        Platform::Tablet,
        (1024, 1366),
        insets(0, 0, 0, 20),
    ),
    // 2560×1600 px at 2.0.
    plain("Galaxy Tab S9", Platform::Tablet, (800, 1280), NONE),
    plain("Desktop 1080p", Platform::Desktop, (1920, 1080), NONE),
    plain("Laptop 1366×768", Platform::Desktop, (1366, 768), NONE),
    plain("Desktop 1440p", Platform::Desktop, (2560, 1440), NONE),
    // Consoles render a 1920×1080 UI.
    plain("Xbox Series X", Platform::Console, (1920, 1080), NONE),
    plain("PlayStation 5", Platform::Console, (1920, 1080), NONE),
    // Roblox draws ScreenGuis on a virtual panel whose size it does not
    // document: 1920×1080 is a stand-in, and Play cannot simulate it.
    plain("Meta Quest 3", Platform::Headset, (1920, 1080), NONE),
];
