//! What a graph holds beside its nodes: the shared masks Mask nodes pick
//! from and the expression nodes properties can be wired to.

/// One joint (or part) of a mask, nested as the rig is.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskJoint {
    pub name: String,
    /// 0..1; the editor seeds every joint at 1.
    pub weight: f32,
    pub children: Vec<MaskJoint>,
}

/// A shared mask: a weight per joint, under the graph's `SharedMasks`.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub name: String,
    /// `RBX_MaskRigType`: "HRD" (built from a HumanoidRigDescription) or
    /// "Custom".
    pub rig_type: String,
    pub rig_name: String,
    pub joints: Vec<MaskJoint>,
}

impl Mask {
    pub fn new(name: &str) -> Mask {
        Mask {
            name: name.to_owned(),
            rig_type: "HRD".into(),
            rig_name: String::new(),
            joints: Vec::new(),
        }
    }

    /// Every joint depth first, the order the editor numbers them in.
    pub fn flatten(&self) -> Vec<(&str, f32)> {
        fn walk<'a>(joints: &'a [MaskJoint], out: &mut Vec<(&'a str, f32)>) {
            for j in joints {
                out.push((&j.name, j.weight));
                walk(&j.children, out);
            }
        }
        let mut out = Vec::new();
        walk(&self.joints, &mut out);
        out
    }

    pub fn weight(&self, joint: &str) -> Option<f32> {
        self.flatten()
            .into_iter()
            .find(|(name, _)| *name == joint)
            .map(|(_, w)| w)
    }
}

/// An expression node (`AnimationValueNodeDefinition`, `NodeType`
/// Expression). Properties bind to one by its source text
/// ([`super::Prop::Expr`]); this keeps its name and place on the canvas, and
/// keeps one alive that nothing is wired to yet.
#[derive(Clone, Debug, PartialEq)]
pub struct Expression {
    /// The instance name: `Expression`, `Expression1`, …
    pub name: String,
    pub source: String,
    pub position: (f32, f32),
    pub size: Option<(f32, f32)>,
    pub collapsed: bool,
}
