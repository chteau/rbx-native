//! `ParticleEmitter2D` (Studio's "UI Particles" beta): particles a
//! `GuiObject` emits in GUI pixels, read into [`GuiEmitter`] and simulated
//! by [`FlatSim`] on the same RNG and sequences as the 3D
//! [`super::Simulation`].
//!
//! Every rule here was measured in Studio 0.748 with the beta on:
//!
//! - The emission area is centred on the parent, moved by `EmissionOffset`
//!   times the parent's size, and is the parent's size times
//!   `EmissionSpread`, plus one whole parent in `BoundingBox` mode. So a
//!   `Point` emitter with no spread is the centre, a `BoundingBox` one with
//!   no spread fills the parent.
//! - `EmissionAngle` 0 is up and grows clockwise; `SpreadAngle` is degrees
//!   either side of it. `Speed`, `Acceleration` and `Size` are GUI pixels
//!   (`Size` the half side), all scaled by the parent's `UIScale`s, and `+Y`
//!   acceleration is down the screen.
//! - The area, the angle and each particle's `Rotation` turn with the
//!   parent's `AbsoluteRotation`. `RotSpeed` is degrees per second clockwise.
//! - `Squash` `s` draws a particle `2^-s` as wide and `2^s` as tall.
//! - `Drag` halves the speed `Drag` times a second: Drag 2 goes half as far
//!   as Drag 1, Drag 0.5 twice as far.
//! - `LockedToParent` particles ride along when the parent moves; the rest
//!   stay where they were born.
//! - `TimeScale` (clamped to `[0, 1]`) runs the whole effect slower, `0`
//!   freezes it.
//! - `Emit` works whatever `Enabled` says, and no `TextureContent` draws
//!   nothing.

mod sim;

use std::sync::Arc;

use rbx_assets::AssetRef;
use rbx_dom::{ColorSequence, Content, NumberSequence, Ref, Variant};

use crate::scene::props::{
    bool_or, color_sequence_or, float_or, int_or, number_range_or, number_sequence_or, vector2_or,
    Properties,
};
use crate::textures::asset_uri;

pub(crate) use sim::{FlatParticle, FlatSim};

pub(crate) const CLASS: &str = "ParticleEmitter2D";
const DEFAULT_TEXTURE: &str = "rbxasset://textures/particles/sparkles_main.dds";

/// `ParticleFlipbookMode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FlipbookMode {
    Loop,
    OneShot,
    PingPong,
    Random,
}

/// How a particle's texture is cut into frames and played.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Flipbook {
    pub(crate) columns: u32,
    pub(crate) rows: u32,
    pub(crate) mode: FlipbookMode,
    /// Frames per second, drawn per particle from the range.
    pub(crate) framerate: (f32, f32),
    pub(crate) start_random: bool,
    pub(crate) blend_frames: bool,
}

impl Flipbook {
    pub(crate) fn frames(&self) -> u32 {
        self.columns * self.rows
    }
}

/// One `ParticleEmitter2D` as read from the place.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GuiEmitter {
    pub(crate) referent: Ref,
    pub(crate) enabled: bool,
    pub(crate) rate: f32,
    pub(crate) lifetime: (f32, f32),
    pub(crate) speed: (f32, f32),
    /// Degrees, 0 up, clockwise.
    pub(crate) angle: f32,
    /// Degrees either side of [`GuiEmitter::angle`].
    pub(crate) spread_angle: f32,
    /// `EmissionMode.BoundingBox` rather than `Point`.
    pub(crate) bounding_box: bool,
    /// `EmissionOffset` and `EmissionSpread`, fractions of the parent's size.
    pub(crate) offset: [f32; 2],
    pub(crate) spread: [f32; 2],
    pub(crate) acceleration: [f32; 2],
    pub(crate) drag: f32,
    pub(crate) size: NumberSequence,
    pub(crate) squash: NumberSequence,
    pub(crate) transparency: NumberSequence,
    pub(crate) color: ColorSequence,
    /// Multiplies the drawn colour, in the display's encoding.
    pub(crate) brightness: f32,
    /// 0 is normal blending, 1 is additive; measured as a straight blend
    /// between the two.
    pub(crate) blend: f32,
    pub(crate) rotation: (f32, f32),
    pub(crate) rot_speed: (f32, f32),
    pub(crate) locked: bool,
    pub(crate) time_scale: f32,
    /// `None` for an emptied `TextureContent`, which draws nothing.
    pub(crate) texture: Option<AssetRef>,
    pub(crate) flipbook: Flipbook,
}

