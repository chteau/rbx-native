use std::collections::HashMap;
use std::sync::Arc;

use glam::{Mat4, Vec2, Vec3};
use rbx_assets::AssetRef;
use rbx_dom::{CFrameData, Instance, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::Pose;

fn test_place() -> WeakDom {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/TestPlace.rbxl");
    let bytes = std::fs::read(&path).expect("fixture must be readable");
    rbx_binary::deserialize(&bytes).expect("fixture must parse")
}

const FOV: f32 = 70.0;

fn pose(position: Vec3, yaw: f32, pitch: f32) -> Pose {
    Pose {
        position,
        yaw,
        pitch,
        fov_degrees: FOV,
        ortho_scale: 20.0,
    }
}

fn identity_cframe(x: f32, y: f32, z: f32) -> CFrameData {
    CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    }
}

#[test]
fn ndc_spans_minus_one_to_one_with_y_flipped() {
    let size = Vec2::new(800.0, 600.0);
    assert_eq!(ndc_of(Vec2::new(400.0, 300.0), size), Vec2::ZERO);
    // Top-left pixel is NDC (-1, +1): the y flip is the whole point.
    assert_eq!(ndc_of(Vec2::ZERO, size), Vec2::new(-1.0, 1.0));
    assert_eq!(ndc_of(size, size), Vec2::new(1.0, -1.0));
}

#[test]
fn a_zero_sized_viewport_does_not_divide_by_zero() {
    let ndc = ndc_of(Vec2::ZERO, Vec2::ZERO);
    assert!(ndc.x.is_finite() && ndc.y.is_finite());
}

#[test]
fn the_centre_ray_runs_straight_down_the_view_axis() {
    let pose = pose(Vec3::new(3.0, 4.0, 5.0), 0.0, 0.0);
    let ray = ray_through(pose.view_projection(false, 16.0 / 9.0), Vec2::ZERO);

    // Yaw and pitch of zero look down -Z (see `camera::direction`).
    assert!((ray.direction - Vec3::NEG_Z).length() < 1e-4);
    // The ray starts on the near plane, which is directly in front of the eye.
    assert!((ray.origin - pose.position).length() < 0.1);
}

#[test]
fn an_edge_ray_leaves_at_half_the_field_of_view() {
    let pose = pose(Vec3::ZERO, 0.0, 0.0);
    // A square frame, so the horizontal half-angle is the vertical one.
    let projection = pose.view_projection(false, 1.0);

    let right = ray_through(projection, Vec2::new(1.0, 0.0));
    assert!(right.direction.x > 0.0);
    assert!((right.direction.angle_between(Vec3::NEG_Z).to_degrees() - FOV * 0.5).abs() < 1e-2);

    let up = ray_through(projection, Vec2::new(0.0, 1.0));
    assert!(up.direction.y > 0.0);
    assert!((up.direction.angle_between(Vec3::NEG_Z).to_degrees() - FOV * 0.5).abs() < 1e-2);
}

#[test]
fn a_yawed_camera_turns_its_rays_with_it() {
    let turned = pose(Vec3::ZERO, std::f32::consts::FRAC_PI_2, 0.0);
    let ray = ray_through(turned.view_projection(false, 1.0), Vec2::ZERO);
    // Yawing a quarter turn swings the view from -Z round to -X.
    assert!((ray.direction - Vec3::NEG_X).length() < 1e-4);
}

#[test]
fn orthographic_rays_stay_parallel_and_move_their_origin_instead() {
    let pose = pose(Vec3::ZERO, 0.0, 0.0);
    let projection = pose.view_projection(true, 1.0);

    let centre = ray_through(projection, Vec2::ZERO);
    let right = ray_through(projection, Vec2::new(1.0, 0.0));

    assert!((centre.direction - right.direction).length() < 1e-4);
    // Half the view volume's width away, which at aspect 1 is `ortho_scale`.
    assert!((right.origin.x - centre.origin.x - pose.ortho_scale).abs() < 1e-2);
}

#[test]
fn a_ray_meets_a_unit_box_at_its_near_face() {
    let ray = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z);
    let hit = ray_hits_box(ray, Mat4::IDENTITY).expect("the ray points at the box");
    assert!((hit - 9.5).abs() < 1e-4);
}

