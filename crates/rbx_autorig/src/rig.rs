//! The R15 vocabulary: the 15 body parts, the 15 joints between them, and the
//! two sides. Names are Roblox's own, so `format!("{}RigAttachment", joint.name())`
//! and `format!("{}_Geo", part.name())` are exactly what Studio and the importer
//! expect.

use std::fmt;

/// A side of the character, as the character sees it. In Roblox space the
/// character faces −Z with +Y up, so its left is −X.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn mirror(self) -> Side {
        match self {
            Side::Left => Side::Right,
            Side::Right => Side::Left,
        }
    }

    /// The sign of X on this side in Roblox space: −1 for the left.
    pub fn sign(self) -> f32 {
        match self {
            Side::Left => -1.,
            Side::Right => 1.,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Side::Left => "Left",
            Side::Right => "Right",
        }
    }
}

/// One of the 15 R15 body parts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Part {
    Head,
    UpperTorso,
    LowerTorso,
    LeftUpperArm,
    LeftLowerArm,
    LeftHand,
    RightUpperArm,
    RightLowerArm,
    RightHand,
    LeftUpperLeg,
    LeftLowerLeg,
    LeftFoot,
    RightUpperLeg,
    RightLowerLeg,
    RightFoot,
}

impl Part {
    pub const ALL: [Part; 15] = [
        Part::Head,
        Part::UpperTorso,
        Part::LowerTorso,
        Part::LeftUpperArm,
        Part::LeftLowerArm,
        Part::LeftHand,
        Part::RightUpperArm,
        Part::RightLowerArm,
        Part::RightHand,
        Part::LeftUpperLeg,
        Part::LeftLowerLeg,
        Part::LeftFoot,
        Part::RightUpperLeg,
        Part::RightLowerLeg,
        Part::RightFoot,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Part::Head => "Head",
            Part::UpperTorso => "UpperTorso",
            Part::LowerTorso => "LowerTorso",
            Part::LeftUpperArm => "LeftUpperArm",
            Part::LeftLowerArm => "LeftLowerArm",
            Part::LeftHand => "LeftHand",
            Part::RightUpperArm => "RightUpperArm",
            Part::RightLowerArm => "RightLowerArm",
            Part::RightHand => "RightHand",
            Part::LeftUpperLeg => "LeftUpperLeg",
            Part::LeftLowerLeg => "LeftLowerLeg",
            Part::LeftFoot => "LeftFoot",
            Part::RightUpperLeg => "RightUpperLeg",
            Part::RightLowerLeg => "RightLowerLeg",
            Part::RightFoot => "RightFoot",
        }
    }

    /// The part this one hangs from in the R15 hierarchy; `None` for
    /// `LowerTorso`, which hangs from the (mesh-less) HumanoidRootPart.
    pub fn parent(self) -> Option<Part> {
        Some(match self {
            Part::LowerTorso => return None,
            Part::UpperTorso => Part::LowerTorso,
            Part::Head => Part::UpperTorso,
            Part::LeftUpperArm | Part::RightUpperArm => Part::UpperTorso,
            Part::LeftLowerArm => Part::LeftUpperArm,
            Part::LeftHand => Part::LeftLowerArm,
            Part::RightLowerArm => Part::RightUpperArm,
            Part::RightHand => Part::RightLowerArm,
            Part::LeftUpperLeg | Part::RightUpperLeg => Part::LowerTorso,
            Part::LeftLowerLeg => Part::LeftUpperLeg,
            Part::LeftFoot => Part::LeftLowerLeg,
            Part::RightLowerLeg => Part::RightUpperLeg,
            Part::RightFoot => Part::RightLowerLeg,
        })
    }

    /// The joint that drives this part (the joint whose `Part1` it is).
    pub fn joint(self) -> Joint {
        Joint::ALL
            .into_iter()
            .find(|j| j.part() == self)
            .expect("every part has a joint")
    }

    pub fn side(self) -> Option<Side> {
        self.joint().side()
    }

    pub fn mirror(self) -> Part {
        self.joint().mirror().part()
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Part {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One of the 15 R15 joints, named as Roblox names the `Motor6D` and its two
/// `<Joint>RigAttachment`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Joint {
    Root,
    Waist,
    Neck,
    LeftShoulder,
    LeftElbow,
    LeftWrist,
    RightShoulder,
    RightElbow,
    RightWrist,
    LeftHip,
    LeftKnee,
    LeftAnkle,
    RightHip,
    RightKnee,
    RightAnkle,
}

impl Joint {
    pub const ALL: [Joint; 15] = [
        Joint::Root,
        Joint::Waist,
        Joint::Neck,
        Joint::LeftShoulder,
        Joint::LeftElbow,
        Joint::LeftWrist,
        Joint::RightShoulder,
        Joint::RightElbow,
        Joint::RightWrist,
        Joint::LeftHip,
        Joint::LeftKnee,
        Joint::LeftAnkle,
        Joint::RightHip,
        Joint::RightKnee,
        Joint::RightAnkle,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Joint::Root => "Root",
            Joint::Waist => "Waist",
            Joint::Neck => "Neck",
            Joint::LeftShoulder => "LeftShoulder",
            Joint::LeftElbow => "LeftElbow",
            Joint::LeftWrist => "LeftWrist",
            Joint::RightShoulder => "RightShoulder",
            Joint::RightElbow => "RightElbow",
            Joint::RightWrist => "RightWrist",
            Joint::LeftHip => "LeftHip",
            Joint::LeftKnee => "LeftKnee",
            Joint::LeftAnkle => "LeftAnkle",
            Joint::RightHip => "RightHip",
            Joint::RightKnee => "RightKnee",
            Joint::RightAnkle => "RightAnkle",
        }
    }

    /// The part this joint drives (its `Part1`).
    pub fn part(self) -> Part {
        match self {
            Joint::Root => Part::LowerTorso,
            Joint::Waist => Part::UpperTorso,
            Joint::Neck => Part::Head,
            Joint::LeftShoulder => Part::LeftUpperArm,
            Joint::LeftElbow => Part::LeftLowerArm,
            Joint::LeftWrist => Part::LeftHand,
            Joint::RightShoulder => Part::RightUpperArm,
            Joint::RightElbow => Part::RightLowerArm,
            Joint::RightWrist => Part::RightHand,
            Joint::LeftHip => Part::LeftUpperLeg,
            Joint::LeftKnee => Part::LeftLowerLeg,
            Joint::LeftAnkle => Part::LeftFoot,
            Joint::RightHip => Part::RightUpperLeg,
            Joint::RightKnee => Part::RightLowerLeg,
            Joint::RightAnkle => Part::RightFoot,
        }
    }

    /// The part this joint hangs from (its `Part0`); `None` for `Root`, whose
    /// `Part0` is HumanoidRootPart.
    pub fn parent_part(self) -> Option<Part> {
        self.part().parent()
    }

    pub fn side(self) -> Option<Side> {
        match self {
            Joint::Root | Joint::Waist | Joint::Neck => None,
            Joint::LeftShoulder
            | Joint::LeftElbow
            | Joint::LeftWrist
            | Joint::LeftHip
            | Joint::LeftKnee
            | Joint::LeftAnkle => Some(Side::Left),
            _ => Some(Side::Right),
        }
    }

    /// The same joint on the other side; centre-line joints mirror to
    /// themselves.
    pub fn mirror(self) -> Joint {
        match self {
            Joint::LeftShoulder => Joint::RightShoulder,
            Joint::LeftElbow => Joint::RightElbow,
            Joint::LeftWrist => Joint::RightWrist,
            Joint::RightShoulder => Joint::LeftShoulder,
            Joint::RightElbow => Joint::LeftElbow,
            Joint::RightWrist => Joint::LeftWrist,
            Joint::LeftHip => Joint::RightHip,
            Joint::LeftKnee => Joint::RightKnee,
            Joint::LeftAnkle => Joint::RightAnkle,
            Joint::RightHip => Joint::LeftHip,
            Joint::RightKnee => Joint::LeftKnee,
            Joint::RightAnkle => Joint::LeftAnkle,
            centre => centre,
        }
    }

    /// The rig attachment name both parts carry for this joint.
    pub fn attachment(self) -> String {
        format!("{}RigAttachment", self.name())
    }

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for Joint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