/// Reads one `ParticleEmitter2D`. Defaults are Studio's for a fresh one.
pub(crate) fn read(properties: &Properties, referent: Ref) -> Arc<GuiEmitter> {
    let pair = |key, default: (f32, f32)| {
        let (x, y) = vector2_or(properties, key, default);
        [x, y]
    };
    let layout = ordinal(properties, "FlipbookLayout", 0);
    let (columns, rows) = match layout {
        1 => (2, 2),
        2 => (4, 4),
        3 => (8, 8),
        4 => (
            int_or(properties, "FlipbookSizeX", 1).max(1) as u32,
            int_or(properties, "FlipbookSizeY", 1).max(1) as u32,
        ),
        _ => (1, 1),
    };
    Arc::new(GuiEmitter {
        referent,
        enabled: bool_or(properties, "Enabled", true),
        rate: float_or(properties, "Rate", 20.0).max(0.0),
        lifetime: number_range_or(properties, "Lifetime", (5.0, 10.0)),
        speed: number_range_or(properties, "Speed", (64.0, 64.0)),
        angle: float_or(properties, "EmissionAngle", 0.0),
        spread_angle: float_or(properties, "SpreadAngle", 0.0),
        bounding_box: ordinal(properties, "EmissionMode", 1) == 1,
        offset: pair("EmissionOffset", (0.0, 0.0)),
        spread: pair("EmissionSpread", (0.0, 0.0)),
        acceleration: pair("Acceleration", (0.0, 0.0)),
        drag: float_or(properties, "Drag", 0.0),
        size: number_sequence_or(properties, "Size", &[(0.0, 32.0), (1.0, 32.0)]),
        squash: number_sequence_or(properties, "Squash", &[(0.0, 0.0), (1.0, 0.0)]),
        transparency: number_sequence_or(properties, "Transparency", &[(0.0, 0.0), (1.0, 0.0)]),
        color: color_sequence_or(properties, "Color", [1.0, 1.0, 1.0]),
        brightness: float_or(properties, "Brightness", 1.0).max(0.0),
        blend: float_or(properties, "Blend", 0.0).clamp(0.0, 1.0),
        rotation: number_range_or(properties, "Rotation", (0.0, 0.0)),
        rot_speed: number_range_or(properties, "RotSpeed", (0.0, 0.0)),
        locked: bool_or(properties, "LockedToParent", false),
        time_scale: float_or(properties, "TimeScale", 1.0).clamp(0.0, 1.0),
        texture: texture(properties),
        flipbook: Flipbook {
            columns,
            rows,
            mode: match ordinal(properties, "FlipbookMode", 0) {
                1 => FlipbookMode::OneShot,
                2 => FlipbookMode::PingPong,
                3 => FlipbookMode::Random,
                _ => FlipbookMode::Loop,
            },
            framerate: number_range_or(properties, "FlipbookFramerate", (1.0, 1.0)),
            start_random: bool_or(properties, "FlipbookStartRandom", false),
            blend_frames: bool_or(properties, "FlipbookBlendFrames", true),
        },
    })
}

fn ordinal(properties: &Properties, key: &str, default: u32) -> u32 {
    match properties.get(key) {
        Some(&Variant::Enum(value)) => value,
        _ => default,
    }
}

/// `TextureContent`: Studio's sparkle when the file has no such property,
/// nothing at all when it holds an empty one.
fn texture(properties: &Properties) -> Option<AssetRef> {
    match properties.get("TextureContent") {
        None => AssetRef::parse(DEFAULT_TEXTURE).ok(),
        Some(Variant::Content(Content::None)) => None,
        Some(value) => asset_uri(value)
            .and_then(|uri| AssetRef::parse(uri).ok())
            .filter(|reference| *reference != AssetRef::Empty),
    }
}

/// Where a parent `GuiObject` came to on screen, what its particles spawn
/// against and, when locked, ride along with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FlatFrame {
    pub(crate) center: [f32; 2],
    pub(crate) size: [f32; 2],
    /// `AbsoluteRotation`, degrees clockwise.
    pub(crate) rotation: f32,
    /// Every `UIScale` from the parent up, multiplied.
    pub(crate) scale: f32,
}

impl FlatFrame {
    /// `local`, in the frame's turned axes, as a screen point.
    pub(crate) fn to_screen(self, local: [f32; 2]) -> [f32; 2] {
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        [
            self.center[0] + local[0] * cos - local[1] * sin,
            self.center[1] + local[0] * sin + local[1] * cos,
        ]
    }
}

#[cfg(test)]
#[path = "flat/tests.rs"]
mod tests;