#[test]
fn a_ray_that_misses_reports_nothing() {
    let ray = Ray::new(Vec3::new(5.0, 0.0, 10.0), Vec3::NEG_Z);
    assert!(ray_hits_box(ray, Mat4::IDENTITY).is_none());
}

#[test]
fn a_box_behind_the_ray_is_not_a_hit() {
    // Pointing away from a box that sits behind the origin: selecting by
    // clicking must never reach something the camera has already flown past.
    let ray = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::Z);
    assert!(ray_hits_box(ray, Mat4::IDENTITY).is_none());
}

#[test]
fn a_ray_starting_inside_the_box_hits_at_zero() {
    let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z);
    assert_eq!(ray_hits_box(ray, Mat4::IDENTITY), Some(0.0));
}

#[test]
fn the_box_follows_its_model_matrix() {
    let model = Mat4::from_translation(Vec3::new(0.0, 0.0, -20.0))
        * Mat4::from_scale(Vec3::new(4.0, 4.0, 2.0));
    let ray = Ray::new(Vec3::ZERO, Vec3::NEG_Z);
    let hit = ray_hits_box(ray, model).expect("the ray points at the box");
    // Centre 20 studs away, half a stud of its own depth in front of that.
    assert!((hit - 19.0).abs() < 1e-4);

    // Just outside the scaled box's half-width of 2 studs.
    let past = Ray::new(Vec3::new(2.5, 0.0, 0.0), Vec3::NEG_Z);
    assert!(ray_hits_box(past, model).is_none());
}

#[test]
fn a_rotated_box_is_hit_on_its_own_axes() {
    // Turned 45 degrees about Y, a 1-stud cube's corner reaches further along
    // Z than its face did: the hit has to be tested in the box's frame, not
    // the world's.
    let model = Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4);
    let ray = Ray::new(Vec3::new(0.0, 0.0, 10.0), Vec3::NEG_Z);
    let hit = ray_hits_box(ray, model).expect("the ray points at the box");
    assert!((hit - (10.0 - 0.5 * std::f32::consts::SQRT_2)).abs() < 1e-4);
}

#[test]
fn a_part_flattened_to_nothing_is_not_pickable() {
    let model = Mat4::from_scale(Vec3::new(4.0, 0.0, 4.0));
    let ray = Ray::new(Vec3::new(0.0, 5.0, 0.0), Vec3::NEG_Y);
    assert!(ray_hits_box(ray, model).is_none());
}

#[test]
fn a_part_model_carries_its_position_and_size() {
    let model = part_model(
        &identity_cframe(10.0, 2.0, -3.0),
        Vector3Data {
            x: 4.0,
            y: 1.0,
            z: 2.0,
        },
    );

    assert!((model.transform_point3(Vec3::ZERO) - Vec3::new(10.0, 2.0, -3.0)).length() < 1e-4);
    // The unit cube's corner lands half a size away on every axis.
    let corner = model.transform_point3(Vec3::splat(0.5));
    assert!((corner - Vec3::new(12.0, 2.5, -2.0)).length() < 1e-4);
}

