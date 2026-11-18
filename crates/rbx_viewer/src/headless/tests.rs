use rbx_dom::{Instance, Ref, WeakDom};

use super::*;

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
    )
    .expect("the no-op device builds anything");

    assert_eq!(headless.rung.level, 3);
    assert!(matches!(headless.quality, QualityLevel::Level(level) if level <= 3));
    assert_eq!(gpu::ooms(), ooms);
}
