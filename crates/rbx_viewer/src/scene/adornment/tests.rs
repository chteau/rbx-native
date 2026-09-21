use std::collections::HashMap;

use glam::Vec3;
use rbx_dom::{Axes, CFrameData, Color3Data, Faces, Instance, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;
use crate::scene::ShapeKind;

const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
const WORKSPACE: u32 = 1;
const PART: u32 = 2;
const ADORNMENT: u32 = 3;
const PART_SIZE: Vec3 = Vec3::new(4.0, 2.0, 6.0);

fn insert(
    dom: &mut WeakDom,
    id: u32,
    class: &str,
    parent: Option<Ref>,
    properties: Vec<(&str, Variant)>,
) -> Ref {
    let referent = Ref::new(id);
    let mut instance = Instance::new(referent, class, class);
    for (name, value) in properties {
        instance.properties_mut().insert(name.to_string(), value);
    }
    dom.insert(instance);
    dom.set_parent(referent, parent);
    referent
}

fn cframe_at(position: Vec3) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        rotation: IDENTITY_ROTATION,
    })
}

/// A place with one part at the origin, and `class` adorned to it under
/// `parent_class` (`Workspace` unless a test says otherwise).
fn place(
    class: &str,
    properties: Vec<(&str, Variant)>,
    adornee: bool,
) -> (WeakDom, HashMap<Ref, Placement>) {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, WORKSPACE, "Workspace", None, vec![]);
    let part = insert(
        &mut dom,
        PART,
        "Part",
        Some(workspace),
        vec![
            (
                "size",
                Variant::Vector3(Vector3Data {
                    x: PART_SIZE.x,
                    y: PART_SIZE.y,
                    z: PART_SIZE.z,
                }),
            ),
            ("CFrame", cframe_at(Vec3::ZERO)),
        ],
    );
    let mut properties = properties;
    if adornee {
        properties.push(("Adornee", Variant::Ref(part)));
    }
    insert(&mut dom, ADORNMENT, class, Some(workspace), properties);

    let placements = HashMap::from([(
        part,
        Placement {
            kind: ShapeKind::Box,
            model: Mat4::from_scale(PART_SIZE),
            size: PART_SIZE,
        },
    )]);
    (dom, placements)
}

fn plan_of(class: &str, properties: Vec<(&str, Variant)>) -> Vec<Adornment> {
    let (dom, placements) = place(class, properties, true);
    plan(&dom, &ReflectionDatabase::embedded(), &placements)
}

fn solids(adornment: &Adornment) -> Vec<Solid> {
    adornment
        .pieces
        .iter()
        .filter_map(|piece| match piece {
            Piece::Solid(solid) => Some(*solid),
            _ => None,
        })
        .collect()
}

/// The documented box: an outline of `LineThickness` studs, and surfaces
/// that are invisible until `SurfaceTransparency` says otherwise.
#[test]
fn a_selection_box_outlines_its_adornee_and_hides_its_surfaces() {
    let planned = plan_of("SelectionBox", vec![]);
    assert_eq!(planned.len(), 1);
    let bars = solids(&planned[0]);
    assert_eq!(bars.len(), 12, "twelve edges, no surface");
    // Every bar is thin across and as long as the side it runs along.
    for bar in &bars {
        let Mesh::Box { size } = bar.mesh else {
            panic!("an edge is a bar");
        };
        let long = size.max_element();
        assert!(PART_SIZE.to_array().contains(&(long - 0.05)));
    }
    assert_eq!(planned[0].covers, vec![Ref::new(PART)]);
}

#[test]
fn a_selection_box_surface_appears_once_it_is_not_transparent() {
    let planned = plan_of(
        "SelectionBox",
        vec![
            ("SurfaceTransparency", Variant::Float32(0.25)),
            (
                "SurfaceColor3",
                Variant::Color3(Color3Data {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                }),
            ),
        ],
    );
    let bars = solids(&planned[0]);
    assert_eq!(bars.len(), 13);
    let surface = bars[0];
    assert_eq!(surface.mesh, Mesh::Box { size: PART_SIZE });
    assert!((surface.alpha - 0.75).abs() < 1e-6);
    assert_eq!(surface.color, [1.0, 0.0, 0.0]);
}

