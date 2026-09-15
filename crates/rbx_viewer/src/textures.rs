//! Pulls the painted surfaces out of a DOM: `Decal`/`Texture` faces on parts,
//! and a `Sky`'s six panels plus its sun and moon discs.
//!
//! Everything here is pure geometry — [`plan`] never touches the network. The
//! images themselves are fetched separately by [`crate::assets`], and the two
//! halves are joined by [`Decor::assemble`].

mod decor;
mod face;
mod part;
mod sky;

pub(crate) use decor::{Celestial, Decor, Group, Panel};
pub(crate) use face::{NormalId, Projection};
pub(crate) use part::asset_uri;
pub(crate) use sky::stars::Star;
pub(crate) use sky::Body;

use std::collections::HashMap;

use glam::Mat4;
use rbx_assets::AssetRef;
use rbx_dom::{Ref, WeakDom};
use rbx_reflection::ReflectionDatabase;

use crate::scene::{self, Placement, ShapeKind};

/// Winding of the two triangles of a quad whose corners are given in image
/// order (top-left, top-right, bottom-right, bottom-left).
pub(crate) const QUAD_INDICES: [u16; 6] = [0, 3, 2, 0, 2, 1];

const SKY_CLASS: &str = "Sky";

/// A textured rectangle: four corners in image order, with their UVs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Quad {
    pub(crate) positions: [[f32; 3]; 4],
    pub(crate) uvs: [[f32; 2]; 4],
    pub(crate) normal: [f32; 3],
}

/// One `Decal`/`Texture` ready to be drawn as a projection on its part's own
/// unit mesh: which mesh (`kind`), where it stands (`model`), and how the image
/// lands on it ([`Projection`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FaceInstance {
    /// The `Decal`/`Texture` instance this was built from — not read when
    /// drawing, only so a Properties-panel edit to its part can find and
    /// rewrite this one instance's GPU record again (see
    /// `renderer::textured::Textured::sync`) instead of only ever getting it
    /// right on a full reload.
    pub(crate) referent: Ref,
    pub(crate) kind: ShapeKind,
    pub(crate) model: Mat4,
    pub(crate) projection: Projection,
    pub(crate) tint: [f32; 3],
    pub(crate) alpha: f32,
}

/// What a DOM asks to be painted, before any image exists.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Plan {
    faces: Vec<(AssetRef, FaceInstance)>,
    sky: Option<Vec<(AssetRef, Quad)>>,
    bodies: Vec<(AssetRef, Body)>,
    /// How many stars the `Sky` asks for; the field itself is generated in
    /// [`Decor::assemble`], since no asset is involved.
    stars: u32,
}

impl Plan {
    /// Every distinct asset the plan needs, in first-seen order so the download
    /// progress line advances in the same order twice in a row.
    pub(crate) fn references(&self) -> Vec<AssetRef> {
        let mut seen = Vec::new();
        let all = self
            .faces
            .iter()
            .map(|(reference, _)| reference)
            .chain(self.sky.iter().flatten().map(|(reference, _)| reference))
            .chain(self.bodies.iter().map(|(reference, _)| reference));
        for reference in all {
            if !seen.contains(reference) {
                seen.push(reference.clone());
            }
        }
        seen
    }
}

/// Walks a DOM and works out everything it wants painted.
///
/// Walking the DOM rather than `placements` keeps the asset order stable from
/// one run to the next, which a hash map would not. Workspace-scoped, same as
/// `Scene::from_dom`'s own part build: `placements` only ever carries
/// `Workspace` parts, so this would filter out anything else anyway — scoping
/// the walk itself just skips visiting it in the first place.
pub(crate) fn plan(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    placements: &HashMap<Ref, Placement>,
) -> Plan {
    let faces = scene::workspace_descendants(dom, database)
        .filter_map(|referent| Some((referent, placements.get(&referent)?)))
        .flat_map(|(referent, placement)| part::faces(dom, database, referent, placement))
        .collect();

    // The sun and moon hang off the same `Sky` the panels come from, so a place
    // with no `Sky` gets neither; its panels, though, fall back to Roblox's own
    // default skybox, which is what Studio shows for such a place.
    let sky = scene::descendants(dom).find(|&referent| is_sky(dom, database, referent));

    Plan {
        faces,
        sky: Some(sky::default::panels_or_default(dom, sky)),
        bodies: sky
            .map(|referent| sky::celestial_bodies(dom, referent))
            .unwrap_or_default(),
        stars: sky.map_or(0, |referent| sky::star_count(dom, referent)),
    }
}

