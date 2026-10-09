//! Classic clothing: the `Shirt`, `Pants` and `ShirtGraphic` children of a
//! character model, and where Roblox's 585x559 clothing template puts each
//! face of each limb.
//!
//! R6 limbs are boxes, so their faces are decals cut out of the template
//! (see [`super::part`]); R15 limbs are meshes whose UVs do not follow the
//! template, so `scene::filemesh` re-maps them onto it.

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};

use super::face::{NormalId, Projection};
use super::part::asset_uri;

/// The template's size in pixels.
pub(crate) const TEMPLATE: [f32; 2] = [585.0, 559.0];

/// The part of a character a garment is cut for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Limb {
    Torso,
    RightArm,
    LeftArm,
    RightLeg,
    LeftLeg,
}

/// One garment: the instance that holds it, its image, and its `Color3`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Layer {
    pub(crate) referent: Ref,
    pub(crate) image: AssetRef,
    pub(crate) tint: [f32; 3],
}

/// What a character wears, lowest layer first when drawn: pants, then shirt,
/// then the T-shirt graphic on the chest.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Wardrobe {
    pub(crate) pants: Option<Layer>,
    pub(crate) shirt: Option<Layer>,
    pub(crate) graphic: Option<Layer>,
}

impl Wardrobe {
    /// The clothes among `model`'s children; `None` when it wears none.
    pub(crate) fn of(dom: &WeakDom, model: Ref) -> Option<Wardrobe> {
        let mut wardrobe = Wardrobe::default();
        for &child in dom.get(model)?.children() {
            let Some(instance) = dom.get(child) else {
                continue;
            };
            let (slot, key) = match instance.class() {
                "Pants" => (&mut wardrobe.pants, "PantsTemplate"),
                "Shirt" => (&mut wardrobe.shirt, "ShirtTemplate"),
                "ShirtGraphic" => (&mut wardrobe.graphic, "Graphic"),
                _ => continue,
            };
            let properties = instance.properties();
            let Some(image) = properties
                .get(key)
                .and_then(asset_uri)
                .and_then(|uri| AssetRef::parse(uri).ok())
                .filter(|image| *image != AssetRef::Empty)
            else {
                continue;
            };
            let tint = match properties.get("Color3") {
                Some(&Variant::Color3(color)) => [color.r, color.g, color.b],
                _ => [1.0; 3],
            };
            slot.get_or_insert(Layer {
                referent: child,
                image,
                tint,
            });
        }
        (wardrobe != Wardrobe::default()).then_some(wardrobe)
    }

