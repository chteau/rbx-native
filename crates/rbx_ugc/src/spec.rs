//! Roblox's published avatar item limits, copied from the creator docs. Each
//! table names the page it came from; a limit no page gives a number for is
//! not here, and the rule that would use it is skipped.

/// What kind of avatar item an [`AssetType`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Rigid,
    Layered,
    Body,
}

/// The Marketplace asset type an item is uploaded as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssetType {
    Hat,
    Hair,
    Face,
    Neck,
    Shoulder,
    Front,
    Back,
    Waist,
    Eyebrow,
    Eyelash,
    TShirt,
    Shirt,
    Sweater,
    Jacket,
    Pants,
    Shorts,
    DressSkirt,
    LeftShoe,
    RightShoe,
    /// A whole 15-part body.
    Body,
    /// A dynamic head on its own.
    DynamicHead,
}

impl AssetType {
    pub const ALL: [AssetType; 21] = [
        AssetType::Hat,
        AssetType::Hair,
        AssetType::Face,
        AssetType::Neck,
        AssetType::Shoulder,
        AssetType::Front,
        AssetType::Back,
        AssetType::Waist,
        AssetType::Eyebrow,
        AssetType::Eyelash,
        AssetType::TShirt,
        AssetType::Shirt,
        AssetType::Sweater,
        AssetType::Jacket,
        AssetType::Pants,
        AssetType::Shorts,
        AssetType::DressSkirt,
        AssetType::LeftShoe,
        AssetType::RightShoe,
        AssetType::Body,
        AssetType::DynamicHead,
    ];

    /// Eyebrows and eyelashes are layered, though Studio's Accessory Fitting
    /// Tool lists them with the rigid types: "Face accessories, such as
    /// eyebrows and eyelashes, are unique layered accessories"
    /// (<https://create.roblox.com/docs/avatar/layered-accessories/specifications>),
    /// and Studio's Save to Roblox refuses one without a `WrapLayer`.
    pub fn category(self) -> Category {
        use AssetType::*;
        match self {
            Hat | Hair | Face | Neck | Shoulder | Front | Back | Waist => Category::Rigid,
            Eyebrow | Eyelash | TShirt | Shirt | Sweater | Jacket | Pants | Shorts | DressSkirt
            | LeftShoe | RightShoe => Category::Layered,
            Body | DynamicHead => Category::Body,
        }
    }

    /// The name Studio's Asset Category list and the Creator Hub use.
    pub fn label(self) -> &'static str {
        use AssetType::*;
        match self {
            Hat => "Hat",
            Hair => "Hair",
            Face => "Face",
            Neck => "Neck",
            Shoulder => "Shoulder",
            Front => "Front",
            Back => "Back",
            Waist => "Waist",
            Eyebrow => "Eyebrow",
            Eyelash => "Eyelash",
            TShirt => "T-Shirt",
            Shirt => "Shirt",
            Sweater => "Sweater",
            Jacket => "Jacket",
            Pants => "Pants",
            Shorts => "Shorts",
            DressSkirt => "Dress & Skirt",
            LeftShoe => "Left Shoe",
            RightShoe => "Right Shoe",
            Body => "Body",
            DynamicHead => "Head",
        }
    }

    /// The `Enum.AssetType` name Studio's validator messages use.
    pub fn type_name(self) -> &'static str {
        use AssetType::*;
        match self {
            Hat => "Hat",
            Hair => "HairAccessory",
            Face => "FaceAccessory",
            Neck => "NeckAccessory",
            Shoulder => "ShoulderAccessory",
            Front => "FrontAccessory",
            Back => "BackAccessory",
            Waist => "WaistAccessory",
            Eyebrow => "EyebrowAccessory",
            Eyelash => "EyelashAccessory",
            TShirt => "TShirtAccessory",
            Shirt => "ShirtAccessory",
            Sweater => "SweaterAccessory",
            Jacket => "JacketAccessory",
            Pants => "PantsAccessory",
            Shorts => "ShortsAccessory",
            DressSkirt => "DressSkirtAccessory",
            LeftShoe => "LeftShoeAccessory",
            RightShoe => "RightShoeAccessory",
            Body => "Body",
            DynamicHead => "DynamicHead",
        }
    }

    /// Attachments the item's handle may use.
    /// <https://create.roblox.com/docs/avatar/rigid-accessories/specifications>
    /// and <https://create.roblox.com/docs/avatar/layered-accessories/specifications>,
    /// "Attachment points". Shoes have no row there; Studio accepts them on
    /// the foot attachments. Bodies and heads have none.
    pub fn attachments(self) -> &'static [&'static str] {
        use AssetType::*;
        match self {
            Hat => &["HatAttachment"],
            Hair => &["HairAttachment"],
            Back => &["BodyBackAttachment"],
            Waist => &[
                "WaistFrontAttachment",
                "WaistCenterAttachment",
                "WaistBackAttachment",
            ],
            Shoulder => &[
                "RightShoulderAttachment",
                "RightCollarAttachment",
                "NeckAttachment",
                "LeftCollarAttachment",
                "LeftShoulderAttachment",
            ],
            Face | Eyebrow | Eyelash => &["FaceFrontAttachment", "FaceCenterAttachment"],
            Neck => &["NeckAttachment"],
            Front | TShirt | Shirt | Sweater | Jacket => &["BodyFrontAttachment"],
            Pants | Shorts | DressSkirt => &["WaistCenterAttachment"],
            LeftShoe => &["LeftFootAttachment"],
            RightShoe => &["RightFootAttachment"],
            Body | DynamicHead => &[],
        }
    }
}

