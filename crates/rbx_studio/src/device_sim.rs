//! Device Simulator data: the device catalog, orientation and insets, the
//! Sober input-mode keys, the Play fit math and the persisted custom device.

mod catalog;

use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Platform {
    Phone,
    Tablet,
    Desktop,
    Console,
    Headset,
}

impl Platform {
    pub(crate) const ALL: [Platform; 5] = [
        Platform::Phone,
        Platform::Tablet,
        Platform::Desktop,
        Platform::Console,
        Platform::Headset,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Platform::Phone => "Phone",
            Platform::Tablet => "Tablet",
            Platform::Desktop => "Desktop",
            Platform::Console => "Console",
            Platform::Headset => "Headset",
        }
    }

    pub(crate) fn rotates(self) -> bool {
        matches!(self, Platform::Phone | Platform::Tablet)
    }

    /// Sober has no VR, so a headset can be previewed but never played.
    pub(crate) fn plays(self) -> bool {
        self != Platform::Headset
    }
}

/// Phones only; groups the device menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Tier {
    Flagship,
    MidRange,
    Entry,
}

impl Tier {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Tier::Flagship => "Flagship",
            Tier::MidRange => "Mid-range",
            Tier::Entry => "Entry level",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Orientation {
    Portrait,
    Landscape,
}

impl Orientation {
    pub(crate) fn rotated(self) -> Self {
        match self {
            Orientation::Portrait => Orientation::Landscape,
            Orientation::Landscape => Orientation::Portrait,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Orientation::Portrait => "Portrait",
            Orientation::Landscape => "Landscape",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) struct Insets {
    pub(crate) left: u32,
    pub(crate) top: u32,
    pub(crate) right: u32,
    pub(crate) bottom: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Device {
    pub(crate) name: &'static str,
    pub(crate) platform: Platform,
    pub(crate) tier: Option<Tier>,
    /// Logical size in the device's natural orientation (portrait for
    /// phones/tablets, landscape for the rest), in points/dp — what Roblox
    /// reports as `Camera.ViewportSize`.
    pub(crate) size: (u32, u32),
    pub(crate) portrait_insets: Insets,
    pub(crate) landscape_insets: Insets,
}

impl Device {
    /// Roblox experiences are overwhelmingly landscape, and Studio opens
    /// phones landscape too.
    pub(crate) fn default_orientation(&self) -> Orientation {
        Orientation::Landscape
    }

    pub(crate) fn screen(&self, o: Orientation) -> (u32, u32) {
        if !self.platform.rotates() {
            return self.size;
        }
        let (w, h) = self.size;
        let (short, long) = (w.min(h), w.max(h));
        match o {
            Orientation::Portrait => (short, long),
            Orientation::Landscape => (long, short),
        }
    }

    pub(crate) fn insets(&self, o: Orientation) -> Insets {
        if !self.platform.rotates() {
            return self.landscape_insets;
        }
        match o {
            Orientation::Portrait => self.portrait_insets,
            Orientation::Landscape => self.landscape_insets,
        }
    }

    pub(crate) fn label(&self, o: Orientation) -> String {
        let (w, h) = self.screen(o);
        if self.platform.rotates() {
            format!("{} · {}×{} {}", self.name, w, h, o.label())
        } else {
            format!("{} · {}×{}", self.name, w, h)
        }
    }
}

pub(crate) fn devices() -> &'static [Device] {
    catalog::DEVICES
}

/// Sober's `touch_mode: "on"` is a touchscreen, and experiences then use the
/// mobile UI; `use_console_experience` selects the console UI (games may
/// register the player as a console). Both are read at launch only.
pub(crate) fn sober_keys(platform: Platform) -> Vec<(String, Value)> {
    let (touch, console) = match platform {
        Platform::Phone | Platform::Tablet => ("on", false),
        Platform::Console => ("off", true),
        Platform::Desktop | Platform::Headset => ("off", false),
    };
    vec![
        ("touch_mode".to_string(), json!(touch)),
        ("use_console_experience".to_string(), json!(console)),
    ]
}

/// `None` is no simulation, i.e. the user's own Sober config.
pub(crate) fn needs_restart(launched: Option<Platform>, now: Option<Platform>) -> bool {
    match (launched, now) {
        (None, None) => false,
        (Some(a), Some(b)) => sober_keys(a) != sober_keys(b),
        _ => true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Rect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// The screen at 1:1 centred in `panel`. The Play client is a foreign
/// window, so it can only be sized, never scaled; a device larger than the
/// 3D view is refused rather than shrunk.
pub(crate) fn centred(panel: Rect, screen: (u32, u32)) -> Option<Rect> {
    let (w, h) = screen;
    if w > panel.width || h > panel.height {
        return None;
    }
    Some(Rect {
        x: panel.x + ((panel.width - w) / 2) as i32,
        y: panel.y + ((panel.height - h) / 2) as i32,
        width: w,
        height: h,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CustomDevice {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) platform: Platform,
}

impl Default for CustomDevice {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 720,
            platform: Platform::Desktop,
        }
    }
}

impl CustomDevice {
    /// Studio's Custom device limits (creator-docs studio/device-simulator.md).
    pub(crate) const WIDTH_RANGE: (u32, u32) = (1, 7680);
    pub(crate) const HEIGHT_RANGE: (u32, u32) = (1, 4320);

    pub(crate) fn new(width: u32, height: u32, platform: Platform) -> Self {
        Self {
            width: width.clamp(Self::WIDTH_RANGE.0, Self::WIDTH_RANGE.1),
            height: height.clamp(Self::HEIGHT_RANGE.0, Self::HEIGHT_RANGE.1),
            platform,
        }
    }

    pub(crate) fn device(&self) -> Device {
        Device {
            name: "Custom",
            platform: self.platform,
            tier: None,
            size: (self.width, self.height),
            portrait_insets: Insets::default(),
            landscape_insets: Insets::default(),
        }
    }

    pub(crate) fn read(value: &Value) -> Self {
        let default = Self::default();
        let custom = &value["device_custom"];
        let dimension = |key: &str, fallback: u32| {
            custom[key]
                .as_u64()
                .map_or(fallback, |n| n.min(u64::from(u32::MAX)) as u32)
        };
        let platform = match custom["platform"].as_str() {
            Some("phone") => Platform::Phone,
            Some("tablet") => Platform::Tablet,
            Some("console") => Platform::Console,
            Some("headset") => Platform::Headset,
            _ => Platform::Desktop,
        };
        Self::new(
            dimension("width", default.width),
            dimension("height", default.height),
            platform,
        )
    }

    pub(crate) fn json(&self) -> Value {
        json!({
            "width": self.width,
            "height": self.height,
            "platform": self.platform.label().to_lowercase(),
        })
    }
}

#[cfg(test)]
mod tests;
