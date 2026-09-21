//! Reading the `Decal` and `Texture` children of one BasePart — and the
//! `AdGui` children, which are the same thing in the only state this viewer
//! can draw them in.

use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::face::{self, Mapping, NormalId};
use super::FaceInstance;
use crate::scene::{self, Placement};

const DECAL_ANCESTOR: &str = "Decal";
/// The immersive-ad surface. Not a `SurfaceGui` (the two are siblings under
/// `SurfaceGuiBase`) and not a GUI tree at all: what it shows is served at
/// run time, and what it shows when nothing is served is one image on one
/// face — which is a `Decal` in all but name, so it is read as one here
/// rather than given a canvas of its own.
const AD_GUI_CLASS: &str = "AdGui";

/// Collects the face instances of one part, each projected onto the unit mesh
/// `placement` describes.
///
// A MeshPart's own MeshId/TextureID and its SurfaceAppearance are not read
// here: both belong to real mesh geometry, which `scene::filemesh` resolves and
// `renderer::filemesh` draws. A part still drawing its fallback box keeps that
// box bare rather than having a mesh texture plastered over it.
pub(super) fn faces(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    placement: &Placement,
) -> Vec<(AssetRef, FaceInstance)> {
    let Some(part) = dom.get(referent) else {
        return Vec::new();
    };

    part.children()
        .iter()
        .filter_map(|&child| {
            if is_face_instance(dom, database, child) {
                painted(dom, child, placement)
            } else if is_ad_gui(dom, database, child) {
                advertised(dom, child, referent, placement)
            } else {
                None
            }
        })
        .collect()
}

/// Asset references reach us either as a plain string or wrapped in a `Content`,
/// depending on how old the file's serializer was.
///
/// `pub(crate)`: `scene::filemesh` reads the same `MeshId`/`TextureID` shape off
/// `MeshPart`/`SpecialMesh` properties.
pub(crate) fn asset_uri(value: &Variant) -> Option<&str> {
    match value {
        Variant::String(text) => Some(text),
        Variant::Content(rbx_dom::Content::Uri(uri)) => Some(uri),
        _ => None,
    }
}

/// Turns one `Decal`/`Texture` into a projection onto its part's own mesh.
///
/// A `Texture` tiles every `StudsPerTile` studs while a `Decal` stretches; the
/// two are told apart by whether those properties are there at all, rather than
/// by class name, so a subclass nobody has written yet still lands on the right
/// branch.
fn painted(
    dom: &WeakDom,
    referent: Ref,
    placement: &Placement,
) -> Option<(AssetRef, FaceInstance)> {
    let properties = dom.get(referent)?.properties();

    let reference = AssetRef::parse(asset_uri(properties.get("Texture")?)?).ok()?;
    if reference == AssetRef::Empty {
        return None;
    }
    let Some(&Variant::Enum(raw_face)) = properties.get("Face") else {
        return None;
    };
    let face = NormalId::from_ordinal(raw_face)?;

    let mapping = match (
        number(properties.get("StudsPerTileU")),
        number(properties.get("StudsPerTileV")),
    ) {
        (Some(u), Some(v)) => Mapping::Tiled {
            studs: [u, v],
            offset: [
                number(properties.get("OffsetStudsU")).unwrap_or(0.0),
                number(properties.get("OffsetStudsV")).unwrap_or(0.0),
            ],
        },
        _ => Mapping::Stretched,
    };

    let alpha = 1.0
        - number(properties.get("Transparency"))
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
    // An invisible decal still costs a draw and a texture upload, so it is
    // dropped here rather than blended away to nothing on the GPU.
    if alpha <= 0.0 {
        return None;
    }

    Some((
        reference,
        FaceInstance {
            referent,
            kind: placement.kind,
            model: placement.model,
            // The part's own extent, not the raw `size` property: a mesh child's
            // Scale and a Ball's clamp to a true sphere both change what the unit
            // mesh actually spans, and a tiled Texture is measured in studs of
            // the surface it lies on.
            projection: face::projection(face, placement.kind, placement.size, mapping),
            tint: match properties.get("Color3") {
                Some(&Variant::Color3(color)) => {
                    [color.r, color.g, color.b].map(scene::srgb_to_linear)
                }
                _ => [1.0; 3],
            },
            alpha,
        },
    ))
}