/// The body a rigid accessory is sized for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum BodyScale {
    Classic,
    #[default]
    Normal,
    Slender,
}

impl BodyScale {
    pub const ALL: [BodyScale; 3] = [BodyScale::Classic, BodyScale::Normal, BodyScale::Slender];

    pub fn label(self) -> &'static str {
        match self {
            BodyScale::Classic => "Classic",
            BodyScale::Normal => "Normal",
            BodyScale::Slender => "Slender",
        }
    }

    /// The `AvatarPartScaleType` value Studio writes for it.
    pub fn part_scale_type(self) -> &'static str {
        match self {
            BodyScale::Classic => "Classic",
            BodyScale::Normal => "ProportionsNormal",
            BodyScale::Slender => "ProportionsSlender",
        }
    }
}

/// Triangles in one mesh of any kind.
/// <https://create.roblox.com/docs/art/modeling/specifications>, "Budgets".
pub const MESH_TRIANGLES: usize = 20_000;
/// Triangles in a rigid or layered accessory.
/// <https://create.roblox.com/docs/avatar/rigid-accessories/specifications>,
/// <https://create.roblox.com/docs/avatar/layered-accessories/specifications>.
pub const ACCESSORY_TRIANGLES: usize = 4_000;
/// Longest side of a Marketplace texture, rigid, layered and bodies alike
/// (the three specification pages above).
pub const TEXTURE_SIZE: u32 = 2048;
/// Bones or joints one vertex may follow.
/// <https://create.roblox.com/docs/art/modeling/specifications>, "Max influences".
pub const MAX_INFLUENCES: usize = 4;

/// A box an item must fit in, in studs, placed on its attachment: `offset`
/// moves the box centre from the attachment along the attachment's own axes
/// (+Y up, +Z behind), which is how the pages' "not centered" rows read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub size: [f32; 3],
    pub offset: [f32; 3],
}

const fn centred(x: f32, y: f32, z: f32) -> Bounds {
    Bounds {
        size: [x, y, z],
        offset: [0.0, 0.0, 0.0],
    }
}

/// A row the page gives as "N* (a up, b down)" / "(a front, b behind)":
/// `up`/`down` and `front`/`behind` are those split extents.
const fn split(x: f32, up: f32, down: f32, front: f32, behind: f32) -> Bounds {
    Bounds {
        size: [x, up + down, front + behind],
        offset: [0.0, (up - down) / 2.0, (behind - front) / 2.0],
    }
}