/// Documented: "If set to `0`, the outline will not be visible at all."
#[test]
fn a_zero_line_thickness_draws_no_outline() {
    let planned = plan_of(
        "SelectionBox",
        vec![("LineThickness", Variant::Float32(0.0))],
    );
    assert!(planned.is_empty(), "nothing left to draw");
}

/// Documented on `GuiBase3d`: `Visible` false hides the object, and
/// `Transparency` 1 is invisible.
#[test]
fn visible_and_transparency_are_read_as_documented() {
    assert!(plan_of("SelectionBox", vec![("Visible", Variant::Bool(false))]).is_empty());
    let faded = plan_of(
        "SelectionBox",
        vec![("Transparency", Variant::Float32(0.25))],
    );
    assert!((solids(&faded[0])[0].alpha - 0.75).abs() < 1e-6);
}

/// Documented: `SizeRelativeOffset` is a scale of the adornee's own size,
/// where 1 reaches the corresponding edge, and `CFrame` is applied after it.
#[test]
fn a_handle_is_offset_by_the_adornees_size_then_by_its_own_cframe() {
    let planned = plan_of(
        "BoxHandleAdornment",
        vec![
            (
                "SizeRelativeOffset",
                Variant::Vector3(Vector3Data {
                    x: 0.0,
                    y: 1.0,
                    z: 0.0,
                }),
            ),
            ("CFrame", cframe_at(Vec3::new(0.0, 3.0, 0.0))),
        ],
    );
    let placed = solids(&planned[0])[0].frame.w_axis.truncate();
    // Half the part's own height, then the handle's own CFrame on top.
    assert!((placed - Vec3::new(0.0, PART_SIZE.y * 0.5 + 3.0, 0.0)).length() < 1e-5);
}

/// Documented: `ZIndex` of -1 overrides `AlwaysOnTop`.
#[test]
fn a_z_index_of_minus_one_gives_up_always_on_top() {
    let on_top = plan_of(
        "BoxHandleAdornment",
        vec![("AlwaysOnTop", Variant::Bool(true))],
    );
    assert!(on_top[0].always_on_top);
    let overridden = plan_of(
        "BoxHandleAdornment",
        vec![
            ("AlwaysOnTop", Variant::Bool(true)),
            ("ZIndex", Variant::Int32(-1)),
        ],
    );
    assert!(!overridden[0].always_on_top);
    let clamped = plan_of("BoxHandleAdornment", vec![("ZIndex", Variant::Int32(99))]);
    assert_eq!(clamped[0].order, 10);
}

/// Every shape with a length runs along its frame's -Z, so a cone with no
/// `CFrame` of its own points straight out of the adornee's front.
#[test]
fn the_shape_adornments_each_read_their_own_size() {
    let cone = plan_of(
        "ConeHandleAdornment",
        vec![
            ("Radius", Variant::Float32(2.0)),
            ("Height", Variant::Float32(7.0)),
        ],
    );
    assert_eq!(
        solids(&cone[0])[0].mesh,
        Mesh::Cone {
            radius: 2.0,
            height: 7.0
        }
    );

    let sphere = plan_of(
        "SphereHandleAdornment",
        vec![("Radius", Variant::Float32(3.0))],
    );
    assert_eq!(solids(&sphere[0])[0].mesh, Mesh::Sphere { radius: 3.0 });

    let cylinder = plan_of(
        "CylinderHandleAdornment",
        vec![
            ("Radius", Variant::Float32(2.0)),
            ("InnerRadius", Variant::Float32(1.0)),
            ("Height", Variant::Float32(5.0)),
            ("Angle", Variant::Float32(90.0)),
        ],
    );
    assert_eq!(
        solids(&cylinder[0])[0].mesh,
        Mesh::Cylinder {
            radius: 2.0,
            inner: 1.0,
            height: 5.0,
            sweep: 90.0
        }
    );
}

/// A cylinder with no `Angle` at all is a whole cylinder: a zero-degree
/// sector would draw nothing, which cannot be what a default does.
#[test]
fn a_cylinder_with_no_angle_is_a_full_one() {
    let planned = plan_of("CylinderHandleAdornment", vec![]);
    let Mesh::Cylinder { sweep, .. } = solids(&planned[0])[0].mesh else {
        panic!("a cylinder");
    };
    assert_eq!(sweep, 360.0);
}

