//! Unit tests for [`super`]: the canvas sizes a container asks for, where its
//! quad lands, and which instance it hangs off.

use std::collections::HashMap;

use glam::{Mat4, Vec3};
use rbx_dom::{UDim, UDim2, Vector2Data, Vector3Data, WeakDom};

use super::*;
use crate::scene::gui::resolve_canvas;
use crate::scene::ShapeKind;

const DATABASE: fn() -> ReflectionDatabase = ReflectionDatabase::embedded;

fn udim2(sx: f32, ox: i32, sy: f32, oy: i32) -> Variant {
    Variant::UDim2(UDim2 {
        x: UDim {
            scale: sx,
            offset: ox,
        },
        y: UDim {
            scale: sy,
            offset: oy,
        },
    })
}

fn vector2_of(x: f32, y: f32) -> Variant {
    Variant::Vector2(Vector2Data { x, y })
}

fn vector3_of(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

/// A part-sized placement at the origin: the unit mesh scaled to `size`.
fn placement(size: Vec3) -> Placement {
    Placement {
        kind: ShapeKind::Box,
        model: Mat4::from_scale(size),
        size,
    }
}

/// `Part -> <class>`, with the part registered as a placement of `size`.
fn fixture(class: &str, size: Vec3) -> (WeakDom, Ref, HashMap<Ref, Placement>) {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    let gui = dom.new_instance(class, class, Some(part));
    let placements = HashMap::from([(part, placement(size))]);
    (dom, gui, placements)
}

/// One `ImageLabel` filling its parent, so the container has something to draw.
fn filled(dom: &mut WeakDom, parent: Ref) {
    let label = dom.new_instance("ImageLabel", "ImageLabel", Some(parent));
    dom.set_property(label, "Size", udim2(1.0, 0, 1.0, 0))
        .unwrap();
    dom.set_property(
        label,
        "Image",
        Variant::String("rbxassetid://1".to_string()),
    )
    .unwrap();
}

fn planned(dom: &WeakDom, placements: &HashMap<Ref, Placement>) -> Vec<SpaceGui> {
    plan(dom, &DATABASE(), placements)
}

#[test]
fn the_scale_half_of_a_billboards_size_is_its_stud_size() {
    let size = studs(super::super::plan::Span {
        scale: [4.0, 3.0],
        offset: [200.0, 50.0],
    });
    assert_eq!(size, [4.0, 3.0]);
}

#[test]
fn an_offset_only_billboard_size_falls_back_to_studs_at_the_reference_density() {
    let size = studs(super::super::plan::Span {
        scale: [0.0, 0.0],
        offset: [200.0, 50.0],
    });
    assert_eq!(size, [4.0, 1.0]);
}

#[test]
fn a_billboards_canvas_is_its_stud_size_at_the_reference_density() {
    assert_eq!(billboard_canvas([4.0, 1.5]), [200.0, 75.0]);
}

/// The fountain sign boards of `marked.rbxl`: a 7.09 × 9.14 stud face.
const BOARD_FACE: [f32; 2] = [7.0938597, 9.136839];

fn fixed_size() -> BTreeMap<String, Variant> {
    BTreeMap::from([("SizingMode".to_string(), Variant::Enum(FIXED_SIZE))])
}

#[test]
fn a_fixed_size_canvas_of_zero_falls_back_to_the_default_per_axis() {
    let mut properties = fixed_size();
    assert_eq!(surface_canvas(&properties, BOARD_FACE), DEFAULT_CANVAS);

    properties.insert("CanvasSize".to_string(), vector2_of(0.0, 600.0));
    assert_eq!(
        surface_canvas(&properties, BOARD_FACE),
        [DEFAULT_CANVAS[0], 600.0]
    );
}

#[test]
fn a_fixed_size_canvas_is_read_whole_and_ignores_the_face() {
    let mut properties = fixed_size();
    properties.insert("CanvasSize".to_string(), vector2_of(800.0, 600.0));
    assert_eq!(surface_canvas(&properties, BOARD_FACE), [800.0, 600.0]);
}

// `PixelsPerStud` is Roblox's default `SizingMode`, and the mode the boards
// use: their `CanvasSize` of 800 × 600 is dead data, the canvas is the face
// at 50 px/stud. Sizing it from `CanvasSize` instead was what shrank a
// 355 px label to a corner of the board.

#[test]
fn pixels_per_stud_is_the_default_and_sizes_the_canvas_from_the_face() {
    let properties = BTreeMap::from([("CanvasSize".to_string(), vector2_of(800.0, 600.0))]);
    assert_eq!(surface_canvas(&properties, BOARD_FACE), [355.0, 457.0]);
}

#[test]
fn pixels_per_stud_scales_the_canvas_by_the_density() {
    let properties = BTreeMap::from([
        ("SizingMode".to_string(), Variant::Enum(1)),
        ("PixelsPerStud".to_string(), Variant::Float32(10.0)),
    ]);
    assert_eq!(surface_canvas(&properties, [4.0, 2.0]), [40.0, 20.0]);
}

#[test]
fn face_studs_are_measured_off_the_placed_quad() {
    let corners = face_corners(NormalId::Front, &placement(Vec3::new(7.0, 9.0, 0.1)), 0.0);
    let studs = face_studs(&corners);
    assert!((studs[0] - 7.0).abs() < 1e-5 && (studs[1] - 9.0).abs() < 1e-5);
}

#[test]
fn a_planned_surface_gui_covers_its_face_at_the_reference_density() {
    let (mut dom, gui, placements) =
        fixture("SurfaceGui", Vec3::new(BOARD_FACE[0], BOARD_FACE[1], 0.088));
    filled(&mut dom, gui);
    dom.set_property(gui, "CanvasSize", vector2_of(800.0, 600.0))
        .unwrap();

    let planned = planned(&dom, &placements);
    assert_eq!(planned[0].canvas, [355.0, 457.0]);
    let elements = resolve_canvas(&planned[0]);
    assert_eq!(elements[0].rect.width, 355.0);
    assert_eq!(elements[0].rect.height, 457.0);
}

#[test]
fn the_front_face_quad_is_the_rectangle_a_stretched_decal_covers() {
    let corners = face_corners(NormalId::Front, &placement(Vec3::new(4.0, 2.0, 1.0)), 0.0);
    // Front is -Z, image right is -X: seen from outside, +X is on the left.
    assert_eq!(corners[0], Vec3::new(2.0, 1.0, -0.5));
    assert_eq!(corners[1], Vec3::new(-2.0, 1.0, -0.5));
    assert_eq!(corners[2], Vec3::new(-2.0, -1.0, -0.5));
    assert_eq!(corners[3], Vec3::new(2.0, -1.0, -0.5));
}

#[test]
fn the_top_face_quad_spans_the_parts_own_width_and_depth() {
    let corners = face_corners(NormalId::Top, &placement(Vec3::new(4.0, 2.0, 6.0)), 0.0);
    for corner in corners {
        assert_eq!(corner.y, 1.0);
        assert_eq!(corner.x.abs(), 2.0);
        assert_eq!(corner.z.abs(), 3.0);
    }
}

// The remaining four faces, pinned to exact corners like the Front/Top cases
// above: `face_corners` shares one code path (`NormalId::axes`) for all six,
// but that path is also what a bug report of "the wrong face renders, or the
// right face renders mirrored" would hide in — Front and Top alone can't rule
// out a swapped axis or an inverted sign on Right/Left/Back/Bottom.
#[test]
fn the_right_face_quad_is_the_rectangle_a_stretched_decal_covers() {
    let corners = face_corners(NormalId::Right, &placement(Vec3::new(4.0, 2.0, 1.0)), 0.0);
    // Right is +X, image right is -Z.
    assert_eq!(corners[0], Vec3::new(2.0, 1.0, 0.5));
    assert_eq!(corners[1], Vec3::new(2.0, 1.0, -0.5));
    assert_eq!(corners[2], Vec3::new(2.0, -1.0, -0.5));
    assert_eq!(corners[3], Vec3::new(2.0, -1.0, 0.5));
}

#[test]
fn the_left_face_quad_is_the_rectangle_a_stretched_decal_covers() {
    let corners = face_corners(NormalId::Left, &placement(Vec3::new(4.0, 2.0, 1.0)), 0.0);
    // Left is -X, image right is +Z.
    assert_eq!(corners[0], Vec3::new(-2.0, 1.0, -0.5));
    assert_eq!(corners[1], Vec3::new(-2.0, 1.0, 0.5));
    assert_eq!(corners[2], Vec3::new(-2.0, -1.0, 0.5));
    assert_eq!(corners[3], Vec3::new(-2.0, -1.0, -0.5));
}

#[test]
fn the_back_face_quad_is_the_rectangle_a_stretched_decal_covers() {
    let corners = face_corners(NormalId::Back, &placement(Vec3::new(4.0, 2.0, 1.0)), 0.0);
    // Back is +Z, image right is +X.
    assert_eq!(corners[0], Vec3::new(-2.0, 1.0, 0.5));
    assert_eq!(corners[1], Vec3::new(2.0, 1.0, 0.5));
    assert_eq!(corners[2], Vec3::new(2.0, -1.0, 0.5));
    assert_eq!(corners[3], Vec3::new(-2.0, -1.0, 0.5));
}

#[test]
fn the_bottom_face_quad_is_the_rectangle_a_stretched_decal_covers() {
    let corners = face_corners(NormalId::Bottom, &placement(Vec3::new(4.0, 2.0, 1.0)), 0.0);
    // Bottom is -Y, image right is +X.
    assert_eq!(corners[0], Vec3::new(-2.0, -1.0, 0.5));
    assert_eq!(corners[1], Vec3::new(2.0, -1.0, 0.5));
    assert_eq!(corners[2], Vec3::new(2.0, -1.0, -0.5));
    assert_eq!(corners[3], Vec3::new(-2.0, -1.0, -0.5));
}

/// `Enum.NormalId`'s real ordinals (Roblox's `creator-docs`): Right=0, Top=1,
/// Back=2, Left=3, Bottom=4, Front=5 — not alphabetical and not declaration
/// order, so this is the one guard against `from_ordinal` drifting from the
/// wire format if the variants above it are ever reordered.
#[test]
fn from_ordinal_matches_robloxs_normalid_enum() {
    assert_eq!(NormalId::from_ordinal(0), Some(NormalId::Right));
    assert_eq!(NormalId::from_ordinal(1), Some(NormalId::Top));
    assert_eq!(NormalId::from_ordinal(2), Some(NormalId::Back));
    assert_eq!(NormalId::from_ordinal(3), Some(NormalId::Left));
    assert_eq!(NormalId::from_ordinal(4), Some(NormalId::Bottom));
    assert_eq!(NormalId::from_ordinal(5), Some(NormalId::Front));
}

#[test]
fn z_offset_pushes_the_quad_out_along_the_face_normal() {
    let corners = face_corners(NormalId::Front, &placement(Vec3::splat(2.0)), 0.25);
    assert!(corners.iter().all(|corner| corner.z == -1.25));
}

#[test]
fn a_surface_gui_hangs_off_its_parent_part_by_default() {
    let (mut dom, gui, placements) = fixture("SurfaceGui", Vec3::new(4.0, 2.0, 1.0));
    filled(&mut dom, gui);
    dom.set_property(gui, "CanvasSize", vector2_of(800.0, 600.0))
        .unwrap();

    let planned = planned(&dom, &placements);
    assert_eq!(planned.len(), 1);
    // `PixelsPerStud` by default: the 4 × 2 stud face at 50 px/stud.
    assert_eq!(planned[0].canvas, [200.0, 100.0]);
    let Anchor::Surface { corners } = planned[0].anchor else {
        panic!("a SurfaceGui is anchored to a face");
    };
    assert_eq!(corners[0], Vec3::new(2.0, 1.0, -0.5));
}

#[test]
fn an_adornee_overrides_the_parent_part() {
    let (mut dom, gui, mut placements) = fixture("SurfaceGui", Vec3::splat(1.0));
    filled(&mut dom, gui);
    let other = dom.new_instance("Part", "Other", None);
    placements.insert(other, placement(Vec3::new(10.0, 10.0, 10.0)));
    dom.set_property(gui, "Adornee", Variant::Ref(other))
        .unwrap();

    let planned = planned(&dom, &placements);
    let Anchor::Surface { corners } = planned[0].anchor else {
        panic!("a SurfaceGui is anchored to a face");
    };
    assert_eq!(corners[0], Vec3::new(5.0, 5.0, -5.0));
}

#[test]
fn a_dangling_adornee_falls_back_to_the_parent() {
    let (mut dom, gui, placements) = fixture("SurfaceGui", Vec3::splat(2.0));
    filled(&mut dom, gui);
    dom.set_property(gui, "Adornee", Variant::Ref(Ref::new(999)))
        .unwrap();

    let planned = planned(&dom, &placements);
    let Anchor::Surface { corners } = planned[0].anchor else {
        panic!("a SurfaceGui is anchored to a face");
    };
    assert_eq!(corners[0], Vec3::new(1.0, 1.0, -1.0));
}

#[test]
fn the_named_face_is_the_one_the_quad_lands_on() {
    let (mut dom, gui, placements) = fixture("SurfaceGui", Vec3::new(4.0, 2.0, 6.0));
    filled(&mut dom, gui);
    // `Enum.NormalId.Top`.
    dom.set_property(gui, "Face", Variant::Enum(1)).unwrap();

    let planned = planned(&dom, &placements);
    let Anchor::Surface { corners } = planned[0].anchor else {
        panic!("a SurfaceGui is anchored to a face");
    };
    assert!(corners.iter().all(|corner| corner.y == 1.0));
}

#[test]
fn a_billboard_takes_its_adornees_own_position() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    let gui = dom.new_instance("BillboardGui", "BillboardGui", Some(part));
    filled(&mut dom, gui);
    dom.set_property(gui, "Size", udim2(4.0, 0, 2.0, 0))
        .unwrap();
    dom.set_property(gui, "StudsOffsetWorldSpace", vector3_of(0.0, 3.0, 0.0))
        .unwrap();
    let placements = HashMap::from([(
        part,
        Placement {
            kind: ShapeKind::Box,
            model: Mat4::from_translation(Vec3::new(7.0, 0.0, -2.0)),
            size: Vec3::ONE,
        },
    )]);

    let planned = planned(&dom, &placements);
    assert_eq!(planned[0].canvas, [200.0, 100.0]);
    let Anchor::Billboard {
        origin,
        size,
        world_offset,
        ..
    } = planned[0].anchor
    else {
        panic!("a BillboardGui faces the camera");
    };
    assert_eq!(origin, Vec3::new(7.0, 0.0, -2.0));
    assert_eq!(size, [4.0, 2.0]);
    assert_eq!(world_offset, Vec3::new(0.0, 3.0, 0.0));
}