/// The box a rigid accessory or layered item must fit, or `None` when the
/// pages give no number (bodies, heads). `attachment` picks the
/// Shoulder row.
/// <https://create.roblox.com/docs/avatar/rigid-accessories/specifications>,
/// "Body scale", the Normal, Slender and Classic tables;
/// <https://create.roblox.com/docs/avatar/layered-accessories/specifications>,
/// "Geometry and budgets" for the layered 8 x 8 x 8 row.
pub fn item_bounds(asset: AssetType, scale: BodyScale, attachment: &str) -> Option<Bounds> {
    use AssetType::*;
    use BodyScale::*;
    let neck = attachment == "NeckAttachment";
    let collar = attachment.ends_with("CollarAttachment");
    Some(match (asset, scale) {
        (Eyebrow | Eyelash, _) => centred(1.5, 0.5, 0.5),
        // Shoes have no row on the page; Studio 0.745 measures them against
        // the same 8 x 8 x 8 ("The max size for type LeftShoeAccessory is
        // [8.00, 8.00, 8.00]").
        (
            TShirt | Shirt | Sweater | Jacket | Pants | Shorts | DressSkirt | LeftShoe | RightShoe,
            _,
        ) => centred(8.0, 8.0, 8.0),
        (Body | DynamicHead, _) => return None,

        (Hat, Normal) => centred(1.87, 2.5, 1.87),
        (Hair, Normal) => split(1.87, 1.25, 1.875, 0.9375, 1.25),
        (Face, Normal) => centred(1.87, 1.25, 1.25),
        (Neck, Normal) => centred(2.95, 3.68, 2.16),
        (Shoulder, Normal) if neck => centred(6.90, 3.68, 3.24),
        (Shoulder, Normal) if collar => centred(2.95, 3.68, 3.24),
        (Shoulder, Normal) => centred(2.67, 4.40, 3.09),
        (Front, Normal) => centred(2.95, 3.68, 3.24),
        (Back, Normal) => split(9.86, 4.295, 4.295, 1.623, 3.246),
        (Waist, Normal) => Bounds {
            size: [3.94, 4.29, 7.57],
            offset: [0.0, (1.842 - 2.457) / 2.0, 0.0],
        },

        (Hat, Slender) => centred(1.78, 2.5, 1.78),
        // The page splits the 2.08 depth as "1.892 front, 1.189 behind",
        // which adds up to 3.08; 2.08 less 1.189 behind is 0.891 front, the
        // Normal row's proportion, so the "1.892" reads as a typo of 0.892.
        (Hair, Slender) => split(1.78, 1.25, 1.875, 2.08 - 1.189, 1.189),
        (Face, Slender) => centred(1.78, 1.25, 1.18),
        (Neck, Slender) => centred(2.59, 3.39, 1.92),
        (Shoulder, Slender) if neck => centred(6.05, 3.39, 2.88),
        (Shoulder, Slender) if collar => centred(2.59, 3.39, 2.88),
        (Shoulder, Slender) => centred(2.37, 3.96, 2.75),
        (Front, Slender) => centred(2.59, 3.39, 2.88),
        (Back, Slender) => split(8.64, 3.955, 3.955, 1.443, 2.886),
        (Waist, Slender) => Bounds {
            size: [3.76, 3.29, 6.73],
            offset: [0.0, (1.414 - 1.885) / 2.0, 0.0],
        },

        (Hat, Classic) => centred(3.0, 4.0, 3.0),
        (Hair, Classic) => split(3.0, 2.0, 3.0, 1.5, 2.0),
        (Face, Classic) => centred(3.0, 2.0, 2.0),
        (Neck, Classic) => centred(3.0, 3.0, 2.0),
        (Shoulder, Classic) if neck => centred(7.0, 3.0, 3.0),
        (Shoulder, Classic) => centred(3.0, 3.0, 3.0),
        (Front, Classic) => centred(3.0, 3.0, 3.0),
        (Back, Classic) => split(10.0, 3.5, 3.5, 1.5, 3.0),
        (Waist, Classic) => split(4.0, 1.5, 2.0, 3.5, 3.5),
    })
}

/// The four groups a body's size limits are given for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BodyGroup {
    Head,
    Arm,
    Torso,
    Leg,
}

