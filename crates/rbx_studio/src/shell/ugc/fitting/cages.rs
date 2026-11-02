//! A layered item's cages: the whole-body template they are made from (the
//! 15 Mannequin part cages stood where the preview body's parts stand, in
//! the handle's space), the item's own mesh in the same space, the cage
//! files in the local asset cache, and the body regions a nudge works on.

use glam::{Vec2, Vec3};
use rbx_autorig::{CageTemplate, ItemCages, Mesh, Part};
use rbx_ugc::Geometry;

use super::super::collect::mesh_geometry;
use super::generate::CageUris;
use crate::shell::rig::Cf;

/// Where the cage files go in the local asset cache, a folder per cage
/// pair named by its content so an edited cage never shows a stale mesh.
const CACHE_FOLDER: &str = "rbx-native-local/fitting";

/// A body part as the preview body stands it.
#[derive(Debug, Clone, Copy)]
pub(super) struct Placed {
    pub(super) part: Part,
    pub(super) frame: Cf,
    pub(super) size: Vec3,
}

fn point(cf: &Cf, v: Vec3) -> Vec3 {
    Vec3::from(cf.rotate(v.to_array())) + Vec3::from(cf.p)
}

/// The Mannequin's part cages as one mesh in the space of `handle`, each
/// stretched by its part's size over the template's and stood where the
/// part stands. Every vertex is kept, in `Part::ALL` order, seams unwelded,
/// UVs as the templates have them.
pub(super) fn template(
    templates: &[CageTemplate],
    placed: &[Placed],
    handle: Cf,
) -> Result<Mesh, String> {
    let back = handle.inverse();
    let mut out = Mesh::default();
    for part in Part::ALL {
        let t = templates
            .iter()
            .find(|t| t.part == part)
            .ok_or_else(|| format!("The Mannequin has no {} cage", part.name()))?;
        let p = placed
            .iter()
            .find(|p| p.part == part)
            .ok_or_else(|| format!("The test body has no {}", part.name()))?;
        let stretch = Vec3::select(t.size.cmpgt(Vec3::ZERO), p.size / t.size, Vec3::ONE);
        let local = back.mul(&p.frame);
        let base = out.positions.len() as u32;
        out.positions
            .extend(t.mesh.positions.iter().map(|&v| point(&local, v * stretch)));
        // Normals go through the inverse transpose: the stretch is
        // diagonal, so that is dividing by it, then the turn.
        out.normals.extend(
            t.mesh
                .normals
                .iter()
                .map(|&n| Vec3::from(local.rotate((n / stretch).to_array())).normalize_or_zero()),
        );
        out.uvs.extend(t.mesh.uvs.iter().copied());
        out.triangles
            .extend(t.mesh.triangles.iter().map(|tri| tri.map(|i| i + base)));
    }
    Ok(out)
}

/// The item's mesh file as the handle draws it: centred and stretched to
/// the handle's `size`, in studs, with area-weighted vertex normals (the
/// file's own would need the same stretch undone).
pub(super) fn item_mesh(bytes: &[u8], size: [f32; 3]) -> Result<Mesh, String> {
    let g = mesh_geometry(bytes, Some(size)).map_err(|e| e.to_string())?;
    let positions: Vec<Vec3> = g.positions.into_iter().map(Vec3::from).collect();
    Ok(Mesh {
        normals: normals(&positions, &g.triangles),
        positions,
        uvs: g.uvs.into_iter().map(Vec2::from).collect(),
        triangles: g.triangles,
        ..Mesh::default()
    })
}

/// Area-weighted vertex normals.
pub(super) fn normals(positions: &[Vec3], triangles: &[[u32; 3]]) -> Vec<Vec3> {
    let mut out = vec![Vec3::ZERO; positions.len()];
    for tri in triangles {
        let [a, b, c] = tri.map(|i| positions.get(i as usize).copied().unwrap_or_default());
        let n = (b - a).cross(c - a);
        for &i in tri {
            if let Some(slot) = out.get_mut(i as usize) {
                *slot += n;
            }
        }
    }
    out.into_iter().map(|n| n.normalize_or(Vec3::Y)).collect()
}

/// A mesh as the validator reads one.
pub(super) fn geometry(mesh: &Mesh) -> Geometry {
    Geometry {
        positions: mesh.positions.iter().map(|v| v.to_array()).collect(),
        triangles: mesh.triangles.clone(),
        uvs: mesh.uvs.iter().map(|v| v.to_array()).collect(),
        influences: Vec::new(),
        bones: Vec::new(),
        in_studs: false,
    }
}

/// The centre and size of `mesh`'s bounds.
pub(super) fn bounds(mesh: &Mesh) -> ([f32; 3], [f32; 3]) {
    let (lo, hi) = mesh.positions.iter().fold(
        (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)),
        |(lo, hi), &p| (lo.min(p), hi.max(p)),
    );
    if !lo.is_finite() {
        return ([0.0; 3], [0.0; 3]);
    }
    (((lo + hi) / 2.0).to_array(), (hi - lo).to_array())
}

/// Writes both cages of item `name` as `version 2.00` meshes into the
/// local asset cache and returns their URIs.
pub(super) fn write(name: &str, cages: &ItemCages) -> Result<CageUris, String> {
    let inner = rbx_autorig::mesh_v2(&cages.inner);
    let outer = rbx_autorig::mesh_v2(&cages.outer);
    let hash = crate::recovery::fnv1a(&inner).rotate_left(7) ^ crate::recovery::fnv1a(&outer);
    let cache = rbx_assets::AssetCache::new(None).map_err(|e| e.to_string())?;
    let put = |suffix: &str, bytes: &[u8]| -> Result<String, String> {
        let relative = format!("{CACHE_FOLDER}/{hash:016x}/{name}_{suffix}.mesh");
        cache
            .put_native(&relative, bytes)
            .map_err(|e| format!("caching {name}_{suffix}: {e}"))?;
        Ok(format!("rbxasset://{relative}"))
    };
    Ok(CageUris {
        inner: put("InnerCage", &inner)?,
        outer: put("OuterCage", &outer)?,
    })
}

/// The body regions a nudge is centred on, each at one part's centre.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Region {
    Head,
    Torso,
    LeftArm,
    RightArm,
    LeftLeg,
    RightLeg,
}

impl Region {
    pub(super) const ALL: [Region; 6] = [
        Region::Head,
        Region::Torso,
        Region::LeftArm,
        Region::RightArm,
        Region::LeftLeg,
        Region::RightLeg,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Region::Head => "Head",
            Region::Torso => "Torso",
            Region::LeftArm => "Left Arm",
            Region::RightArm => "Right Arm",
            Region::LeftLeg => "Left Leg",
            Region::RightLeg => "Right Leg",
        }
    }

    /// The part whose centre the region is: the middle of a limb.
    pub(super) fn part(self) -> Part {
        match self {
            Region::Head => Part::Head,
            Region::Torso => Part::UpperTorso,
            Region::LeftArm => Part::LeftLowerArm,
            Region::RightArm => Part::RightLowerArm,
            Region::LeftLeg => Part::LeftLowerLeg,
            Region::RightLeg => Part::RightLowerLeg,
        }
    }

    /// The region's centre in the space of `handle`.
    pub(super) fn centre(self, placed: &[Placed], handle: Cf) -> Option<Vec3> {
        let p = placed.iter().find(|p| p.part == self.part())?;
        Some(Vec3::from(handle.inverse().mul(&p.frame).p))
    }
}

#[cfg(test)]
#[path = "cages/tests.rs"]
mod tests;
