use rbx_dom::{Instance, Ref, WeakDom};

use super::*;

fn noop_headless() -> Headless {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
    dom.set_parent(Ref::new(1), None);
    Headless::build(
        &dom,
        Toggles {
            textures: false,
            materials: false,
            lights: true,
            clock_time: None,
            show_development_gui: false,
        },
        ReflectionDatabase::embedded(),
        View::default(),
        3,
        Some(wgpu::Device::noop(&wgpu::DeviceDescriptor::default())),
        false,
    )
    .expect("the no-op device builds anything")
}

/// Play moves the game camera every frame, its eye one stud from the
/// point it looks at: an orthographic view stays at the place's
/// `OrthographicSize` (35 studs tall in Studio's Play, measured) rather
/// than zooming to that one stud.
#[test]
fn a_game_camera_moved_each_frame_keeps_the_orthographic_size() {
    let mut headless = noop_headless();
    headless.open_at([0.0, 10.0, 12.0], [0.0, 10.0, 11.0], 70.0);
    headless.set_orthographic(true);
    headless.set_ortho_span(35.0, crate::OrthoAxis::Vertical);
    headless.apply_ortho_span(800, 400);
    for z in [11.0, 25.0] {
        headless.open_at([0.0, 10.0, z + 1.0], [0.0, 10.0, z], 70.0);
        assert!((headless.view_pose().ortho_scale - 17.5).abs() < 1e-3);
    }
}

/// A device handed to `build` is built on once, at the rung a build
/// starts at, and keeps the whole cap to draw up to: there is no adapter
/// of its own to step down on.
#[test]
fn a_handed_in_device_builds_once_at_the_starting_rung() {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
    dom.set_parent(Ref::new(1), None);
    let ooms = gpu::ooms();

    let headless = Headless::build(
        &dom,
        Toggles {
            textures: false,
            materials: false,
            lights: true,
            clock_time: None,
            show_development_gui: false,
        },
        ReflectionDatabase::embedded(),
        View::default(),
        3,
        Some(wgpu::Device::noop(&wgpu::DeviceDescriptor::default())),
        false,
    )
    .expect("the no-op device builds anything");

    assert_eq!(headless.rung.level, 3);
    assert!(matches!(headless.quality, QualityLevel::Level(level) if level <= 3));
    assert_eq!(gpu::ooms(), ooms);
}