#[test]
fn a_billboard_on_an_attachment_composes_the_parts_cframe() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(rbx_dom::CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 5.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    )
    .unwrap();
    let attachment = dom.new_instance("Attachment", "Attachment", Some(part));
    dom.set_property(
        attachment,
        "CFrame",
        Variant::CFrame(rbx_dom::CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 2.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    )
    .unwrap();
    let gui = dom.new_instance("BillboardGui", "BillboardGui", Some(attachment));
    filled(&mut dom, gui);
    dom.set_property(gui, "Size", udim2(1.0, 0, 1.0, 0))
        .unwrap();

    let planned = planned(&dom, &HashMap::new());
    let Anchor::Billboard { origin, .. } = planned[0].anchor else {
        panic!("a BillboardGui faces the camera");
    };
    assert_eq!(origin, Vec3::new(0.0, 7.0, 0.0));
}

#[test]
fn a_disabled_container_is_not_planned() {
    let (mut dom, gui, placements) = fixture("SurfaceGui", Vec3::ONE);
    filled(&mut dom, gui);
    dom.set_property(gui, "Enabled", Variant::Bool(false))
        .unwrap();
    assert!(planned(&dom, &placements).is_empty());
}

#[test]
fn a_container_holding_only_transparent_text_is_not_planned() {
    let (mut dom, gui, placements) = fixture("SurfaceGui", Vec3::ONE);
    let label = dom.new_instance("TextLabel", "TextLabel", Some(gui));
    dom.set_property(label, "BackgroundTransparency", Variant::Float32(1.0))
        .unwrap();
    assert!(planned(&dom, &placements).is_empty());
}

