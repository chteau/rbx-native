//! The `SurfaceAppearance` child of a `MeshPart`: a PBR map set authored in the
//! mesh's own UVs, which replaces both the mesh's `TextureID` and the texture
//! pack the part's `BasePart.Material` would otherwise project onto it.
//!
//! Pure DOM extraction like the rest of `scene`; `renderer::filemesh::appearance`
//! uploads what this names.

use std::collections::HashMap;

use rbx_assets::AssetRef;
use rbx_dom::{Instance, Variant, WeakDom};
use rbx_materials::MapKind;

use crate::assets::Image;
use crate::scene::material::property_of;

const CLASS: &str = "SurfaceAppearance";
/// `Enum.AlphaMode.Transparency`. The other value, 0, is `Overlay`.
const TRANSPARENCY: u32 = 1;

/// What the `ColorMap`'s alpha channel is taken to mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum AlphaMode {
    /// The part's own `Color` shows through wherever the map is not opaque.
    #[default]
    Overlay,
    /// The alpha is real transparency and the surface has to be blended.
    Transparency,
}

/// One `SurfaceAppearance`, reduced to what the renderer needs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Appearance {
    /// The four maps in [`MapKind::ALL`] order, so the GPU side indexes them
    /// exactly as it indexes a material pack.
    pub(crate) maps: [Option<AssetRef>; 4],
    pub(crate) alpha_mode: AlphaMode,
    /// `SurfaceAppearance.Color`, linearized: a tint over the colour map.
    pub(crate) tint: [f32; 3],
}

impl Appearance {
    /// The same set with every map that failed to download dropped, so the GPU
    /// side only ever sees references it can upload.
    pub(super) fn resolved(&self, images: &HashMap<AssetRef, Image>) -> Appearance {
        let maps = self
            .maps
            .clone()
            .map(|map| map.filter(|reference| images.contains_key(reference)));
        // The alpha mode describes the colour map, so once that map is gone
        // there is nothing left to blend — and Overlay is the mode whose
        // neutral fallback leaves the part's own colour alone.
        let alpha_mode = match maps[MapKind::Color.index()] {
            Some(_) => self.alpha_mode,
            None => AlphaMode::Overlay,
        };

        Appearance {
            maps,
            alpha_mode,
            tint: self.tint,
        }
    }

    /// Whether its instances belong in the blended pass: only `Transparency`
    /// reads the colour map's alpha as transparency, and only a map that
    /// actually carries one changes anything.
    pub(crate) fn is_translucent(&self, images: &HashMap<AssetRef, Image>) -> bool {
        self.alpha_mode == AlphaMode::Transparency
            && self.maps[MapKind::Color.index()]
                .as_ref()
                .and_then(|reference| images.get(reference))
                .is_some_and(Image::has_alpha)
    }
}

/// Reads the first `SurfaceAppearance` child of a part.
///
/// Studio only ever creates one; a file carrying several (the fixture's `Head`
/// does) is resolved the way the engine resolves it, by keeping the first.
pub(super) fn of(dom: &WeakDom, part: &Instance) -> Option<Appearance> {
    let child = part.children().iter().find_map(|&referent| {
        let child = dom.get(referent)?;
        (child.class() == CLASS).then_some(child)
    })?;
    let properties = child.properties();

    // `TexturePack` is deliberately not read: it is the layered-clothing bundle
    // these four maps were unpacked from, so following it would paint twice.
    let maps = MapKind::ALL.map(|kind| {
        properties
            .get(property_of(kind))
            .and_then(super::parsed_asset_ref)
    });
    let alpha_mode = match properties.get("AlphaMode") {
        Some(&Variant::Enum(TRANSPARENCY)) => AlphaMode::Transparency,
        _ => AlphaMode::Overlay,
    };
    let tint = match properties.get("Color") {
        Some(&Variant::Color3(color)) => {
            [color.r, color.g, color.b].map(crate::scene::srgb_to_linear)
        }
        _ => [1.0; 3],
    };

    Some(Appearance {
        maps,
        alpha_mode,
        tint,
    })
}