    /// What `limb` is actually dressed in, bottom layer first, with the
    /// T-shirt graphic last.
    pub(crate) fn for_limb(&self, limb: Limb) -> Vec<(Garment, &Layer)> {
        let pants = self.pants.as_ref().filter(|_| limb.takes_pants());
        let shirt = self.shirt.as_ref().filter(|_| limb.takes_shirt());
        let graphic = self.graphic.as_ref().filter(|_| limb == Limb::Torso);
        [
            (Garment::Pants, pants),
            (Garment::Shirt, shirt),
            (Garment::Graphic, graphic),
        ]
        .into_iter()
        .filter_map(|(garment, layer)| Some((garment, layer?)))
        .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Garment {
    Pants,
    Shirt,
    /// A T-shirt's image, which fills the front of the torso rather than
    /// following the template.
    Graphic,
}

impl Limb {
    /// The limb an R6 part of this name is.
    pub(crate) fn r6(name: &str) -> Option<Limb> {
        Some(match name {
            "Torso" => Limb::Torso,
            "Right Arm" => Limb::RightArm,
            "Left Arm" => Limb::LeftArm,
            "Right Leg" => Limb::RightLeg,
            "Left Leg" => Limb::LeftLeg,
            _ => return None,
        })
    }

    /// The limb an R15 part of this name is a section of.
    pub(crate) fn r15(name: &str) -> Option<Limb> {
        Some(match name {
            "UpperTorso" | "LowerTorso" => Limb::Torso,
            "RightUpperArm" | "RightLowerArm" | "RightHand" => Limb::RightArm,
            "LeftUpperArm" | "LeftLowerArm" | "LeftHand" => Limb::LeftArm,
            "RightUpperLeg" | "RightLowerLeg" | "RightFoot" => Limb::RightLeg,
            "LeftUpperLeg" | "LeftLowerLeg" | "LeftFoot" => Limb::LeftLeg,
            _ => return None,
        })
    }

    fn takes_shirt(self) -> bool {
        !matches!(self, Limb::RightLeg | Limb::LeftLeg)
    }

    fn takes_pants(self) -> bool {
        !matches!(self, Limb::RightArm | Limb::LeftArm)
    }

    /// Where `face` of this limb sits in the template, as `[x, y, w, h]` in
    /// pixels.
    pub(crate) fn rect(self, face: NormalId) -> [f32; 4] {
        // The right limbs run left to right Left, Back, Right, Front; the
        // left limbs Front, Left, Back, Right, both under their own cap.
        let (cap_x, sides_x) = match self {
            Limb::Torso => {
                return match face {
                    NormalId::Top => [231.0, 8.0, 128.0, 64.0],
                    NormalId::Bottom => [231.0, 204.0, 128.0, 64.0],
                    NormalId::Front => [231.0, 74.0, 128.0, 128.0],
                    NormalId::Back => [427.0, 74.0, 128.0, 128.0],
                    NormalId::Right => [165.0, 74.0, 64.0, 128.0],
                    NormalId::Left => [361.0, 74.0, 64.0, 128.0],
                }
            }
            Limb::RightArm | Limb::RightLeg => (217.0, [19.0, 85.0, 151.0, 217.0]),
            Limb::LeftArm | Limb::LeftLeg => (308.0, [374.0, 440.0, 506.0, 308.0]),
        };
        // Sides in the order Left, Back, Right, Front.
        let side = |index: usize| [sides_x[index], 355.0, 64.0, 128.0];
        match (self, face) {
            (_, NormalId::Top) => [cap_x, 289.0, 64.0, 64.0],
            (_, NormalId::Bottom) => [cap_x, 485.0, 64.0, 64.0],
            (_, NormalId::Left) => side(0),
            (_, NormalId::Back) => side(1),
            (_, NormalId::Right) => side(2),
            (_, NormalId::Front) => side(3),
        }
    }
}

/// Image right and image down on `face`, in the box's object space.
///
/// The template is folded around the body with its top cap above the front
/// and its bottom cap below it, so the caps are turned a half turn from the
/// default decal orientation — the front edge of a cap is the one that meets
/// the front of the limb. The sides are the decal orientation as it is.
pub(crate) fn axes(face: NormalId) -> (Vec3, Vec3, Vec3) {
    let (normal, u, v) = face.axes();
    match face {
        NormalId::Top => (normal, -Vec3::X, -Vec3::Z),
        NormalId::Bottom => (normal, -Vec3::X, Vec3::Z),
        _ => (normal, u, v),
    }
}

/// The cut of the template that lies on `face` of `limb`, as a projection
/// onto the unit box.
pub(crate) fn projection(limb: Limb, face: NormalId) -> Projection {
    let (normal, u, v) = axes(face);
    let [x, y, w, h] = limb.rect(face);
    Projection {
        normal,
        u,
        v,
        uv_scale: [w / TEMPLATE[0], h / TEMPLATE[1]],
        uv_offset: [x / TEMPLATE[0], y / TEMPLATE[1]],
    }
}

/// The whole image on `face`: what a T-shirt's graphic does on the chest.
pub(crate) fn whole(face: NormalId) -> Projection {
    let (normal, u, v) = axes(face);
    Projection {
        normal,
        u,
        v,
        uv_scale: [1.0, 1.0],
        uv_offset: [0.0, 0.0],
    }
}

pub(crate) const FACES: [NormalId; 6] = [
    NormalId::Right,
    NormalId::Top,
    NormalId::Back,
    NormalId::Left,
    NormalId::Bottom,
    NormalId::Front,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// The Roblox 585x559 template: the torso block, with the arm or leg
    /// blocks below it, each a cap over four 64x128 sides.
    #[test]
    fn rects_match_the_documented_template_regions() {
        assert_eq!(
            Limb::Torso.rect(NormalId::Front),
            [231.0, 74.0, 128.0, 128.0]
        );
        assert_eq!(
            Limb::Torso.rect(NormalId::Back),
            [427.0, 74.0, 128.0, 128.0]
        );
        assert_eq!(
            Limb::Torso.rect(NormalId::Right),
            [165.0, 74.0, 64.0, 128.0]
        );
        assert_eq!(Limb::Torso.rect(NormalId::Left), [361.0, 74.0, 64.0, 128.0]);
        assert_eq!(
            Limb::RightArm.rect(NormalId::Front),
            [217.0, 355.0, 64.0, 128.0]
        );
        assert_eq!(
            Limb::LeftArm.rect(NormalId::Front),
            [308.0, 355.0, 64.0, 128.0]
        );
        assert_eq!(
            Limb::RightLeg.rect(NormalId::Top),
            [217.0, 289.0, 64.0, 64.0]
        );
        assert_eq!(
            Limb::LeftLeg.rect(NormalId::Bottom),
            [308.0, 485.0, 64.0, 64.0]
        );
    }

    #[test]
    fn each_limbs_faces_sit_inside_the_template_and_apart() {
        for limb in [Limb::Torso, Limb::RightArm, Limb::LeftArm] {
            let rects: Vec<[f32; 4]> = FACES.iter().map(|&f| limb.rect(f)).collect();
            for (i, a) in rects.iter().enumerate() {
                assert!(a[0] + a[2] <= TEMPLATE[0] && a[1] + a[3] <= TEMPLATE[1]);
                for b in &rects[i + 1..] {
                    let apart = a[0] + a[2] <= b[0]
                        || b[0] + b[2] <= a[0]
                        || a[1] + a[3] <= b[1]
                        || b[1] + b[3] <= a[1];
                    assert!(apart, "{a:?} overlaps {b:?}");
                }
            }
        }
    }
}