/// An `AdGui`'s own fallback image, laid on the face it adorns.
///
/// Roblox documents an ad surface as showing "the creator-supplied
/// `FallbackImage`, or a default Roblox fallback image when that property is
/// empty" whenever no ad is available — which, in a viewer that serves no
/// ads, is always. The creator's image is drawn; Roblox's own default is not
/// this project's to ship, so an `AdGui` with no `FallbackImage` draws
/// nothing rather than a stand-in for someone else's artwork.
///
/// `Adornee` is honoured only where it names the part this is a child of:
/// the decor plan is a walk of each part's own children (see [`faces`]), so
/// an ad surface adorned to a part somewhere else in the tree is left
/// undrawn rather than drawn on the wrong one.
fn advertised(
    dom: &WeakDom,
    referent: Ref,
    part: Ref,
    placement: &Placement,
) -> Option<(AssetRef, FaceInstance)> {
    let properties = dom.get(referent)?.properties();
    if !matches!(properties.get("Enabled"), None | Some(Variant::Bool(true))) {
        return None;
    }
    if let Some(Variant::Ref(adornee)) = properties.get("Adornee") {
        if *adornee != part {
            return None;
        }
    }

    let reference = AssetRef::parse(asset_uri(properties.get("FallbackImage")?)?).ok()?;
    if reference == AssetRef::Empty {
        return None;
    }
    let face = match properties.get("Face") {
        Some(&Variant::Enum(raw)) => NormalId::from_ordinal(raw)?,
        // `SurfaceGuiBase.Face`'s own default.
        _ => NormalId::Front,
    };

    Some((
        reference,
        FaceInstance {
            referent,
            kind: placement.kind,
            model: placement.model,
            projection: face::projection(face, placement.kind, placement.size, Mapping::Stretched),
            tint: [1.0; 3],
            alpha: 1.0,
        },
    ))
}

fn is_ad_gui(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> bool {
    dom.get(referent)
        .is_some_and(|instance| database.is_subclass_of(instance.class(), AD_GUI_CLASS))
}

fn number(value: Option<&Variant>) -> Option<f32> {
    match value? {
        Variant::Float32(v) => Some(*v),
        Variant::Float64(v) => Some(*v as f32),
        Variant::Int32(v) => Some(*v as f32),
        _ => None,
    }
}

fn is_face_instance(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> bool {
    dom.get(referent)
        .is_some_and(|instance| database.is_subclass_of(instance.class(), DECAL_ANCESTOR))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_uri_is_read_from_a_string_or_a_content_property() {
        let text = Variant::String("rbxassetid://1".to_string());
        assert_eq!(asset_uri(&text), Some("rbxassetid://1"));

        let content = Variant::Content(rbx_dom::Content::Uri("rbxasset://sky/sun.jpg".to_string()));
        assert_eq!(asset_uri(&content), Some("rbxasset://sky/sun.jpg"));

        assert_eq!(asset_uri(&Variant::Float32(1.0)), None);
        assert_eq!(asset_uri(&Variant::Content(rbx_dom::Content::None)), None);
    }

    /// A place with an ad surface on it draws the creator's own fallback
    /// image on the face it adorns, since this viewer serves no ads — and
    /// draws nothing at all where the creator supplied none.
    #[test]
    fn an_ad_surface_draws_its_fallback_image_on_its_own_face() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "Billboard", Some(workspace));
        let ad = dom.new_instance("AdGui", "AdGui", Some(part));
        let database = ReflectionDatabase::embedded();
        let placement = Placement {
            kind: crate::scene::ShapeKind::Box,
            model: glam::Mat4::from_scale(glam::Vec3::new(20.0, 10.0, 1.0)),
            size: glam::Vec3::new(20.0, 10.0, 1.0),
        };

        assert!(
            faces(&dom, &database, part, &placement).is_empty(),
            "no fallback image, nothing to draw"
        );

        dom.set_property(
            ad,
            "FallbackImage",
            Variant::Content(rbx_dom::Content::Uri("rbxassetid://12345".to_string())),
        )
        .unwrap();
        // `NormalId.Right`, from the API dump.
        dom.set_property(ad, "Face", Variant::Enum(0)).unwrap();

        let drawn = faces(&dom, &database, part, &placement);
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].0, AssetRef::Id(12345));
        assert_eq!(drawn[0].1.referent, ad);
        assert_eq!(drawn[0].1.alpha, 1.0);
        // On the face it named, pointing out of the part's own +X.
        assert!((drawn[0].1.projection.normal - glam::Vec3::X).length() < 1e-5);
    }

    /// An ad surface adorned to another part belongs to that part, not to
    /// the one it happens to be parented under.
    #[test]
    fn an_ad_surface_adorned_elsewhere_is_not_drawn_here() {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "Holder", Some(workspace));
        let other = dom.new_instance("Part", "Other", Some(workspace));
        let ad = dom.new_instance("AdGui", "AdGui", Some(part));
        dom.set_property(
            ad,
            "FallbackImage",
            Variant::Content(rbx_dom::Content::Uri("rbxassetid://12345".to_string())),
        )
        .unwrap();
        dom.set_property(ad, "Adornee", Variant::Ref(other))
            .unwrap();

        let placement = Placement {
            kind: crate::scene::ShapeKind::Box,
            model: glam::Mat4::IDENTITY,
            size: glam::Vec3::ONE,
        };
        assert!(faces(&dom, &ReflectionDatabase::embedded(), part, &placement).is_empty());
    }

    #[test]
    fn numbers_are_read_whatever_width_the_file_stored_them_at() {
        assert_eq!(number(Some(&Variant::Float32(8.0))), Some(8.0));
        assert_eq!(number(Some(&Variant::Float64(8.0))), Some(8.0));
        assert_eq!(number(Some(&Variant::Int32(8))), Some(8.0));
        assert_eq!(number(Some(&Variant::Bool(true))), None);
        assert_eq!(number(None), None);
    }
}