/// `Length` is documented in studs and `Thickness` in pixels.
#[test]
fn a_line_handle_runs_its_length_in_studs_and_its_thickness_in_pixels() {
    let planned = plan_of(
        "LineHandleAdornment",
        vec![
            ("Length", Variant::Float32(8.0)),
            ("Thickness", Variant::Float32(3.0)),
        ],
    );
    let Piece::Line(line) = planned[0].pieces[0] else {
        panic!("a line");
    };
    assert!((line.to - line.from).length() - 8.0 < 1e-5);
    assert_eq!(line.pixels, 3.0);
}

/// Documented: "the shape of the handles can be set to either arrows or
/// spheres", one per enabled face.
#[test]
fn handles_draw_one_handle_per_enabled_face() {
    let faces = Variant::Faces(Faces {
        top: true,
        right: true,
        front: false,
        back: false,
        left: false,
        bottom: false,
    });
    let resize = plan_of("Handles", vec![("Faces", faces.clone())]);
    assert_eq!(solids(&resize[0]).len(), 2, "a sphere per face");
    assert!(matches!(solids(&resize[0])[0].mesh, Mesh::Sphere { .. }));

    let movement = plan_of(
        "Handles",
        vec![("Faces", faces), ("Style", Variant::Enum(1))],
    );
    let arrows = solids(&movement[0]);
    assert_eq!(arrows.len(), 4, "a shaft and a head per face");
    assert!(matches!(arrows[0].mesh, Mesh::Cylinder { .. }));
    assert!(matches!(arrows[1].mesh, Mesh::Cone { .. }));
    // Each handle stands off its own face rather than the part's centre:
    // the one on Top is clear of the top face, the one on Right of the side.
    assert!(arrows
        .iter()
        .any(|arrow| arrow.frame.w_axis.y > PART_SIZE.y * 0.5));
    assert!(arrows
        .iter()
        .any(|arrow| arrow.frame.w_axis.x > PART_SIZE.x * 0.5));
}

#[test]
fn arc_handles_draw_one_ring_per_enabled_axis() {
    let planned = plan_of(
        "ArcHandles",
        vec![(
            "Axes",
            Variant::Axes(Axes {
                x: true,
                y: false,
                z: true,
            }),
        )],
    );
    let rings = solids(&planned[0]);
    assert_eq!(rings.len(), 2);
    for ring in rings {
        assert!(matches!(ring.mesh, Mesh::Arc { .. }));
    }
}

/// Documented: "the sphere's geometry consists of a ring/outline in
/// addition to a surface", with the surface invisible by default.
#[test]
fn a_selection_sphere_is_a_ring_until_its_surface_shows() {
    let planned = plan_of("SelectionSphere", vec![]);
    assert_eq!(planned[0].pieces.len(), 1);
    let Piece::Ring(ring) = planned[0].pieces[0] else {
        panic!("a ring");
    };
    // Big enough to hold the whole part.
    assert!(ring.radius >= PART_SIZE.max_element() * 0.5);

    let filled = plan_of(
        "SelectionSphere",
        vec![("SurfaceTransparency", Variant::Float32(0.0))],
    );
    assert_eq!(filled[0].pieces.len(), 2);
    assert!(matches!(solids(&filled[0])[0].mesh, Mesh::Sphere { .. }));
}

/// Documented: a `SurfaceSelection` "highlights a particular face
/// (`TargetSurface`) of its `Adornee`".
#[test]
fn a_surface_selection_lies_on_the_face_it_names() {
    // `NormalId.Right` is 0 in the API dump.
    let planned = plan_of(
        "SurfaceSelection",
        vec![("TargetSurface", Variant::Enum(0))],
    );
    let slab = solids(&planned[0])[0];
    assert!((slab.frame.w_axis.x - PART_SIZE.x * 0.5).abs() < 1e-5);
    let Mesh::Box { size } = slab.mesh else {
        panic!("a slab");
    };
    assert!(size.x < 0.1, "thin across the face");
    assert_eq!(size.y, PART_SIZE.y);
    assert_eq!(size.z, PART_SIZE.z);
}

