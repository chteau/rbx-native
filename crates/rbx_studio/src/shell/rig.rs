//! Model › Insert Rig…: a `Model` with a `Humanoid`, built from nothing, in
//! Studio's own R6 or R15 layout.
//!
//! `build_rig` is a pure function of its options and the place's joint
//! setting: everything the dialog or "My Avatar" adds is laid over its
//! result, so the builder is testable without a window or the network.
//!
//! An R15 rig is made of `MeshPart`s with Roblox's own body meshes and
//! attachments (see `bundle`), plus `BodyColors`, `Shirt`, `Pants` and the
//! stock `Animate` script. R6 is the classic block rig. The *Feminine* body
//! shape, the in-between Rthro sizes and the Rthro Slender sizes (scaled
//! Classic-compatible meshes, as no Rthro mesh ids are public) are this
//! editor's presets, not Roblox's tables.

mod animate;
mod avatar;
mod bones;
mod build;
mod bundle;
mod cframe;
mod dialog;
mod insert;
mod layout;
mod proportions;
mod r15;
mod r6;

use rbx_dom::{Variant, WeakDom};

pub(crate) use build::build_rig;
pub(crate) use cframe::V3;
pub(crate) use dialog::RigDialog;
pub(crate) use proportions::{BodyScale, BodyShape, RigType, Scales};

/// `StarterPlayer.AvatarJointUpgrade`: whether R15 characters use
/// `AnimationConstraint`s or the older `Motor6D`s.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JointStyle {
    AnimationConstraint,
    Motor6D,
}

const JOINT_UPGRADE: &str = "AvatarJointUpgrade_SerializedRollout";
const JOINT_UPGRADE_DISABLED: u32 = 1;

impl JointStyle {
    /// The place's setting; `Default` counts as enabled, as it does in
    /// Studio's current rollout.
    pub(crate) fn of_place(dom: &WeakDom) -> JointStyle {
        let disabled = dom
            .root_refs()
            .iter()
            .filter_map(|&root| dom.get(root))
            .filter(|instance| instance.class() == "StarterPlayer")
            .any(|player| {
                player.properties().get(JOINT_UPGRADE)
                    == Some(&Variant::Enum(JOINT_UPGRADE_DISABLED))
            });
        if disabled {
            JointStyle::Motor6D
        } else {
            JointStyle::AnimationConstraint
        }
    }
}

/// An 8-bit sRGB colour per limb group, as in `BodyColors`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BodyColors {
    pub(crate) head: [u8; 3],
    pub(crate) torso: [u8; 3],
    pub(crate) left_arm: [u8; 3],
    pub(crate) right_arm: [u8; 3],
    pub(crate) left_leg: [u8; 3],
    pub(crate) right_leg: [u8; 3],
}

impl Default for BodyColors {
    /// The classic Roblox starter look: yellow head and arms, blue torso,
    /// green legs.
    fn default() -> Self {
        BodyColors {
            head: [245, 205, 48],
            torso: [13, 105, 172],
            left_arm: [245, 205, 48],
            right_arm: [245, 205, 48],
            left_leg: [75, 151, 75],
            right_leg: [75, 151, 75],
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RigOptions {
    pub(crate) name: String,
    pub(crate) rig_type: RigType,
    pub(crate) shape: BodyShape,
    pub(crate) scale: BodyScale,
    /// Ignored for R6, which is always `Motor6D`.
    pub(crate) joints: JointStyle,
    /// Replaces the preset of `shape` and `scale` (a user's own avatar).
    pub(crate) scales: Option<Scales>,
    pub(crate) colors: BodyColors,
    /// The R15 body meshes; a player's own packages replace stock entries.
    pieces: bundle::Pieces,
    /// Where the soles of the feet stand.
    pub(crate) feet: V3,
}

impl RigOptions {
    pub(crate) fn new(
        rig_type: RigType,
        shape: BodyShape,
        scale: BodyScale,
        joints: JointStyle,
    ) -> RigOptions {
        let family = bundle::Family::of(Scales::preset(scale, shape).body_type);
        RigOptions {
            name: "Rig".into(),
            rig_type,
            shape,
            scale,
            joints,
            scales: None,
            colors: BodyColors::default(),
            pieces: bundle::stock(family, shape),
            feet: [0.; 3],
        }
    }

    /// The scales the rig is built at: its own, or the preset of its shape.
    pub(crate) fn scales(&self) -> Scales {
        self.scales
            .unwrap_or_else(|| Scales::preset(self.scale, self.shape))
    }

    /// Sets the scales and swaps the stock parts for the family they call for.
    pub(crate) fn set_scales(&mut self, scales: Scales) {
        self.scales = Some(scales);
        self.pieces = bundle::stock(self.family(), self.shape);
    }

    fn family(&self) -> bundle::Family {
        bundle::Family::of(self.scales().body_type)
    }

    /// Only the Mannequin body has no female form of its own to widen.
    fn hip_spread(&self) -> f32 {
        if self.shape == BodyShape::Feminine && self.family() == bundle::Family::Mannequin {
            proportions::FEMININE_HIP_SPREAD
        } else {
            1.
        }
    }
}

#[cfg(test)]
#[path = "rig/tests.rs"]
mod tests;
