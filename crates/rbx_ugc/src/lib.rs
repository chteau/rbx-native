//! Checks an avatar item (rigid accessory, layered clothing, body or head)
//! against the limits Roblox publishes for the Marketplace, before anything
//! is uploaded. The caller describes the item as plain geometry ([`Item`]),
//! read from a file or from the editor's instances, and gets back one
//! [`Finding`] per broken or unchecked rule. No UI, no I/O.
//!
//! These are the documented rules (see [`spec`]); Roblox's own validator runs
//! server-side and can refuse things the pages don't spell out.

pub mod head;
pub mod schema;
pub mod spec;

mod rules;

pub use spec::{AssetType, BodyGroup, BodyScale, Bounds, Category};

/// Mesh data in the part's own space. `positions` are studs when
/// `in_studs` is set (a `MeshPart` scaled to its `Size`); a source file's
/// units are its own, so size rules skip it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub triangles: Vec<[u32; 3]>,
    /// One per vertex, or empty when the mesh has none.
    pub uvs: Vec<[f32; 2]>,
    /// Bones each vertex follows, one per vertex; empty when unskinned.
    pub influences: Vec<u8>,
    /// Bone names of a skinned mesh, in file order.
    pub bones: Vec<String>,
    pub in_studs: bool,
}

/// A position and rotation, Roblox `CFrame` component order: x, y, z, then
/// the 3 x 3 rotation row by row.
pub type CFrame = [f32; 12];

pub const IDENTITY: CFrame = [0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

/// An `Attachment` under a part, placed in the part's space.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub name: String,
    pub cframe: CFrame,
}

/// One render mesh: an accessory's `Handle` or one body part.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub name: String,
    /// `None` when the mesh could not be read; its rules are reported as
    /// not checked.
    pub geometry: Option<Geometry>,
    /// The part in the item's space (a body's parts relative to each other).
    pub cframe: CFrame,
    pub attachments: Vec<Placement>,
}

/// A texture's pixel size and where it is used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texture {
    /// The part or `SurfaceAppearance` using it.
    pub owner: String,
    /// `TextureID`, `ColorMap`, a file name...
    pub slot: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CageKind {
    Inner,
    Outer,
}

/// A layered item's cage mesh as found, by name.
#[derive(Debug, Clone, PartialEq)]
pub struct Cage {
    pub kind: CageKind,
    pub name: String,
    pub geometry: Geometry,
    /// The cage it was made from, when known: its vertex count and UVs must
    /// survive untouched.
    pub template: Option<Geometry>,
}

/// Everything about an item the rules look at.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    /// The accessory or model name; cages are named after it.
    pub name: String,
    pub asset_type: AssetType,
    /// Picks the rigid size table.
    pub scale: BodyScale,
    pub parts: Vec<Part>,
    pub textures: Vec<Texture>,
    pub cages: Vec<Cage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// Roblox refuses the item.
    Error,
    /// Against the spec, but not something Roblox is known to refuse.
    Warning,
    /// A rule that could not be checked, and why.
    Note,
}

/// The rule a finding is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rule {
    MeshTriangles,
    AccessoryTriangles,
    BodyTriangles,
    SingleMesh,
    TextureSize,
    /// A handle with neither a `TextureID` nor a `SurfaceAppearance`. No
    /// spec page states it; Studio's Save to Roblox refuses it.
    MissingTexture,
    BoneInfluences,
    Attachment,
    ItemSize,
    BodyPartSize,
    BodySize,
    CageMissing,
    CageTopology,
    CageUvs,
    Watertight,
    /// Instances Studio's schema doesn't allow, or required ones missing.
    Schema,
    Tags,
    Attributes,
    /// A property Studio wants a set value for (`Anchored`, an unscaled
    /// layered handle).
    Property,
    /// A layered item's inner cage, which Studio makes the HSR data from.
    Hsr,
    /// Eyebrow and eyelash skinning transfer.
    Skinning,
    Unreadable,
    Head(head::HeadRule),
}