/// The classes with nothing serialized to draw from: recognized, and drawn
/// as nothing rather than guessed at.
#[test]
fn the_classes_with_no_serialized_geometry_draw_nothing() {
    for class in [
        "WireframeHandleAdornment",
        "ParabolaAdornment",
        "SelectionLasso",
    ] {
        assert!(plan_of(class, vec![]).is_empty(), "{class}");
    }
}

/// An adornment renders where Roblox says it does: under the `Workspace`,
/// or where GUI objects are rendered — here, `StarterGui`.
#[test]
fn only_an_adornment_somewhere_rendered_is_planned() {
    let database = ReflectionDatabase::embedded();
    let (mut dom, placements) = place("SelectionBox", vec![], true);
    assert_eq!(plan(&dom, &database, &placements).len(), 1);

    // Moved into a service that renders nothing.
    let storage = insert(&mut dom, 20, "ReplicatedStorage", None, vec![]);
    dom.set_parent(Ref::new(ADORNMENT), Some(storage));
    assert!(plan(&dom, &database, &placements).is_empty());

    let starter = insert(&mut dom, 21, "StarterGui", None, vec![]);
    dom.set_parent(Ref::new(ADORNMENT), Some(starter));
    assert_eq!(plan(&dom, &database, &placements).len(), 1);
}

/// A `Model` has no orientation of its own, so it gets the world-axis
/// -aligned box round everything beneath it — and every part under it is
/// what a move has to re-plan against.
#[test]
fn a_container_adornee_takes_the_box_around_its_parts() {
    let mut dom = WeakDom::new();
    let workspace = insert(&mut dom, WORKSPACE, "Workspace", None, vec![]);
    let model = insert(&mut dom, 10, "Model", Some(workspace), vec![]);
    let first = insert(&mut dom, 11, "Part", Some(model), vec![]);
    let second = insert(&mut dom, 12, "Part", Some(model), vec![]);
    insert(
        &mut dom,
        ADORNMENT,
        "SelectionBox",
        Some(workspace),
        vec![("Adornee", Variant::Ref(model))],
    );
    let placements = HashMap::from([
        (
            first,
            Placement {
                kind: ShapeKind::Box,
                model: Mat4::from_scale(Vec3::splat(2.0)),
                size: Vec3::splat(2.0),
            },
        ),
        (
            second,
            Placement {
                kind: ShapeKind::Box,
                model: Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0))
                    * Mat4::from_scale(Vec3::splat(2.0)),
                size: Vec3::splat(2.0),
            },
        ),
    ]);

    let planned = plan(&dom, &ReflectionDatabase::embedded(), &placements);
    assert_eq!(planned.len(), 1);
    assert_eq!(planned[0].covers.len(), 2);
    let bars = solids(&planned[0]);
    // The longest bar spans both parts: 10 studs apart plus 2 of part.
    let longest = bars
        .iter()
        .map(|bar| match bar.mesh {
            Mesh::Box { size } => size.max_element(),
            _ => 0.0,
        })
        .fold(0.0f32, f32::max);
    assert!((longest - 12.05).abs() < 1e-4, "{longest}");
}

/// An adornment naming nothing has nothing to wrap.
#[test]
fn a_selection_box_with_no_adornee_draws_nothing() {
    let (dom, placements) = place("SelectionBox", vec![], false);
    assert!(plan(&dom, &ReflectionDatabase::embedded(), &placements).is_empty());
}

/// A handle adornment, by contrast, is documented as drawing "into the
/// `Workspace`" with no adornee at all — its own `CFrame` is then a world
/// frame.
#[test]
fn a_handle_with_no_adornee_stands_in_world_space() {
    let (dom, placements) = place(
        "BoxHandleAdornment",
        vec![("CFrame", cframe_at(Vec3::new(5.0, 6.0, 7.0)))],
        false,
    );
    let planned = plan(&dom, &ReflectionDatabase::embedded(), &placements);
    assert_eq!(planned.len(), 1);
    assert!(planned[0].covers.is_empty());
    let placed = solids(&planned[0])[0].frame.w_axis.truncate();
    assert!((placed - Vec3::new(5.0, 6.0, 7.0)).length() < 1e-5);
}
