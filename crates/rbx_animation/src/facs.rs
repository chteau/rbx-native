//! FACS face channels: the 50 `FaceControls` properties a dynamic head's
//! face animates through, grouped the way the editor's Face menus group
//! them. A clip keys them as `NumberPose`s in a `FaceControls` folder under
//! the `Head` pose. Each one eases from key to key exactly as a joint does.
//! Its value is clamped to 0..1 when played.

use crate::easing::{Direction, Style};

/// (region, channels), in the order Roblox's editor lists them.
pub const REGIONS: [(&str, &[&str]); 5] = [
    (
        "Brows",
        &[
            "Corrugator",
            "LeftBrowLowerer",
            "RightBrowLowerer",
            "LeftInnerBrowRaiser",
            "RightInnerBrowRaiser",
            "LeftOuterBrowRaiser",
            "RightOuterBrowRaiser",
            "LeftNoseWrinkler",
            "RightNoseWrinkler",
        ],
    ),
    (
        "Eyes",
        &[
            "EyesLookDown",
            "EyesLookLeft",
            "EyesLookRight",
            "EyesLookUp",
            "LeftEyeClosed",
            "RightEyeClosed",
            "LeftEyeUpperLidRaiser",
            "RightEyeUpperLidRaiser",
            "LeftCheekRaiser",
            "RightCheekRaiser",
        ],
    ),
    ("Jaw", &["JawDrop", "JawLeft", "JawRight"]),
    (
        "Mouth",
        &[
            "ChinRaiser",
            "ChinRaiserUpperLip",
            "FlatPucker",
            "Funneler",
            "LeftCheekPuff",
            "RightCheekPuff",
            "LeftDimpler",
            "RightDimpler",
            "LeftLipCornerDown",
            "RightLipCornerDown",
            "LeftLipCornerPuller",
            "RightLipCornerPuller",
            "LeftLipStretcher",
            "RightLipStretcher",
            "LeftLowerLipDepressor",
            "RightLowerLipDepressor",
            "LeftUpperLipRaiser",
            "RightUpperLipRaiser",
            "LipPresser",
            "LipsTogether",
            "LowerLipSuck",
            "UpperLipSuck",
            "MouthLeft",
            "MouthRight",
            "Pucker",
        ],
    ),
    ("Tongue", &["TongueDown", "TongueOut", "TongueUp"]),
];

/// The folder under the `Head` pose that holds the face's `NumberPose`s.
pub const FOLDER: &str = "FaceControls";

pub fn is_channel(name: &str) -> bool {
    REGIONS.iter().any(|(_, names)| names.contains(&name))
}

/// One face channel's key: a `NumberPose`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceKey {
    pub value: f32,
    pub style: Style,
    pub direction: Direction,
}

impl FaceKey {
    pub fn new(value: f32) -> FaceKey {
        FaceKey {
            value: value.clamp(0.0, 1.0),
            style: Style::default(),
            direction: Direction::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifty_channels_in_five_regions() {
        let total: usize = REGIONS.iter().map(|(_, n)| n.len()).sum();
        assert_eq!(total, 50);
        assert!(is_channel("JawDrop") && !is_channel("Head"));
        assert_eq!(FaceKey::new(1.7).value, 1.0);
    }
}
