//! Model › Insert Rig…: a `Model` with a `Humanoid`, built from nothing, in
//! Studio's own R6 or R15 layout.
//!
//! `build_rig` is a pure function of its options and the place's joint
//! setting: everything the dialog or "My Avatar" adds is laid over its
//! result, so the builder is testable without a window or the network.
//!
//! Roblox publishes no mesh data for its rigs here, so the parts are blocks
//! sized from the documented limits (see `proportions`); the *Feminine* body
//! shape and the in-between Rthro sizes are this editor's presets, not
//! Roblox's tables.

mod avatar;
mod build;
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

impl BodyColors {
    pub(crate) const fn uniform(color: [u8; 3]) -> BodyColors {
        BodyColors {
            head: color,
            torso: color,
            left_arm: color,
            right_arm: color,
            left_leg: color,
            right_leg: color,
        }
    }
}

impl Default for BodyColors {
    /// Medium stone grey, the colour of a new `Part`.
    fn default() -> Self {
        BodyColors::uniform([163, 162, 165])
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
        RigOptions {
            name: "Rig".into(),
            rig_type,
            shape,
            scale,
            joints,
            scales: None,
            colors: BodyColors::default(),
            feet: [0.; 3],
        }
    }
}

#[cfg(test)]
#[path = "rig/tests.rs"]
mod tests;