impl BodyGroup {
    /// The group an R15 part belongs to.
    pub fn of(part: &str) -> Option<BodyGroup> {
        Some(match part {
            "Head" => BodyGroup::Head,
            "UpperTorso" | "LowerTorso" => BodyGroup::Torso,
            p if p.ends_with("UpperArm") || p.ends_with("LowerArm") || p.ends_with("Hand") => {
                BodyGroup::Arm
            }
            p if p.ends_with("UpperLeg") || p.ends_with("LowerLeg") || p.ends_with("Foot") => {
                BodyGroup::Leg
            }
            _ => return None,
        })
    }
}

/// Smallest and largest box, width x length x depth in studs, a body group
/// or (with `None`) the whole body may take. The same for every body scale.
/// <https://create.roblox.com/docs/avatar/character-bodies/specifications>,
/// "Bounding box limits".
pub fn body_bounds(group: Option<BodyGroup>) -> ([f32; 3], [f32; 3]) {
    match group {
        Some(BodyGroup::Head) => ([0.05, 0.5, 0.05], [4.0, 2.0, 4.0]),
        Some(BodyGroup::Arm) => ([0.05, 0.5, 0.05], [4.0, 4.5, 4.0]),
        Some(BodyGroup::Torso) => ([0.05, 0.65, 0.05], [4.0, 4.0, 4.0]),
        Some(BodyGroup::Leg) => ([0.05, 0.5, 0.05], [4.0, 4.5, 4.0]),
        None => ([1.0, 4.0, 0.5], [8.0, 10.0, 4.0]),
    }
}

/// The 15 R15 parts, in the order the body page lists them.
pub const R15_PARTS: [&str; 15] = [
    "Head",
    "UpperTorso",
    "LowerTorso",
    "LeftUpperArm",
    "LeftLowerArm",
    "LeftHand",
    "RightUpperArm",
    "RightLowerArm",
    "RightHand",
    "LeftUpperLeg",
    "LeftLowerLeg",
    "LeftFoot",
    "RightUpperLeg",
    "RightLowerLeg",
    "RightFoot",
];

/// Triangle budgets per body asset: the asset's name, its parts and the most
/// triangles they may hold together. The whole body is [`BODY_TRIANGLES`].
/// <https://create.roblox.com/docs/avatar/character-bodies/specifications>,
/// "Triangle budgets".
pub const BODY_ASSET_TRIANGLES: [(&str, &[&str], usize); 6] = [
    ("DynamicHead", &["Head"], 4000),
    ("Torso", &["UpperTorso", "LowerTorso"], 1750),
    (
        "LeftArm",
        &["LeftUpperArm", "LeftLowerArm", "LeftHand"],
        1248,
    ),
    (
        "RightArm",
        &["RightUpperArm", "RightLowerArm", "RightHand"],
        1248,
    ),
    (
        "LeftLeg",
        &["LeftUpperLeg", "LeftLowerLeg", "LeftFoot"],
        1248,
    ),
    (
        "RightLeg",
        &["RightUpperLeg", "RightLowerLeg", "RightFoot"],
        1248,
    ),
];
pub const BODY_TRIANGLES: usize = 10_742;

/// Robux paid on the Creator Hub per submission, and with an emissive mask.
/// <https://create.roblox.com/docs/marketplace/marketplace-fees-and-commissions>,
/// "Upload fees".
pub const UPLOAD_FEE: u32 = 80;
pub const EMISSIVE_UPLOAD_FEE: u32 = 500;

/// The refundable publishing advance in Robux when the item is put on sale:
/// (non-limited, paid limited), `None` where the table has no row. With an emissive mask the non-limited
/// advance is five times this. Same page, "Publishing advance".
pub fn publishing_advance(asset: AssetType) -> Option<(u32, u32)> {
    use AssetType::*;
    Some(match asset {
        Hat | Face => (1500, 13000),
        Hair => (1000, 6000),
        Neck | Shoulder | Front | Back | Waist => (1000, 15000),
        Jacket | TShirt | Shirt | Sweater | Pants | DressSkirt | Shorts | LeftShoe | RightShoe => {
            (600, 5000)
        }
        Body => (2500, 20000),
        DynamicHead => (1500, 10000),
        // The page's table has no Eyebrow or Eyelash row.
        Eyebrow | Eyelash => return None,
    })
}

/// The non-limited advance with an emissive mask: "5 times the standard".
pub const EMISSIVE_ADVANCE_FACTOR: u32 = 5;
