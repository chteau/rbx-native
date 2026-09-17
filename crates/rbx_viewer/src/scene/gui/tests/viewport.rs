//! What a `ViewportFrame` is read into: its camera (live or as Studio saves
//! it), its lighting, which parts are its scene, and the projection they are
//! drawn through.

use glam::{Vec3, Vec4};
use rbx_dom::{CFrameData, Vector3Data};

use super::*;
use crate::scene::{srgb_to_linear, Scene};

fn cframe(x: f32, y: f32, z: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    })
}

fn part(dom: &mut WeakDom, parent: Ref, x: f32, red: u8) -> Ref {
    let part = dom.new_instance("Part", "Part", Some(parent));
    dom.set_property(part, "CFrame", cframe(x, 0.0, 0.0))
        .unwrap();
    dom.set_property(
        part,
        "size",
        Variant::Vector3(Vector3Data {
            x: 2.0,
            y: 2.0,
            z: 2.0,
        }),
    )
    .unwrap();
    dom.set_property(
        part,
        "Color3uint8",
        Variant::Color3uint8 { r: red, g: 0, b: 0 },
    )
    .unwrap();
    part
}

/// A `ViewportFrame` filling the screen, with a `Camera` child it looks
/// through from `z` studs down +Z.
fn viewport_frame(dom: &mut WeakDom, gui: Ref, z: f32) -> Ref {
    let frame = dom.new_instance("ViewportFrame", "ViewportFrame", Some(gui));
    dom.set_property(frame, "Size", udim2(1.0, 0, 1.0, 0))
        .unwrap();
    let camera = dom.new_instance("Camera", "Camera", Some(frame));
    dom.set_property(camera, "CFrame", cframe(0.0, 0.0, z))
        .unwrap();
    dom.set_property(camera, "FieldOfView", Variant::Float32(90.0))
        .unwrap();
    dom.set_property(frame, "CurrentCamera", Variant::Ref(camera))
        .unwrap();
    frame
}

fn viewport(dom: &WeakDom) -> Viewport {
    let elements = resolve(&screens(dom), VIEWPORT);
    elements[0]
        .viewport
        .clone()
        .expect("a ViewportFrame carries its scene")
}

#[test]
fn a_live_camera_and_the_docs_defaults_are_read() {
    let (mut dom, gui) = screen_gui();
    viewport_frame(&mut dom, gui, 10.0);

    let viewport = viewport(&dom);

    let camera = viewport.camera.expect("CurrentCamera points at a Camera");
    assert_eq!(camera.eye(), Vec3::new(0.0, 0.0, 10.0));
    assert_eq!(camera.fov_degrees, 90.0);
    assert_eq!(viewport.ambient, [srgb_to_linear(200.0 / 255.0); 3]);
    assert_eq!(viewport.light_color, [srgb_to_linear(140.0 / 255.0); 3]);
    // `(-1, -1, -1)` is where the light travels; the lamp sits opposite.
    let expected = Vec3::ONE.normalize();
    assert!((viewport.light - expected).length() < 1e-6);
    assert_eq!(viewport.tint, [1.0; 3]);
    assert_eq!(viewport.alpha, 1.0);
}

#[test]
fn the_pose_studio_saves_on_the_frame_stands_in_for_the_camera() {
    let (mut dom, gui) = screen_gui();
    let frame = dom.new_instance("ViewportFrame", "ViewportFrame", Some(gui));
    dom.set_property(frame, "Size", udim2(1.0, 0, 1.0, 0))
        .unwrap();
    dom.set_property(frame, "CameraCFrame", cframe(1.0, 2.0, 3.0))
        .unwrap();
    // Radians on the frame, unlike the `Camera`'s own degrees.
    dom.set_property(
        frame,
        "CameraFieldOfView",
        Variant::Float32(60.0_f32.to_radians()),
    )
    .unwrap();
    dom.set_property(frame, "ImageTransparency", Variant::Float32(0.5))
        .unwrap();
    dom.set_property(
        frame,
        "ImageColor3",
        Variant::Color3(Color3Data {
            r: 0.0,
            g: 0.0,
            b: 1.0,
        }),
    )
    .unwrap();
    dom.set_property(
        frame,
        "LightDirection",
        Variant::Vector3(Vector3Data {
            x: 0.0,
            y: -2.0,
            z: 0.0,
        }),
    )
    .unwrap();

    let viewport = viewport(&dom);

    let camera = viewport.camera.unwrap();
    assert_eq!(camera.eye(), Vec3::new(1.0, 2.0, 3.0));
    assert!((camera.fov_degrees - 60.0).abs() < 1e-4);
    assert_eq!(viewport.alpha, 0.5);
    assert_eq!(viewport.tint, [0.0, 0.0, 1.0]);
    assert_eq!(viewport.light, Vec3::Y);
}

