//! The R15 accessory attachments (`HatAttachment`, `LeftGripAttachment`
//! and the rest), the same set and on the same parts as the Mannequin's,
//! placed from the fit and each part's bounds the way the Mannequin's sit on
//! its own parts:
//!
//! - Head: Hat and Hair on top of the box, FaceFront on its front face,
//!   FaceCenter at its centre.
//! - UpperTorso: Neck at the neck joint; the collars 0.83 of the way out to
//!   the shoulders and 0.34 of the way down to them; BodyFront and BodyBack
//!   on the box's front and back faces, a little below its centre.
//! - LowerTorso: WaistCenter at the root joint, WaistFront and WaistBack on
//!   the box's faces at that height.
//! - Upper arms: Shoulder halfway in from the joint to the part's middle,
//!   0.7 of the way up from the joint to the top of the part.
//! - Hands: Grip halfway from the wrist to the hand's tip.
//! - Feet: Foot under the ankle, on the sole.
//!
//! Positions are relative to the part's centre and unrotated, like the rig
//! attachments.

use glam::Vec3;

use crate::fit::Fit;
use crate::rig::{Joint, Part, Side};

/// `part`'s accessory attachments, relative to `centre`, for a part whose
/// bounds are `lo`–`hi` (Roblox space).
pub(crate) fn accessories(part: Part, fit: &Fit, lo: Vec3, hi: Vec3) -> Vec<(String, Vec3)> {
    let centre = (lo + hi) / 2.;
    let j = |joint| fit.joint(joint);
    let at = |name: &str, p: Vec3| (format!("{name}Attachment"), p - centre);
    let side_of = |left: bool| if left { Side::Left } else { Side::Right };
    match part {
        Part::Head => vec![
            at("Hat", Vec3::new(centre.x, hi.y, centre.z)),
            at("Hair", Vec3::new(centre.x, hi.y, centre.z)),
            at("FaceFront", Vec3::new(centre.x, centre.y, lo.z)),
            at("FaceCenter", centre),
        ],
        Part::UpperTorso => {
            let neck = j(Joint::Neck);
            let collar = |shoulder: Vec3| {
                Vec3::new(
                    neck.x + 0.83 * (shoulder.x - neck.x),
                    neck.y - 0.34 * (neck.y - shoulder.y),
                    neck.z,
                )
            };
            let size = hi - lo;
            vec![
                at("Neck", neck),
                at("LeftCollar", collar(j(Joint::LeftShoulder))),
                at("RightCollar", collar(j(Joint::RightShoulder))),
                at(
                    "BodyFront",
                    Vec3::new(neck.x, centre.y - 0.09 * size.y, lo.z),
                ),
                at(
                    "BodyBack",
                    Vec3::new(neck.x, centre.y - 0.064 * size.y, hi.z),
                ),
            ]
        }
        Part::LowerTorso => {
            let root = j(Joint::Root);
            vec![
                at("WaistCenter", root),
                at("WaistFront", Vec3::new(root.x, root.y, lo.z)),
                at("WaistBack", Vec3::new(root.x, root.y, hi.z)),
            ]
        }
        Part::LeftUpperArm | Part::RightUpperArm => {
            let side = side_of(part == Part::LeftUpperArm);
            let shoulder = j(part.joint());
            let p = Vec3::new(
                (shoulder.x + centre.x) / 2.,
                shoulder.y + 0.7 * (hi.y - shoulder.y).max(0.),
                shoulder.z,
            );
            vec![at(&format!("{}Shoulder", side.name()), p)]
        }
        Part::LeftHand | Part::RightHand => {
            let side = side_of(part == Part::LeftHand);
            let grip = j(part.joint()).lerp(fit.extremities.hand(side), 0.5);
            vec![at(&format!("{}Grip", side.name()), grip)]
        }
        Part::LeftFoot | Part::RightFoot => {
            let side = side_of(part == Part::LeftFoot);
            let ankle = j(part.joint());
            let p = Vec3::new(ankle.x, lo.y, ankle.z.clamp(lo.z, hi.z));
            vec![at(&format!("{}Foot", side.name()), p)]
        }
        _ => Vec::new(),
    }
}
