//! One red part in front of a camera, baked into a small texture and read
//! back: the middle pixel is the part, the corner is the transparent clear
//! the frame's own background shows through — and, one level up, the
//! element now carries that texture as its image, so the quad build emits a
//! textured run for it.

use rbx_dom::{CFrameData, UDim, UDim2, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use glam::Vec3;

use super::batch::MAX_SIDE;
use super::*;
use crate::renderer::gui::quads;
use crate::renderer::gui::text::Typesetter;
use crate::renderer::material::{self, Materials};
use crate::scene::{gui_layout_with, GuiRect, Scene};

const SIZE: (u32, u32) = (32, 32);

fn cframe(x: f32, y: f32, z: f32) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data { x, y, z },
        rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
    })
}

fn whole() -> Variant {
    Variant::UDim2(UDim2 {
        x: UDim {
            scale: 1.0,
            offset: 0,
        },
        y: UDim {
            scale: 1.0,
            offset: 0,
        },
    })
}

/// A workspace with one part (a scene needs one), and a full-screen
/// `ViewportFrame` looking at a red 2-stud cube from 10 studs back.
fn place() -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let gui = dom.new_instance("ScreenGui", "ScreenGui", None);
    dom.set_property(gui, "ScreenInsets", Variant::Enum(0))
        .unwrap();
    let frame = dom.new_instance("ViewportFrame", "ViewportFrame", Some(gui));
    dom.set_property(frame, "Size", whole()).unwrap();
    dom.set_property(frame, "BackgroundTransparency", Variant::Float32(1.0))
        .unwrap();
    let camera = dom.new_instance("Camera", "Camera", Some(frame));
    dom.set_property(camera, "CFrame", cframe(0.0, 0.0, 10.0))
        .unwrap();
    dom.set_property(frame, "CurrentCamera", Variant::Ref(camera))
        .unwrap();
    for (parent, x) in [(workspace, 100.0), (frame, 0.0)] {
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
            Variant::Color3uint8 { r: 255, g: 0, b: 0 },
        )
        .unwrap();
    }
    dom
}

/// The texture's pixels, RGBA8 in row-major order.
fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let (width, height) = (texture.width(), texture.height());
    // A copy row has to be a multiple of 256 bytes.
    let row = (width * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(height),
            },
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |result| {
        result.expect("the readback maps")
    });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("the GPU never caught up");
    let mapped = slice.get_mapped_range().expect("the readback is mapped");
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        let start = (y * row) as usize;
        pixels.extend_from_slice(&mapped[start..start + (width * 4) as usize]);
    }
    pixels
}

#[test]
fn a_red_part_lands_in_the_middle_over_a_transparent_clear() {
    let Some((device, queue)) = crate::gpu::for_tests() else {
        return;
    };
    let database = ReflectionDatabase::embedded();
    let dom = place();
    let scene = Scene::from_dom(&dom, &database).unwrap();
    let quality = QualityLevel::default().profile();
    let material_layout = material::layout(&device);
    let materials = Materials::new(
        &device,
        &queue,
        &material_layout,
        scene.materials(),
        &quality,
    );
    let mut viewports = Viewports::new(&device, &queue, &material_layout, &quality);
    let mut atlas = Atlas::new(
        &device,
        &queue,
        &[],
        &crate::load::Answered::new(),
        &quality,
    );
    let mut fonts = Typesetter::new();

    let mut elements = gui_layout_with(
        scene.gui_screens(),
        [SIZE.0 as f32, SIZE.1 as f32],
        &mut fonts,
    );
    assert_eq!(elements.len(), 1);
    assert!(elements[0].image.is_none());
    viewports.bake_all(
        &device,
        &queue,
        &materials.bind_group,
        &mut atlas,
        "test",
        &mut elements,
    );

    // The frame now shows its bake as an image, tinted and faded like an
    // `ImageLabel`'s, and the quad build samples it rather than the flat
    // white texel a plain background does.
    let image = elements[0]
        .image
        .as_ref()
        .expect("the bake became the image");
    assert_eq!(image.tint, [1.0; 3]);
    assert_eq!(image.alpha, 1.0);
    let slot = atlas.slot_of()[&image.asset];
    assert_eq!(slot.size, [SIZE.0 as f32, SIZE.1 as f32]);
    let (_, runs, _) = quads::build(&elements, atlas.slot_of(), SIZE, &mut fonts);
    assert!(runs.iter().any(|run| run.texture == slot.linear));
    assert!(runs.iter().all(|run| run.texture != quads::WHITE));

    let pixels = read_back(&device, &queue, &viewports.textures[&image.asset]);
    let at = |x: u32, y: u32| {
        let start = ((y * SIZE.0 + x) * 4) as usize;
        &pixels[start..start + 4]
    };
    let middle = at(SIZE.0 / 2, SIZE.1 / 2);
    assert!(
        middle[0] > 150 && middle[1] < 20 && middle[2] < 20 && middle[3] == 255,
        "the part is lit red in the middle: {middle:?}"
    );
    assert_eq!(at(0, 0)[3], 0, "nothing drawn in the corner");

    // A second layout at another size replaces the texture under the same
    // key instead of growing the atlas.
    let groups = atlas.groups().len();
    let mut again = gui_layout_with(scene.gui_screens(), [16.0, 8.0], &mut fonts);
    viewports.bake_all(
        &device,
        &queue,
        &materials.bind_group,
        &mut atlas,
        "test",
        &mut again,
    );
    assert_eq!(atlas.groups().len(), groups);
    assert_eq!(
        atlas.slot_of()[&again[0].image.as_ref().unwrap().asset].size,
        [16.0, 8.0]
    );
}