#[test]
fn without_a_camera_there_is_nothing_to_look_through() {
    let (mut dom, gui) = screen_gui();
    let frame = dom.new_instance("ViewportFrame", "ViewportFrame", Some(gui));
    dom.set_property(frame, "Size", udim2(1.0, 0, 1.0, 0))
        .unwrap();
    part(&mut dom, frame, 0.0, 255);

    let viewport = viewport(&dom);

    assert!(viewport.camera.is_none());
    assert_eq!(viewport.parts.len(), 1);
}

#[test]
fn a_plain_frame_carries_no_scene() {
    let (mut dom, gui) = screen_gui();
    frame(&mut dom, gui, udim2(0.0, 0, 0.0, 0), udim2(1.0, 0, 1.0, 0));

    let elements = resolve(&screens(&dom), VIEWPORT);

    assert!(elements[0].viewport.is_none());
}

// The frame's scene is its own subtree, however deep, and never the
// workspace's; and the workspace never draws the frame's parts either.
#[test]
fn only_the_parts_under_the_frame_are_its_scene() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    part(&mut dom, workspace, 100.0, 1);
    let gui = dom.new_instance("ScreenGui", "ScreenGui", None);
    let frame = viewport_frame(&mut dom, gui, 10.0);
    let model = dom.new_instance("Model", "Model", Some(frame));
    part(&mut dom, model, -3.0, 2);
    part(&mut dom, frame, 3.0, 3);
    let database = ReflectionDatabase::embedded();

    let scene = Scene::from_dom(&dom, &database).unwrap();
    let viewport = viewport(&dom);

    let mut xs: Vec<f32> = viewport
        .parts
        .iter()
        .map(|part| part.transform.w_axis.x)
        .collect();
    xs.sort_by(f32::total_cmp);
    assert_eq!(xs, vec![-3.0, 3.0]);
    assert_eq!(scene.parts().len(), 1);
    assert_eq!(scene.parts()[0].transform.w_axis.x, 100.0);
}

// At a vertical field of view of 90 degrees a camera 10 studs back sees 10
// studs up and, at an aspect of 2, 20 studs across.
#[test]
fn the_camera_matrix_frames_the_field_of_view_at_the_boxs_aspect() {
    let camera = ViewCamera {
        cframe: glam::Mat4::from_translation(Vec3::new(0.0, 0.0, 10.0)),
        fov_degrees: 90.0,
    };
    let view_projection = camera.view_projection(2.0);
    let ndc = |point: Vec3| {
        let clip = view_projection * Vec4::from((point, 1.0));
        clip.truncate() / clip.w
    };

    let centre = ndc(Vec3::ZERO);
    assert!(centre.x.abs() < 1e-5 && centre.y.abs() < 1e-5);
    assert!((ndc(Vec3::new(0.0, 10.0, 0.0)).y - 1.0).abs() < 1e-4);
    assert!((ndc(Vec3::new(20.0, 0.0, 0.0)).x - 1.0).abs() < 1e-4);
    // Reversed-Z: the near plane is depth 1, and 10 studs out is less.
    let depth = centre.z;
    assert!(depth > 0.0 && depth < 1.0);
    assert!(ndc(Vec3::new(0.0, 0.0, -100.0)).z < depth);
}
