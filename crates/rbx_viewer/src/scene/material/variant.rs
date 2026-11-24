//! `MaterialVariant`: a place's own texture set substituted for a built-in
//! material, and what its colour map's alpha does under Studio's
//! "MaterialVariant Alpha Mode" beta.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use rbx_assets::AssetRef;
use rbx_dom::Variant;
use rbx_materials::{MapKind, Set};
use rbx_reflection::ReflectionDatabase;

use super::{builtin, property_of, Def, Kind, DEFAULT_MATERIAL, MATERIAL_ENUM};
use crate::textures::asset_uri;

/// `Enum.AlphaMode` as a `MaterialVariant` stores it.
const ALPHA_OVERLAY: u32 = 0;
const ALPHA_TRANSPARENCY: u32 = 1;
const ALPHA_TINT_MASK: u32 = 2;

/// Studio's "MaterialVariant Alpha Mode" beta, which this renderer follows
/// only when the host switches it on (see [`set_variant_alpha_modes`]).
static VARIANT_ALPHA_MODES: AtomicBool = AtomicBool::new(false);

/// Whether a `MaterialVariant`'s `AlphaMode` decides what its colour map's
/// alpha does, as with Studio's "MaterialVariant Alpha Mode" beta on. Off,
/// every variant draws as Studio draws it with the beta off: the alpha is
/// ignored and the map tints the part's colour everywhere, whatever the
/// property says. Read when a scene is built, so a change applies from the
/// next load or reload.
pub fn set_variant_alpha_modes(on: bool) {
    VARIANT_ALPHA_MODES.store(on, Ordering::Relaxed);
}

/// What a `MaterialVariant`'s colour map alpha does to the surface, measured
/// against Studio 0.748 with its "MaterialVariant Alpha Mode" beta on. The
/// discriminants ride in the instance buffer beside [`Kind`] (see
/// `renderer::instance`), so they have to match the `VARIANT_ALPHA_*`
/// constants in `renderer/material.wgsl`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum VariantAlpha {
    /// The alpha does nothing and the map tints the part's colour all over:
    /// `Opaque`, a built-in material, and every variant with the beta off.
    #[default]
    Ignored = 0,
    /// The part's own colour shows where the map's alpha dips, and the map
    /// itself, untinted, where it is opaque.
    Overlay = 1,
    /// Coloured as [`VariantAlpha::TintMask`], and cut out (not blended)
    /// wherever the alpha is below one half.
    Transparency = 2,
    /// The alpha says where the part's colour tints the map: fully where it
    /// is opaque, not at all where it is clear.
    TintMask = 3,
}

/// Whether the host switched alpha modes on, see [`set_variant_alpha_modes`].
pub(super) fn alpha_modes() -> bool {
    VARIANT_ALPHA_MODES.load(Ordering::Relaxed)
}

/// One `MaterialVariant`: its own maps over its `BaseMaterial`'s flavour.
///
/// Maps it leaves empty stay empty rather than falling back to the base
/// material's own pack, Studio requires a colour and a normal map to create
/// one, so a missing map is the author saying "none", not "inherit".
pub(super) fn variant(
    properties: &BTreeMap<String, Variant>,
    database: &ReflectionDatabase,
    alpha_modes: bool,
) -> Def {
    let base = match properties.get("BaseMaterial") {
        Some(&Variant::Enum(value)) => database
            .enum_name(MATERIAL_ENUM, value)
            .unwrap_or(DEFAULT_MATERIAL),
        _ => DEFAULT_MATERIAL,
    };
    let studs_per_tile = match properties.get("StudsPerTile") {
        Some(&Variant::Float32(studs)) if studs > 0.0 => studs,
        Some(&Variant::Float64(studs)) if studs > 0.0 => studs as f32,
        _ => rbx_materials::DEFAULT_STUDS_PER_TILE,
    };
    let maps = MapKind::ALL.map(|kind| {
        properties
            .get(property_of(kind))
            .and_then(asset_uri)
            .and_then(|uri| AssetRef::parse(uri).ok())
            .filter(|reference| *reference != AssetRef::Empty)
    });
    // A file that leaves `AlphaMode` out holds the default, Opaque, which
    // is also every value this table does not name.
    let alpha = match properties.get("AlphaMode") {
        Some(&Variant::Enum(ALPHA_OVERLAY)) if alpha_modes => VariantAlpha::Overlay,
        Some(&Variant::Enum(ALPHA_TRANSPARENCY)) if alpha_modes => VariantAlpha::Transparency,
        Some(&Variant::Enum(ALPHA_TINT_MASK)) if alpha_modes => VariantAlpha::TintMask,
        _ => VariantAlpha::Ignored,
    };

    Def {
        // A variant is textured by definition; only its base material's special
        // flavours (Neon, ForceField, Glass) survive the substitution.
        kind: match builtin(base, Set::Current).kind {
            Kind::Plastic | Kind::Textured => Kind::Textured,
            flavored => flavored,
        },
        studs_per_tile,
        maps,
        alpha,
    }
}