/// Re-derives one part's face instances against a freshly patched
/// [`Placement`] — the Properties-panel fast path's decal counterpart to
/// [`plan`], called for exactly the one part `Scene::patch_part` just
/// recomputed rather than walking the whole DOM again.
pub(crate) fn faces(
    dom: &WeakDom,
    database: &ReflectionDatabase,
    referent: Ref,
    placement: &Placement,
) -> Vec<(AssetRef, FaceInstance)> {
    part::faces(dom, database, referent, placement)
}

fn is_sky(dom: &WeakDom, database: &ReflectionDatabase, referent: Ref) -> bool {
    dom.get(referent)
        .is_some_and(|instance| database.is_subclass_of(instance.class(), SKY_CLASS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::Scene;
    use crate::textures::sky::SKY_FACES;
    use glam::Vec3;
    use rbx_dom::{CFrameData, Instance, Variant, Vector3Data};

    const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];

    fn database() -> ReflectionDatabase {
        ReflectionDatabase::embedded()
    }

    /// Plans a DOM the way the viewer does: every part the scene still draws
    /// offers the placement its face instances are projected onto.
    fn planned(dom: &WeakDom) -> Plan {
        let database = database();
        let placements = Scene::from_dom(dom, &database)
            .map(|scene| scene.placements())
            .unwrap_or_default();
        plan(dom, &database, &placements)
    }

    /// A part at the origin carrying the given face instances, each described as
    /// (class, texture uri, face ordinal, studs per tile).
    fn dom_with(size: [f32; 3], faces: &[(&str, &str, u32, Option<f32>)]) -> WeakDom {
        let mut dom = WeakDom::new();
        // `Scene::from_dom` and `textures::plan` both only look under
        // `Workspace` now (see their doc comments).
        let workspace = Ref::new(9000);
        dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
        dom.set_parent(workspace, None);
        let part_ref = Ref::new(1);
        let mut part = Instance::new(part_ref, "Part", "Part");
        part.properties_mut().insert(
            "size".to_string(),
            Variant::Vector3(Vector3Data {
                x: size[0],
                y: size[1],
                z: size[2],
            }),
        );
        part.properties_mut().insert(
            "CFrame".to_string(),
            Variant::CFrame(CFrameData {
                position: Vector3Data {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
                rotation: IDENTITY_ROTATION,
            }),
        );
        dom.insert(part);
        dom.set_parent(part_ref, Some(workspace));

        for (offset, (class, uri, face, studs)) in faces.iter().enumerate() {
            let child_ref = Ref::new(u32::try_from(offset).unwrap() + 2);
            let mut child = Instance::new(child_ref, *class, *class);
            let properties = child.properties_mut();
            properties.insert("Texture".to_string(), Variant::String(uri.to_string()));
            properties.insert("Face".to_string(), Variant::Enum(*face));
            if let Some(studs) = studs {
                properties.insert("StudsPerTileU".to_string(), Variant::Float32(*studs));
                properties.insert("StudsPerTileV".to_string(), Variant::Float32(*studs));
            }
            dom.insert(child);
            dom.set_parent(child_ref, Some(part_ref));
        }
        dom
    }

    #[test]
    fn collects_one_instance_per_face_instance() {
        let dom = dom_with(
            [8.0, 8.0, 8.0],
            &[
                ("Texture", "rbxassetid://1", 1, Some(4.0)),
                ("Decal", "rbxassetid://2", 5, None),
            ],
        );

        let plan = planned(&dom);

        assert_eq!(plan.faces.len(), 2);
        // The tiled one covers 8/4 = 2 tiles per axis, the stretched one covers 1.
        assert_eq!(plan.faces[0].1.projection.uv_scale, [2.0, 2.0]);
        assert_eq!(plan.faces[1].1.projection.uv_scale, [1.0, 1.0]);
    }

    #[test]
    fn repeated_assets_are_listed_once_in_reference_order() {
        let dom = dom_with(
            [4.0, 4.0, 4.0],
            &[
                ("Texture", "rbxassetid://7", 0, Some(4.0)),
                ("Texture", "rbxassetid://9", 1, Some(4.0)),
                ("Texture", "rbxassetid://7", 2, Some(4.0)),
            ],
        );

        let references = planned(&dom).references();

        // The default sky's six native panels follow the face images.
        let ids: Vec<&AssetRef> = references
            .iter()
            .filter(|reference| matches!(reference, AssetRef::Id(_)))
            .collect();
        assert_eq!(ids, vec![&AssetRef::Id(7), &AssetRef::Id(9)]);
        assert_eq!(references.len(), 2 + 6);
    }

    #[test]
    fn unusable_face_instances_are_dropped_rather_than_drawn_blank() {
        let dom = dom_with(
            [4.0, 4.0, 4.0],
            &[
                ("Decal", "", 0, None),
                ("Decal", "not-a-uri", 1, None),
                ("Decal", "rbxassetid://5", 99, None),
            ],
        );

        assert!(planned(&dom).faces.is_empty());
    }

    #[test]
    fn a_fully_transparent_face_is_not_collected() {
        let mut dom = dom_with([4.0, 4.0, 4.0], &[("Decal", "rbxassetid://5", 0, None)]);
        dom.get_mut(Ref::new(2))
            .unwrap()
            .properties_mut()
            .insert("Transparency".to_string(), Variant::Float32(1.0));

        assert!(planned(&dom).faces.is_empty());
    }

    #[test]
    fn an_instance_carries_its_part_own_shape_and_model_matrix() {
        let dom = dom_with([2.0, 6.0, 2.0], &[("Decal", "rbxassetid://5", 1, None)]);

        let face = planned(&dom).faces[0].1;

        // Face 1 is Top, and the model matrix is the one the part is drawn with:
        // the unit mesh scaled to 2x6x2 at the origin.
        assert_eq!(face.kind, ShapeKind::Box);
        assert_eq!(face.projection.normal, Vec3::Y);
        assert_eq!(
            face.model.transform_point3(Vec3::new(0.5, 0.5, 0.5)),
            Vec3::new(1.0, 3.0, 1.0)
        );
    }

    // The Properties-panel fast path's own re-derivation: `Scene::patch_part`
    // hands `faces` the part's new `Placement`, not a fresh DOM walk, and the
    // renderer finds the instance to rewrite again by this same referent (see
    // `renderer::textured::Textured::sync`) — so a patch that moved the part
    // has to keep naming the same Decal, not a fresh one.
    #[test]
    fn faces_re_derives_the_same_decal_against_a_patched_placement() {
        let dom = dom_with([2.0, 6.0, 2.0], &[("Decal", "rbxassetid://5", 1, None)]);
        let original = planned(&dom).faces[0].1;

        let moved = Placement {
            kind: ShapeKind::Box,
            model: Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0)) * original.model,
            size: Vec3::new(2.0, 6.0, 2.0),
        };
        let patched = faces(&dom, &database(), Ref::new(1), &moved);

        assert_eq!(patched.len(), 1);
        let face = patched[0].1;
        assert_eq!(face.referent, original.referent);
        assert_eq!(
            face.model.transform_point3(Vec3::new(0.5, 0.5, 0.5)),
            Vec3::new(11.0, 3.0, 1.0)
        );
    }

    // A `Shape` edit is one of the three the live-edit patch path has to
    // carry a decal through (`CFrame`/`Size`/`Shape`) — the new unit mesh has
    // to follow too, not just the matrix.
    #[test]
    fn faces_re_derives_a_patched_shape_change_too() {
        let dom = dom_with([4.0, 4.0, 4.0], &[("Decal", "rbxassetid://5", 0, None)]);

        let ball = Placement {
            kind: ShapeKind::Ball,
            model: Mat4::IDENTITY,
            size: Vec3::splat(4.0),
        };
        let patched = faces(&dom, &database(), Ref::new(1), &ball);

        assert_eq!(patched[0].1.kind, ShapeKind::Ball);
    }

    // The rule that keeps a resolved MeshPart's decal from hanging in the air
    // where its fallback box used to be: no placement, no projection.
    #[test]
    fn a_part_the_scene_no_longer_draws_contributes_no_faces() {
        let dom = dom_with([4.0, 4.0, 4.0], &[("Decal", "rbxassetid://5", 0, None)]);

        let plan = plan(&dom, &database(), &HashMap::new());

        assert!(plan.faces.is_empty());
    }

    fn is_default_sky(panels: &Option<Vec<(AssetRef, Quad)>>) -> bool {
        panels.as_ref().is_some_and(|panels| {
            panels.len() == 6
                && panels.iter().all(|(reference, _)| {
                    matches!(reference, AssetRef::Native(path) if path.starts_with("sky/sky512_"))
                })
        })
    }

    // Studio shows Roblox's own skybox for a place with no `Sky`, not a blank.
    #[test]
    fn a_dom_without_a_sky_plans_robloxs_default_skybox() {
        let dom = dom_with([4.0, 4.0, 4.0], &[]);

        assert!(is_default_sky(&planned(&dom).sky));
    }

    #[test]
    fn a_sky_missing_one_panel_is_refused_whole() {
        let mut dom = WeakDom::new();
        let sky_ref = Ref::new(1);
        let mut instance = Instance::new(sky_ref, "Sky", "Sky");
        for face in SKY_FACES.into_iter().take(5) {
            instance.properties_mut().insert(
                face.property().to_string(),
                Variant::String("rbxassetid://1".to_string()),
            );
        }
        dom.insert(instance);
        dom.set_parent(sky_ref, None);

        // Five panels and a hole would read as a bug; the default sky stands in.
        assert!(is_default_sky(&planned(&dom).sky));
    }

    #[test]
    fn a_complete_sky_plans_six_panels_in_face_order() {
        let mut dom = WeakDom::new();
        let sky_ref = Ref::new(1);
        let mut instance = Instance::new(sky_ref, "Sky", "Sky");
        for (offset, face) in SKY_FACES.into_iter().enumerate() {
            instance.properties_mut().insert(
                face.property().to_string(),
                Variant::String(format!("rbxassetid://{}", offset + 1)),
            );
        }
        dom.insert(instance);
        dom.set_parent(sky_ref, None);

        let sky = planned(&dom).sky.unwrap();

        assert_eq!(sky.len(), 6);
        let references: Vec<AssetRef> = sky.iter().map(|(r, _)| r.clone()).collect();
        assert_eq!(references, (1..=6).map(AssetRef::Id).collect::<Vec<_>>());
    }

    #[test]
    fn the_test_place_fixture_yields_its_decal_texture_and_skybox() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/tests/TestPlace.rbxl");
        let bytes = std::fs::read(&path).expect("fixture must be readable");
        let dom = rbx_binary::deserialize(&bytes).expect("fixture must parse");

        let plan = planned(&dom);

        // A SpawnLocation carrying one Decal and one Texture, under a Sky.
        assert_eq!(plan.faces.len(), 2);
        assert_eq!(plan.sky.as_ref().map(Vec::len), Some(6));
        assert!(plan
            .references()
            .contains(&AssetRef::Native("textures/SpawnLocation.png".to_string())));
    }
}