#[test]
fn a_text_label_with_an_opaque_background_earns_a_canvas() {
    let (mut dom, gui, placements) = fixture("SurfaceGui", Vec3::ONE);
    let label = dom.new_instance("TextLabel", "TextLabel", Some(gui));
    dom.set_property(label, "Size", udim2(1.0, 0, 1.0, 0))
        .unwrap();

    let planned = planned(&dom, &placements);
    assert_eq!(planned.len(), 1);
    assert_eq!(resolve_canvas(&planned[0])[0].background_alpha, 1.0);
}

// The boards themselves: a white 355 px `ImageLabel` with the two brown
// `TextLabel`s the `UIListLayout` stacks under it, together covering the
// whole 355 × 457 canvas so none of the part's own dark face shows.

#[test]
fn the_fountain_board_stacks_its_labels_under_the_image() {
    let (mut dom, gui, placements) =
        fixture("SurfaceGui", Vec3::new(BOARD_FACE[0], BOARD_FACE[1], 0.088));
    let image = dom.new_instance("ImageLabel", "MapThumbnail", Some(gui));
    dom.set_property(image, "Size", udim2(0.0, 355, 0.0, 355))
        .unwrap();
    let list = dom.new_instance("UIListLayout", "UIListLayout", Some(gui));
    dom.set_property(list, "FillDirection", Variant::Enum(1))
        .unwrap();
    dom.set_property(list, "VerticalAlignment", Variant::Enum(1))
        .unwrap();
    for (name, height) in [("MapName", 50), ("VotesAmount", 55)] {
        let label = dom.new_instance("TextLabel", name, Some(gui));
        dom.set_property(label, "Size", udim2(1.0, 0, 0.0, height))
            .unwrap();
    }

    let elements = resolve_canvas(&planned(&dom, &placements)[0]);
    let rects: Vec<[f32; 4]> = elements
        .iter()
        .map(|element| {
            let rect = element.rect;
            [rect.x, rect.y, rect.width, rect.height]
        })
        .collect();
    assert_eq!(
        rects,
        [
            [0.0, 0.0, 355.0, 355.0],
            [0.0, 355.0, 355.0, 50.0],
            [0.0, 405.0, 355.0, 55.0],
        ]
    );
}

#[test]
fn always_on_top_is_carried_through() {
    let (mut dom, gui, placements) = fixture("SurfaceGui", Vec3::ONE);
    filled(&mut dom, gui);
    dom.set_property(gui, "AlwaysOnTop", Variant::Bool(true))
        .unwrap();
    assert!(planned(&dom, &placements)[0].always_on_top);
}

#[test]
fn a_canvas_resolves_udim2_against_its_own_pixels_not_a_viewport() {
    let (mut dom, gui, placements) = fixture("SurfaceGui", Vec3::ONE);
    filled(&mut dom, gui);
    dom.set_property(gui, "SizingMode", Variant::Enum(FIXED_SIZE))
        .unwrap();
    dom.set_property(gui, "CanvasSize", vector2_of(800.0, 600.0))
        .unwrap();

    let elements = resolve_canvas(&planned(&dom, &placements)[0]);
    assert_eq!(elements.len(), 1);
    assert_eq!(elements[0].rect.width, 800.0);
    assert_eq!(elements[0].rect.height, 600.0);
}