#[test]
fn a_ray_meets_a_plane_where_it_crosses_it() {
    let ray = Ray::new(Vec3::new(0.0, 10.0, 0.0), Vec3::NEG_Y);
    let hit = ray_hits_plane(ray, Vec3::new(5.0, 3.0, 5.0), Vec3::Y).expect("it crosses");
    assert!((hit - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-4);
}

#[test]
fn a_click_down_at_the_place_finds_its_parts_nearest_first() {
    // TestPlace is a 2048x16x2048 baseplate centred at y=-8 with a 12x1x12
    // spawn standing on it at y=0.5, so a ray straight down the middle meets
    // the spawn first and the baseplate behind it.
    let dom = test_place();
    let database = ReflectionDatabase::embedded();
    let down = Ray::new(Vec3::new(0.0, 200.0, 0.0), Vec3::NEG_Y);

    let hits = parts_along(&dom, &database, &Meshes::default(), down);
    assert_eq!(hits.len(), 2, "the spawn and the baseplate");

    let names: Vec<&str> = hits
        .iter()
        .map(|&referent| dom.get(referent).expect("a hit resolves").name())
        .collect();
    assert_eq!(names[0], "SpawnLocation");
    assert_eq!(names[1], "Baseplate");
}

#[test]
fn a_click_off_the_edge_of_the_place_finds_nothing() {
    let dom = test_place();
    let database = ReflectionDatabase::embedded();
    let past = Ray::new(Vec3::new(5000.0, 200.0, 0.0), Vec3::NEG_Y);

    assert!(parts_along(&dom, &database, &Meshes::default(), past).is_empty());
}

#[test]
fn an_empty_dom_is_clickable_without_panicking() {
    let database = ReflectionDatabase::embedded();
    let down = Ray::new(Vec3::new(0.0, 200.0, 0.0), Vec3::NEG_Y);

    assert!(parts_along(&WeakDom::new(), &database, &Meshes::default(), down).is_empty());
}

// --- Real shapes through the DOM -------------------------------------------

/// A `Workspace` holding whatever `add` puts under it: a scene small enough
/// to reason about by hand.
fn workspace_with(add: impl FnOnce(&mut WeakDom, Ref)) -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = Ref::new(9000);
    dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
    dom.set_parent(workspace, None);
    add(&mut dom, workspace);
    dom
}

/// An axis-aligned `class` instance named `name`, centred at `position`.
fn add_part(
    dom: &mut WeakDom,
    parent: Ref,
    id: u32,
    class: &str,
    name: &str,
    position: Vec3,
    size: Vec3,
) -> Ref {
    let referent = Ref::new(id);
    let mut instance = Instance::new(referent, class, name);
    let properties = instance.properties_mut();
    properties.insert(
        "CFrame".to_string(),
        Variant::CFrame(identity_cframe(position.x, position.y, position.z)),
    );
    properties.insert(
        "size".to_string(),
        Variant::Vector3(Vector3Data {
            x: size.x,
            y: size.y,
            z: size.z,
        }),
    );
    dom.insert(instance);
    dom.set_parent(referent, Some(parent));
    referent
}

/// `Enum.PartType`: Ball=0, Cylinder=2.
const BALL: u32 = 0;
const CYLINDER: u32 = 2;

fn names(dom: &WeakDom, hits: &[Ref]) -> Vec<String> {
    hits.iter()
        .map(|&referent| {
            dom.get(referent)
                .expect("a hit resolves")
                .name()
                .to_string()
        })
        .collect()
}

/// The tetrahedron the plane `x + y + z = -0.5` cuts off the unit cube's
/// (-, -, -) corner: a mesh whose bounding box is almost entirely empty, so
/// a box test and a triangle test disagree nearly everywhere.
pub(super) fn corner_tetrahedron() -> rbx_mesh::Mesh {
    let vertex = |x: f32, y: f32, z: f32| rbx_mesh::Vertex {
        position: [x, y, z],
        normal: [0.0; 3],
        uv: [0.0; 2],
        color: [255; 4],
    };
    rbx_mesh::Mesh {
        version: (4, 1),
        vertices: vec![
            vertex(-0.5, -0.5, -0.5),
            vertex(0.5, -0.5, -0.5),
            vertex(-0.5, 0.5, -0.5),
            vertex(-0.5, -0.5, 0.5),
        ],
        indices: vec![0, 1, 2, 0, 1, 3, 0, 2, 3, 1, 2, 3],
        lods: Vec::new(),
        // The full unit cube, deliberately: a `MeshPart` is fitted to its
        // `size` by these bounds, so the empty corners stay empty.
        bounds: rbx_mesh::Aabb {
            min: [-0.5; 3],
            max: [0.5; 3],
        },
    }
}

fn tetrahedron_meshes(asset: AssetRef) -> Meshes {
    Meshes::new(HashMap::from([(asset, Arc::new(corner_tetrahedron()))]))
}

