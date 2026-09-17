//! Unit tests for [`super`]: the local frame a group's run is moved into,
//! and — on a GPU — the one blend a flattened group comes to.

use std::collections::HashMap;
use std::sync::mpsc;

use super::super::pipeline;
use super::*;
use crate::load::Answered;
use crate::quality::QualityLevel;
use crate::scene::{GuiGroup, GuiGroupTint};

fn boxed(x: f32, y: f32, width: f32, height: f32, alpha: f32) -> GuiElement {
    GuiElement {
        rect: GuiRect {
            x,
            y,
            width,
            height,
        },
        clip: None,
        rotation: 0.0,
        background: [1.0, 0.0, 0.0],
        background_alpha: alpha,
        border: None,
        border_inset: 0.0,
        z_index: 1,
        image: None,
        corner_radii: [0.0; 4],
        stroke: None,
        gradient: None,
        text: None,
        group: None,
        viewport: None,
    }
}

fn group(rect: GuiRect, alpha: f32, descendants: usize) -> GuiElement {
    GuiElement {
        rect,
        group: Some(GuiGroup {
            tint: GuiGroupTint {
                color: [1.0; 3],
                alpha,
            },
            descendants,
            texture: None,
        }),
        ..boxed(0.0, 0.0, 0.0, 0.0, 0.0)
    }
}

// A group's run is painted in the group's own frame: origin at its corner,
// and its rotation — which every descendant carries — taken back out, so the
// picture is upright and turns as one with the group's quad.
#[test]
fn a_run_is_moved_into_the_groups_own_upright_frame() {
    let rect = GuiRect {
        x: 100.0,
        y: 100.0,
        width: 50.0,
        height: 20.0,
    };
    let pivot = [125.0, 110.0];
    let mut parent = group(rect, 0.5, 1);
    parent.rotation = 30.0;
    parent.clip = Some(GuiRect {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
    });
    // Sits 10 px in and 5 px down from the group's corner, carried around
    // the group's centre the way the layout carries a child.
    let mut child = boxed(110.0, 105.0, 10.0, 10.0, 1.0);
    child.rect = child.rect.turned(30.0, pivot);
    child.rotation = 30.0;

    let local = local(&[parent, child]);

    assert_eq!(local[0].rotation, 0.0);
    assert_eq!(local[0].clip, None, "the quad is clipped instead");
    assert_eq!(local[0].group, None, "or it would be flattened again");
    assert!((local[0].rect.x).abs() < 1e-3 && (local[0].rect.y).abs() < 1e-3);
    assert_eq!(local[1].rotation, 0.0);
    assert!((local[1].rect.x - 10.0).abs() < 1e-3);
    assert!((local[1].rect.y - 5.0).abs() < 1e-3);
    assert_eq!(local[1].rect.width, 10.0);
}

// Two opaque children overlapping under a half-transparent group must come
// to *one* blend with what is behind the group: half red over black is
// exactly half red, wherever both or only one of the children paints. Two
// blends — each child faded on its own — would leave the overlap at three
// quarters.
#[test]
fn a_half_transparent_group_blends_its_overlapping_children_once() {
    let Some((device, queue)) = crate::gpu::for_tests() else {
        return;
    };
    // Linear, so the bytes read back are the arithmetic itself.
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let size = (64, 64);
    let quality = QualityLevel::Automatic.profile();
    let mut atlas = Atlas::new(&device, &queue, &[], &Answered::new(), &quality);
    let viewport_layout = pipeline::viewport_layout(&device);
    let mut painter = Painter::new(
        &device,
        &queue,
        format,
        &viewport_layout,
        &atlas.image_layout,
    );
    let mut fonts = Typesetter::new();
    let mut baked = Baked::new(&device);

    let whole = GuiRect {
        x: 0.0,
        y: 0.0,
        width: 64.0,
        height: 64.0,
    };
    let mut first = boxed(0.0, 0.0, 48.0, 48.0, 1.0);
    let mut second = boxed(16.0, 16.0, 48.0, 48.0, 1.0);
    first.clip = Some(whole);
    second.clip = Some(whole);
    let elements = vec![group(whole, 0.5, 2), first, second];

    let flat = flatten(
        &mut Bake {
            device: &device,
            queue: &queue,
            painter: &mut painter,
            atlas: &mut atlas,
            fonts: &mut fonts,
            format,
            baked: &mut baked,
        },
        elements,
    );
    assert_eq!(flat.len(), 1, "the run collapsed into the group");
    let slot = flat[0].group.and_then(|group| group.texture);
    assert_eq!(
        slot,
        Some(atlas.groups().len()),
        "the first slot past the atlas'"
    );

    painter.prepare(&device, &queue, &flat, &HashMap::new(), size, &mut fonts);
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());
    // 64 px × 4 bytes is exactly the 256-byte row alignment a copy wants.
    let bytes_per_row = size.0 * 4;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(bytes_per_row * size.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter.draw(
        &mut encoder,
        &view,
        wgpu::LoadOp::Clear(wgpu::Color::BLACK),
        &baked.bindings(&atlas),
        size,
    );
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(size.1),
            },
        },
        wgpu::Extent3d {
            width: size.0,
            height: size.1,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));
    let (sender, mapped) = mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("the GPU never caught up");
    mapped
        .recv()
        .expect("the map callback ran")
        .expect("the readback mapped");
    let pixels = readback
        .slice(..)
        .get_mapped_range()
        .expect("the mapped range is readable");
    let red = |x: u32, y: u32| pixels[((y * bytes_per_row) + x * 4) as usize];

    let overlap = red(32, 32);
    let alone = red(8, 8);
    assert!(
        (126..=129).contains(&overlap),
        "the overlap is one half-red blend, not two: {overlap}"
    );
    assert!(
        (126..=129).contains(&alone),
        "a single child fades exactly the same: {alone}"
    );
    assert_eq!(
        red(60, 4),
        0,
        "outside both children the group shows nothing"
    );
}