// The texture is only worth baking where something would show in it.
#[test]
fn a_frame_with_no_camera_or_no_box_is_left_without_an_image() {
    let Some((device, queue)) = crate::gpu::for_tests() else {
        return;
    };
    let database = ReflectionDatabase::embedded();
    let mut dom = place();
    let frame = dom
        .root_refs()
        .iter()
        .flat_map(|&root| dom.get(root).unwrap().children().to_vec())
        .find(|&child| dom.get(child).unwrap().class() == "ViewportFrame")
        .unwrap();
    dom.set_property(
        frame,
        "CurrentCamera",
        Variant::Ref(rbx_dom::Ref::new(u32::MAX)),
    )
    .unwrap();
    let scene = Scene::from_dom(&dom, &database).unwrap();
    let quality = QualityLevel::default().profile();
    let material_layout = material::layout(&device);
    let materials = Materials::new(
        &device,
        &queue,
        &material_layout,
        scene.materials(),
        &quality,
    );
    let mut viewports = Viewports::new(&device, &queue, &material_layout, &quality);
    let mut atlas = Atlas::new(
        &device,
        &queue,
        &[],
        &crate::load::Answered::new(),
        &quality,
    );
    let mut fonts = Typesetter::new();

    let mut elements = gui_layout_with(scene.gui_screens(), [32.0, 32.0], &mut fonts);
    viewports.bake_all(
        &device,
        &queue,
        &materials.bind_group,
        &mut atlas,
        "test",
        &mut elements,
    );
    assert!(elements[0].image.is_none());

    assert_eq!(
        pixels(&GuiRect {
            x: 0.0,
            y: 0.0,
            width: 0.4,
            height: 10.0
        }),
        None
    );
    assert_eq!(
        pixels(&GuiRect {
            x: 0.0,
            y: 0.0,
            width: 10.2,
            height: 1.0e6
        }),
        Some((11, MAX_SIDE))
    );
}

// Opaque parts go first, grouped by shape; translucent ones follow, the
// furthest from the eye first, so nearer glass blends over farther glass.
#[test]
fn the_batch_draws_opaque_shapes_first_and_glass_back_to_front() {
    let database = ReflectionDatabase::embedded();
    let mut dom = place();
    let frame = dom
        .root_refs()
        .iter()
        .flat_map(|&root| dom.get(root).unwrap().children().to_vec())
        .find(|&child| dom.get(child).unwrap().class() == "ViewportFrame")
        .unwrap();
    for (z, transparency) in [(-5.0, 0.5), (5.0, 0.5), (0.0, 0.0)] {
        let part = dom.new_instance("Part", "Part", Some(frame));
        dom.set_property(part, "CFrame", cframe(0.0, 0.0, z))
            .unwrap();
        dom.set_property(
            part,
            "size",
            Variant::Vector3(Vector3Data {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            }),
        )
        .unwrap();
        dom.set_property(part, "Transparency", Variant::Float32(transparency))
            .unwrap();
    }
    let scene = Scene::from_dom(&dom, &database).unwrap();
    let elements = gui_layout_with(scene.gui_screens(), [32.0, 32.0], &mut Typesetter::new());
    let viewport = elements[0].viewport.as_ref().unwrap();
    let parts: Vec<&Part> = viewport.parts.iter().collect();

    let (instances, runs) = batch(&parts, Vec3::new(0.0, 0.0, 10.0));

    assert_eq!(instances.len(), 4);
    assert_eq!(runs.len(), 2);
    assert!(!runs[0].blended && runs[0].instances == (0..2));
    assert!(runs[1].blended && runs[1].instances == (2..4));
    // The far pane (z = -5) is uploaded before the near one (z = 5).
    assert_eq!(instances[2].center().z, -5.0);
    assert_eq!(instances[3].center().z, 5.0);
}