#[test]
fn a_thin_slab_in_front_of_a_ball_is_hit_first() {
    // A 4-stud ball and a slab standing at z = 1.5..1.7, inside the ball's
    // bounding box but outside its sphere where x = 1.5 (the surface there
    // is at z = sqrt(4 - 2.25) = 1.32). A box test would put the ball first.
    let dom = workspace_with(|dom, workspace| {
        let ball = add_part(
            dom,
            workspace,
            1,
            "Part",
            "Ball",
            Vec3::ZERO,
            Vec3::splat(4.0),
        );
        dom.set_property(ball, "shape", Variant::Enum(BALL))
            .expect("the ball exists");
        add_part(
            dom,
            workspace,
            2,
            "Part",
            "Slab",
            Vec3::new(0.0, 0.0, 1.6),
            Vec3::new(4.0, 4.0, 0.2),
        );
    });
    let database = ReflectionDatabase::embedded();
    let ray = Ray::new(Vec3::new(1.5, 0.0, 10.0), Vec3::NEG_Z);

    let box_only = |referent: u32| {
        ray_hits_box(ray, model_of(&dom, Ref::new(referent)).expect("a part")).expect("a box hit")
    };
    assert!(
        box_only(1) < box_only(2),
        "the boxes disagree with the shapes"
    );

    let hits = parts_along(&dom, &database, &Meshes::default(), ray);
    assert_eq!(names(&dom, &hits), ["Slab", "Ball"]);
}

#[test]
fn a_click_at_the_corner_of_a_balls_box_finds_nothing() {
    let dom = workspace_with(|dom, workspace| {
        let ball = add_part(
            dom,
            workspace,
            1,
            "Part",
            "Ball",
            Vec3::ZERO,
            Vec3::splat(4.0),
        );
        dom.set_property(ball, "shape", Variant::Enum(BALL))
            .expect("the ball exists");
    });
    let database = ReflectionDatabase::embedded();
    let corner = Ray::new(Vec3::new(1.9, 1.9, 10.0), Vec3::NEG_Z);
    assert!(ray_hits_box(corner, model_of(&dom, Ref::new(1)).expect("a part")).is_some());
    assert!(parts_along(&dom, &database, &Meshes::default(), corner).is_empty());

    let centre = Ray::new(Vec3::new(0.5, -0.5, 10.0), Vec3::NEG_Z);
    assert_eq!(
        names(
            &dom,
            &parts_along(&dom, &database, &Meshes::default(), centre)
        ),
        ["Ball"]
    );
}

#[test]
fn a_part_cylinder_is_picked_along_its_x_axis() {
    // 8 studs long along X, 2 across. Down Y at z = 0.95 the curved side is
    // there (radius 1); a cylinder standing along Y would be an ellipse 4 by
    // 1 in that plane and miss this ray entirely.
    let dom = workspace_with(|dom, workspace| {
        let pipe = add_part(
            dom,
            workspace,
            1,
            "Part",
            "Pipe",
            Vec3::ZERO,
            Vec3::new(8.0, 2.0, 2.0),
        );
        dom.set_property(pipe, "shape", Variant::Enum(CYLINDER))
            .expect("the pipe exists");
    });
    let database = ReflectionDatabase::embedded();

    let side = Ray::new(Vec3::new(3.5, 10.0, 0.95), Vec3::NEG_Y);
    assert_eq!(
        names(
            &dom,
            &parts_along(&dom, &database, &Meshes::default(), side)
        ),
        ["Pipe"]
    );

    // Along the axis at the corner of the end cap: box, not disc.
    let cap_corner = Ray::new(Vec3::new(10.0, 0.9, 0.9), Vec3::NEG_X);
    assert!(ray_hits_box(cap_corner, model_of(&dom, Ref::new(1)).expect("a part")).is_some());
    assert!(parts_along(&dom, &database, &Meshes::default(), cap_corner).is_empty());
}

