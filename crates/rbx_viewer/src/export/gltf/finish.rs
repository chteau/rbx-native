//! What `renderer/material.wgsl` does with the three materials no texture
//! captures, in the standard glTF extensions that say the most of it.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::super::{ExportMesh, Finish};

/// How many times its own colour a Neon surface emits in a glTF viewer. The
/// renderer's own radiance squares the colour (`NEON_EMISSIVE` in
/// `renderer/material.wgsl`), which a glTF emissive factor times a strength
/// cannot carry, so the export keeps the colour as it is at a strength that
/// still blooms in a viewer.
const NEON_STRENGTH: f32 = 6.0;
/// `material.wgsl`'s ForceField opacity is `alpha * mix(0.4, 2.0, rim)`
/// with `rim = (1 - |n·v|)²`: this is its face-on end, what a glTF viewer's
/// own Fresnel then raises towards the rim (see [`finish`]).
const FORCE_FIELD_FACE_OPACITY: f32 = 0.4;
/// `FORCE_FIELD_PATTERN_GLOW`: how bright a ForceField mesh's showing
/// pattern glows in its own colour.
const FORCE_FIELD_PATTERN_GLOW: f32 = 1.6;
/// How tight the sheen rim is: rough enough to spread over the outer band
/// of the shell as `rim`'s square does, not a hairline.
const FORCE_FIELD_SHEEN_ROUGHNESS: f32 = 0.5;
/// OBJ has no Fresnel and no rim, so the `.mtl` takes the shell averaged
/// over every angle a closed shell shows an eye (`|n·v|` spread as
/// `2c dc`): a mean rim of 1/6.
const FORCE_FIELD_MEAN_RIM: f32 = 1.0 / 6.0;
pub(in crate::export) const FORCE_FIELD_OPACITY: f32 =
    FORCE_FIELD_FACE_OPACITY + (2.0 - FORCE_FIELD_FACE_OPACITY) * FORCE_FIELD_MEAN_RIM;
pub(in crate::export) const FORCE_FIELD_GLOW: f32 = 2.2 * FORCE_FIELD_MEAN_RIM;
/// Roblox publishes no index of refraction for Glass, and the viewport's
/// refraction is a screen-space displacement with none either (see
/// `GLASS_REFRACTION` in `material.wgsl`); 1.5 is window glass, and the
/// extension's own default.
pub(in crate::export) const GLASS_IOR: f32 = 1.5;
/// `material.wgsl` keeps Glass "barely rougher than a mirror" when no
/// roughness map says otherwise.
const GLASS_ROUGHNESS: f32 = 0.05;

pub(super) fn finish(
    material: &mut Value,
    mesh: &ExportMesh,
    texture: &mut impl FnMut(usize) -> usize,
    used: &mut BTreeSet<&'static str>,
) {
    let [r, g, b, alpha] = mesh.color;
    match mesh.finish {
        Finish::Plain => {}
        // Unlit: the light it gives is all a Neon surface shows, so it
        // reflects none (black base) and emits its colour, times its image.
        Finish::Neon => {
            let pbr = &mut material["pbrMetallicRoughness"];
            pbr["baseColorFactor"] = json!([0.0, 0.0, 0.0, alpha]);
            if let Some(texture) = pbr
                .as_object_mut()
                .and_then(|pbr| pbr.remove("baseColorTexture"))
            {
                material["emissiveTexture"] = texture;
            }
            material["emissiveFactor"] = json!([r, g, b]);
            emissive_strength(material, NEON_STRENGTH, used);
        }
        // See-through by `Transparency` through transmission rather than
        // blending, which is what lets a viewer refract what is behind it.
        Finish::Glass => {
            used.insert("KHR_materials_transmission");
            used.insert("KHR_materials_ior");
            material["pbrMetallicRoughness"]["baseColorFactor"] = json!([r, g, b, 1.0]);
            if mesh.maps.metallic_roughness.is_none() {
                material["pbrMetallicRoughness"]["roughnessFactor"] = json!(GLASS_ROUGHNESS);
            }
            if let Some(material) = material.as_object_mut() {
                material.remove("alphaMode");
            }
            material["extensions"]["KHR_materials_transmission"] =
                json!({ "transmissionFactor": 1.0 - alpha });
            material["extensions"]["KHR_materials_ior"] = json!({ "ior": GLASS_IOR });
        }
        // A shell faint face-on and solid, glowing in its own colour, at
        // its rim. Transmission lets a viewer's own Fresnel do the
        // view-dependent part: the share it transmits falls towards
        // grazing angles, so the edge reads more solid, and a sheen in the
        // part's colour (KHR_materials_sheen, brightest at grazing angles)
        // lights the rim as `force_field_energy` does. What stays
        // approximate: the rim is lit sheen rather than `material.wgsl`'s
        // own 2.2× glow, and Fresnel's falloff is not `rim`'s square.
        Finish::ForceField => {
            used.insert("KHR_materials_transmission");
            used.insert("KHR_materials_sheen");
            if let Some(material) = material.as_object_mut() {
                material.remove("alphaMode");
            }
            // Transmission tints what is behind by the base colour, where
            // the viewport lays its colour over it at the face-on opacity:
            // `bg * (1 - o) + colour * o`. Against a backdrop about as
            // bright as the light, a tint of `mix(1, colour, o)` lands on
            // the same.
            let opacity = (alpha * FORCE_FIELD_FACE_OPACITY).min(1.0);
            let tint = |c: f32| 1.0 + (c - 1.0) * opacity;
            material["pbrMetallicRoughness"]["baseColorFactor"] =
                json!([tint(r), tint(g), tint(b), 1.0]);
            // A smooth shell: rough transmission would frost what is seen
            // through it, which `material.wgsl` never does.
            material["pbrMetallicRoughness"]["roughnessFactor"] = json!(GLASS_ROUGHNESS);
            let mut transmission = json!({ "transmissionFactor": 1.0 - opacity });
            // Where the mesh's pattern shows, the shell is solid and glows.
            if let Some(index) = mesh.maps.see_through {
                transmission["transmissionTexture"] = json!({ "index": texture(index) });
            }
            material["extensions"]["KHR_materials_transmission"] = transmission;
            material["extensions"]["KHR_materials_sheen"] = json!({
                "sheenColorFactor": [r, g, b],
                "sheenRoughnessFactor": FORCE_FIELD_SHEEN_ROUGHNESS,
            });
            if let Some(index) = mesh.maps.pattern {
                material["emissiveTexture"] = json!({ "index": texture(index) });
                material["emissiveFactor"] = json!([r, g, b]);
                emissive_strength(material, FORCE_FIELD_PATTERN_GLOW, used);
            }
        }
    }
}

fn emissive_strength(material: &mut Value, strength: f32, used: &mut BTreeSet<&'static str>) {
    // A black surface emits nothing however strong; the validator flags
    // the extension on a zero factor as dead weight.
    let lit = material["emissiveFactor"]
        .as_array()
        .is_some_and(|factor| factor.iter().any(|c| c.as_f64() != Some(0.0)));
    if strength != 1.0 && lit {
        used.insert("KHR_materials_emissive_strength");
        material["extensions"]["KHR_materials_emissive_strength"] =
            json!({ "emissiveStrength": strength });
    }
}
