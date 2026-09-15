//! Reading the `Decal` and `Texture` children of one BasePart.

use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::face::{self, Mapping, NormalId};
use super::FaceInstance;
use crate::scene::{self, Placement};

const DECAL_ANCESTOR: &str = "Decal";

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
        .filter(|&&child| is_face_instance(dom, database, child))
        .filter_map(|&child| painted(dom, child, placement))
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

    #[test]
    fn numbers_are_read_whatever_width_the_file_stored_them_at() {
        assert_eq!(number(Some(&Variant::Float32(8.0))), Some(8.0));
        assert_eq!(number(Some(&Variant::Float64(8.0))), Some(8.0));
        assert_eq!(number(Some(&Variant::Int32(8))), Some(8.0));
        assert_eq!(number(Some(&Variant::Bool(true))), None);
        assert_eq!(number(None), None);
    }
}