#[test]
fn a_mesh_part_is_picked_against_its_downloaded_triangles() {
    let asset = AssetRef::Id(7);
    let dom = workspace_with(|dom, workspace| {
        let rock = add_part(
            dom,
            workspace,
            1,
            "MeshPart",
            "Rock",
            Vec3::ZERO,
            Vec3::splat(2.0),
        );
        dom.set_property(
            rock,
            "MeshId",
            Variant::String("rbxassetid://7".to_string()),
        )
        .expect("the rock exists");
    });
    let database = ReflectionDatabase::embedded();
    let meshes = tetrahedron_meshes(asset);

    // Through the empty corner of the 2-stud box: nothing there.
    let empty = Ray::new(Vec3::new(0.8, 0.8, 10.0), Vec3::NEG_Z);
    assert!(parts_along(&dom, &database, &meshes, empty).is_empty());
    // Where the tetrahedron actually is.
    let solid = Ray::new(Vec3::new(-0.8, -0.8, 10.0), Vec3::NEG_Z);
    assert_eq!(
        names(&dom, &parts_along(&dom, &database, &meshes, solid)),
        ["Rock"]
    );
}

#[test]
fn a_mesh_part_whose_mesh_never_downloaded_is_picked_as_its_box() {
    // The same empty-corner ray as above, with no mesh to test against: the
    // part is drawn as a box, so it is picked as one.
    let dom = workspace_with(|dom, workspace| {
        let rock = add_part(
            dom,
            workspace,
            1,
            "MeshPart",
            "Rock",
            Vec3::ZERO,
            Vec3::splat(2.0),
        );
        dom.set_property(
            rock,
            "MeshId",
            Variant::String("rbxassetid://7".to_string()),
        )
        .expect("the rock exists");
    });
    let database = ReflectionDatabase::embedded();
    let empty = Ray::new(Vec3::new(0.8, 0.8, 10.0), Vec3::NEG_Z);
    assert_eq!(
        names(
            &dom,
            &parts_along(&dom, &database, &Meshes::default(), empty)
        ),
        ["Rock"]
    );
}

#[test]
fn a_special_mesh_file_mesh_is_picked_at_its_own_scale_not_the_parts_size() {
    // A 10-stud part wearing a FileMesh scaled to 2 studs: the mesh, not the
    // part's box, is what is on screen.
    let asset = AssetRef::Id(7);
    let dom = workspace_with(|dom, workspace| {
        let host = add_part(
            dom,
            workspace,
            1,
            "Part",
            "Host",
            Vec3::ZERO,
            Vec3::splat(10.0),
        );
        let mesh = Ref::new(2);
        let mut child = Instance::new(mesh, "SpecialMesh", "Mesh");
        let properties = child.properties_mut();
        properties.insert("MeshType".to_string(), Variant::Enum(5));
        properties.insert(
            "MeshId".to_string(),
            Variant::String("rbxassetid://7".to_string()),
        );
        properties.insert(
            "Scale".to_string(),
            Variant::Vector3(Vector3Data {
                x: 2.0,
                y: 2.0,
                z: 2.0,
            }),
        );
        dom.insert(child);
        dom.set_parent(mesh, Some(host));
    });
    let database = ReflectionDatabase::embedded();
    let meshes = tetrahedron_meshes(asset);

    // Inside the part's 10-stud box, well outside the 2-stud mesh.
    let outside = Ray::new(Vec3::new(3.0, 3.0, 10.0), Vec3::NEG_Z);
    assert!(parts_along(&dom, &database, &meshes, outside).is_empty());
    assert_eq!(
        names(
            &dom,
            &parts_along(&dom, &database, &Meshes::default(), outside)
        ),
        ["Host"]
    );

    let inside = Ray::new(Vec3::new(-0.8, -0.8, 10.0), Vec3::NEG_Z);
    assert_eq!(
        names(&dom, &parts_along(&dom, &database, &meshes, inside)),
        ["Host"]
    );
}

#[test]
fn a_ray_parallel_to_a_plane_never_meets_it() {
    let ray = Ray::new(Vec3::new(0.0, 10.0, 0.0), Vec3::NEG_Z);
    assert!(ray_hits_plane(ray, Vec3::ZERO, Vec3::Y).is_none());
}

#[test]
fn a_part_stands_for_itself() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    let database = ReflectionDatabase::embedded();

    let selected = Selected::read(&dom, &database, part);
    assert!(selected.is_part());
    assert_eq!(selected.parts(), [part]);
}