impl Rule {
    /// Short title for a report heading.
    pub fn title(self) -> &'static str {
        match self {
            Rule::MeshTriangles => "Mesh triangles",
            Rule::AccessoryTriangles => "Accessory triangles",
            Rule::BodyTriangles => "Body triangles",
            Rule::SingleMesh => "Single mesh",
            Rule::TextureSize => "Texture size",
            Rule::MissingTexture => "Texture",
            Rule::BoneInfluences => "Bone influences",
            Rule::Attachment => "Attachment",
            Rule::ItemSize => "Item size",
            Rule::BodyPartSize => "Body part size",
            Rule::BodySize => "Body size",
            Rule::CageMissing => "Cages",
            Rule::CageTopology => "Cage vertices",
            Rule::CageUvs => "Cage UVs",
            Rule::Watertight => "Watertight",
            Rule::Schema => "Instances",
            Rule::Tags => "Tags",
            Rule::Attributes => "Attributes",
            Rule::Property => "Properties",
            Rule::Hsr => "HSR data",
            Rule::Skinning => "Skinning",
            Rule::Unreadable => "Not checked",
            Rule::Head(rule) => rule.title(),
        }
    }

    /// The creator-docs page the rule's number comes from.
    pub fn source(self) -> &'static str {
        match self {
            Rule::MeshTriangles | Rule::BoneInfluences | Rule::Unreadable => {
                "https://create.roblox.com/docs/art/modeling/specifications"
            }
            Rule::AccessoryTriangles
            | Rule::SingleMesh
            | Rule::TextureSize
            | Rule::MissingTexture
            | Rule::Attachment
            | Rule::ItemSize
            | Rule::Schema
            | Rule::Tags
            | Rule::Attributes
            | Rule::Property => {
                "https://create.roblox.com/docs/avatar/rigid-accessories/specifications"
            }
            Rule::CageMissing | Rule::CageTopology | Rule::CageUvs | Rule::Hsr | Rule::Skinning => {
                "https://create.roblox.com/docs/avatar/layered-accessories/specifications"
            }
            Rule::BodyTriangles | Rule::BodyPartSize | Rule::BodySize | Rule::Watertight => {
                "https://create.roblox.com/docs/avatar/character-bodies/specifications"
            }
            Rule::Head(rule) => rule.source(),
        }
    }
}

/// A measured value or a limit.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Count(usize),
    Pixels {
        width: u32,
        height: u32,
    },
    /// Width, height, depth.
    Studs([f32; 3]),
    Text(String),
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Count(n) => write!(f, "{n}"),
            Value::Pixels { width, height } => write!(f, "{width} \u{d7} {height}"),
            Value::Studs([x, y, z]) => write!(f, "{x:.2} \u{d7} {y:.2} \u{d7} {z:.2} studs"),
            Value::Text(text) => f.write_str(text),
        }
    }
}

/// One broken or unchecked rule.
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub rule: Rule,
    pub severity: Severity,
    /// The mesh, part, cage or texture it is about.
    pub subject: String,
    pub measured: Option<Value>,
    pub limit: Option<Value>,
    /// What is wrong and what to do about it, as one or two sentences.
    pub message: String,
}

/// Every finding for `item`, errors first. Empty means every documented
/// rule that applies passed.
pub fn validate(item: &Item) -> Vec<Finding> {
    let mut findings = Vec::new();
    rules::unreadable(item, &mut findings);
    rules::triangles(item, &mut findings);
    rules::textures(item, &mut findings);
    rules::influences(item, &mut findings);
    rules::attachment(item, &mut findings);
    rules::joints(item, &mut findings);
    rules::size(item, &mut findings);
    rules::cages(item, &mut findings);
    rules::watertight(item, &mut findings);
    findings.sort_by_key(|f| f.severity);
    findings
}

/// Whether any finding blocks an upload.
pub fn blocks(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.severity == Severity::Error)
}
