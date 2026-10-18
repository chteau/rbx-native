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
//!
//! Density is the panel's physical pixels per inch and pixels per point/dp,
//! from the same spec sheets; it drives Physical size display scaling only.
//!
//! Keyboard heights are approximations: neither Apple nor Google publishes
//! the system keyboard's height, which also changes with the keyboard app,
//! its suggestion strip and the language. They are typical heights of the
//! stock keyboard with its suggestion strip and the home indicator or
//! navigation bar below it, rounded, and good for checking that controls
//! clear a keyboard-sized cover — not for pixel work. Studio's own default
//! is 0, no cover.

use super::{Density, Device, Insets, Os, Platform, Tier};

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

const fn density(ppi: u32, scale: f32) -> Option<Density> {
    Some(Density { ppi, scale })
}

/// A phone or tablet: rotates, has a density and an on-screen keyboard.
#[allow(clippy::too_many_arguments)]
const fn touch(
    name: &'static str,
    platform: Platform,
    tier: Option<Tier>,
    os: Os,
    size: (u32, u32),
    (portrait_insets, landscape_insets): (Insets, Insets),
    density: Option<Density>,
    keyboard: (u32, u32),
) -> Device {
    Device {
        name,
        platform,
        tier,
        os,
        size,
        portrait_insets,
        landscape_insets,
        density,
        keyboard,
    }
}

const fn plain(name: &'static str, platform: Platform, size: (u32, u32)) -> Device {
    Device {
        name,
        platform,
        tier: None,
        os: Os::Other,
        size,
        portrait_insets: NONE,
        landscape_insets: NONE,
        density: None,
        keyboard: (0, 0),
    }
}

const PHONE: Platform = Platform::Phone;
const TABLET: Platform = Platform::Tablet;
/// Face ID iPhones: the Dynamic Island's 59 (47 on the iPhone 13's notch)
/// and the home indicator's 34 portrait / 21 landscape.
const ISLAND: (Insets, Insets) = (insets(0, 59, 0, 34), insets(59, 0, 59, 21));
const NOTCH: (Insets, Insets) = (insets(0, 47, 0, 34), insets(47, 0, 47, 21));
/// The bottom 20 is the home indicator on Face ID iPads.
const IPAD: (Insets, Insets) = (insets(0, 0, 0, 20), insets(0, 0, 0, 20));
const FLAT: (Insets, Insets) = (NONE, NONE);
/// iOS keyboard with its suggestion bar and the home indicator below it.
const IOS_KEYBOARD: (u32, u32) = (336, 209);
/// Gboard / Samsung Keyboard with the suggestion strip and the
/// navigation bar.
const ANDROID_KEYBOARD: (u32, u32) = (300, 230);

pub(super) const DEVICES: &[Device] = &[
    touch(
        "iPhone 15 Pro",
        PHONE,
        Some(Tier::Flagship),
        Os::Ios,
        (393, 852),
        ISLAND,
        density(460, 3.0),
        IOS_KEYBOARD,
    ),
    touch(
        "iPhone 15 Pro Max",
        PHONE,
        Some(Tier::Flagship),
        Os::Ios,
        (430, 932),
        ISLAND,
        density(460, 3.0),
        (346, 209),
    ),
    // 1080×2400 px at 2.625.
    touch(
        "Pixel 8",
        PHONE,
        Some(Tier::Flagship),
        Os::Android,
        (412, 915),
        FLAT,
        density(428, 2.625),
        ANDROID_KEYBOARD,
    ),
    // 1080×2340 px at 3.0.
    touch(
        "Galaxy S24",
        PHONE,
        Some(Tier::Flagship),
        Os::Android,
        (360, 780),
        FLAT,
        density(416, 3.0),
        ANDROID_KEYBOARD,
    ),
    touch(
        "iPhone 13",
        PHONE,
        Some(Tier::MidRange),
        Os::Ios,
        (390, 844),
        NOTCH,
        density(460, 3.0),
        IOS_KEYBOARD,
    ),
    // 1080×2400 px at 2.625.
    touch(
        "Pixel 7a",
        PHONE,
        Some(Tier::MidRange),
        Os::Android,
        (412, 915),
        FLAT,
        density(429, 2.625),
        ANDROID_KEYBOARD,
    ),
    // Touch ID: no home indicator under the keyboard.
    touch(
        "iPhone SE (3rd gen)",
        PHONE,
        Some(Tier::Entry),
        Os::Ios,
        (375, 667),
        FLAT,
        density(326, 2.0),
        (260, 200),
    ),
    // 720×1600 px at 2.0.
    touch(
        "Galaxy A13 5G",
        PHONE,
        Some(Tier::Entry),
        Os::Android,
        (360, 800),
        FLAT,
        density(270, 2.0),
        ANDROID_KEYBOARD,
    ),
    // iPad keyboards are taller in landscape, where the keys are wider.
    touch(
        "iPad (10th gen)",
        TABLET,
        None,
        Os::Ios,
        (820, 1180),
        IPAD,
        density(264, 2.0),
        (313, 398),
    ),
    touch(
        "iPad Pro 12.9\"",
        TABLET,
        None,
        Os::Ios,
        (1024, 1366),
        IPAD,
        density(264, 2.0),
        (328, 423),
    ),
    // 2560×1600 px at 2.0.
    touch(
        "Galaxy Tab S9",
        TABLET,
        None,
        Os::Android,
        (800, 1280),
        FLAT,
        density(274, 2.0),
        (340, 380),
    ),
    plain("Desktop 1080p", Platform::Desktop, (1920, 1080)),
    plain("Laptop 1366×768", Platform::Desktop, (1366, 768)),
    plain("Desktop 1440p", Platform::Desktop, (2560, 1440)),
    // Consoles render a 1920×1080 UI.
    plain("Xbox Series X", Platform::Console, (1920, 1080)),
    plain("PlayStation 5", Platform::Console, (1920, 1080)),
    // Roblox draws ScreenGuis on a virtual panel whose size it does not
    // document: 1920×1080 is a stand-in, and Play cannot simulate it.
    plain("Meta Quest 3", Platform::Headset, (1920, 1080)),
];