/// What makes a selected `Model` transformable at all: it stands for the
/// geometry beneath it, however deeply that is buried, and the containers on
/// the way down are not geometry themselves.
#[test]
fn a_model_stands_for_every_part_beneath_it_however_deeply_nested() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Revolver", None);
    let barrel = dom.new_instance("MeshPart", "Barrel", Some(model));
    let group = dom.new_instance("Folder", "Group", Some(model));
    let grip = dom.new_instance("Part", "Grip", Some(group));
    dom.new_instance("Script", "Fire", Some(group));

    let database = ReflectionDatabase::embedded();
    let selected = Selected::read(&dom, &database, model);

    assert!(!selected.is_part());
    assert_eq!(selected.parts().len(), 2);
    assert!(selected.parts().contains(&barrel) && selected.parts().contains(&grip));
}

/// The one case that still resolves to nothing, and should: there is no
/// geometry under it to outline or drag.
#[test]
fn a_container_with_no_geometry_stands_for_nothing() {
    let mut dom = WeakDom::new();
    let folder = dom.new_instance("Folder", "Scripts", None);
    dom.new_instance("Script", "Main", Some(folder));

    let database = ReflectionDatabase::embedded();
    assert!(Selected::read(&dom, &database, folder).parts().is_empty());
}

/// A `BasePart` is a part however much is parented under it — a welded
/// assembly, or a `Tool`'s `Handle` with something screwed onto it. Reading
/// that off the number of parts resolved for it made such a part a
/// *container*, and outlined it with a loose world-axis-aligned box around
/// itself and its child instead of the tight one every other part gets.
#[test]
fn a_part_with_parts_under_it_is_still_a_part() {
    let mut dom = WeakDom::new();
    let handle = dom.new_instance("Part", "Handle", None);
    dom.new_instance("MeshPart", "Sight", Some(handle));

    let database = ReflectionDatabase::embedded();
    let selected = Selected::read(&dom, &database, handle);

    assert!(selected.is_part());
    // And it stands for itself alone: nothing in Roblox moves a child part
    // because its parent part moved, so a drag must not carry the child.
    assert_eq!(selected.parts(), [handle]);
}

/// Selecting a `Model` and something inside it names the same geometry twice
/// — a second box drawn over the first, and a group drag moving the shared
/// part twice as far as the gizmo travelled.
#[test]
fn a_model_selected_with_its_own_child_covers_it_once() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Revolver", None);
    let barrel = dom.new_instance("Part", "Barrel", Some(model));
    let grip = dom.new_instance("Part", "Grip", Some(model));
    let database = ReflectionDatabase::embedded();

    // Either order: which one the user ctrl-clicked first says nothing about
    // which box spans the other.
    for referents in [[model, grip], [grip, model]] {
        let entries = selection(&dom, &database, &referents);
        assert_eq!(entries.len(), 1, "{referents:?}");
        assert_eq!(entries[0].referent(), model);
        assert_eq!(entries[0].parts().len(), 2);
        assert!(entries[0].parts().contains(&barrel));
    }
}

/// Two selections that do not contain one another are both kept — the dedup
/// is about covered geometry, not about trimming the selection.
#[test]
fn two_unrelated_selections_are_both_kept() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "House", Some(workspace));
    dom.new_instance("Part", "Wall", Some(model));
    let loose = dom.new_instance("Part", "Baseplate", Some(workspace));

    let database = ReflectionDatabase::embedded();
    let entries = selection(&dom, &database, &[model, loose]);

    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].referent(), loose);
}

/// The same instance named twice keeps the first mention, which is the one
/// Scale and Rotate take their anchor from.
#[test]
fn the_same_instance_twice_is_one_entry() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);

    let database = ReflectionDatabase::embedded();
    assert_eq!(selection(&dom, &database, &[part, part]).len(), 1);
}

/// An empty container covers nothing, so it can neither swallow another entry
/// nor be swallowed by one.
#[test]
fn an_empty_container_neither_covers_nor_is_covered() {
    let mut dom = WeakDom::new();
    let empty = dom.new_instance("Folder", "Scripts", None);
    let part = dom.new_instance("Part", "Part", None);

    let database = ReflectionDatabase::embedded();
    assert_eq!(selection(&dom, &database, &[empty, part]).len(), 2);
}
