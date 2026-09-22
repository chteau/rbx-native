//! GPU tests for [`super`]: that the pipeline accepts what the quads
//! emit, and that a translucent quad composites in encoded space.

use rbx_dom::{Color3Data, ColorSequence, ColorSequenceKeypoint, NumberSequence};

use super::*;
use crate::renderer::texture;
use crate::scene::{GuiGradient, GuiGradientKind, GuiJoin, GuiRect, GuiStroke, GuiTile};

// The pipeline has to accept the vertex layout the quads emit, and a
// rounded, stroked, shaded element has to make it through a whole
// prepare-and-draw — which is the one thing a CPU-side test cannot say.
#[test]
fn a_rounded_stroked_shaded_element_draws_through_the_pipeline() {
    let Some((device, queue)) = crate::gpu::for_tests() else {
        return;
    };
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let viewport_layout = pipeline::viewport_layout(&device);
    let image_layout = texture::layout(&device);
    let mut painter = Painter::new(&device, &queue, format, &viewport_layout, &image_layout);

    let element = GuiElement {
        referent: rbx_dom::Ref::new(0),
        editable: Default::default(),
        rect: GuiRect {
            x: 8.0,
            y: 8.0,
            width: 48.0,
            height: 32.0,
        },
        clip: None,
        rotation: 30.0,
        background: [1.0, 1.0, 1.0],
        background_alpha: 1.0,
        border: None,
        image: None,
        border_inset: 0.0,
        z_index: 1,
        corner_radii: [8.0; 4],
        strokes: vec![GuiStroke {
            color: [0.0; 3],
            alpha: 1.0,
            band: [0.0, 3.0],
            join: GuiJoin::Round,
            on_text: false,
            thickness: 3.0,
            scaled: false,
        }],
        gradient: Some(GuiGradient {
            color: ColorSequence {
                keypoints: vec![ColorSequenceKeypoint {
                    time: 0.0,
                    color: Color3Data {
                        r: 1.0,
                        g: 0.0,
                        b: 0.0,
                    },
                    envelope: 0.0,
                }],
            },
            transparency: NumberSequence { keypoints: vec![] },
            origin: [0.0, 0.0],
            axis: [1.0 / 48.0, 0.0],
            kind: GuiGradientKind::Linear,
            tile: GuiTile::Clamp,
        }),
        text: None,
        viewport: None,
        group: None,
        scroll: None,
    };
    let size = (64, 48);
    painter.prepare(
        &device,
        &queue,
        &[element],
        &HashMap::new(),
        size,
        &mut Typesetter::new(),
    );

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
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[pipeline::encoded(format)],
    });
    let view = pipeline::encoded_view(&target);
    let white = texture::Uploaded::color(
        &device,
        &queue,
        &crate::assets::Image {
            width: 1,
            height: 1,
            pixels: vec![255; 4],
        },
    );
    let sampler = texture::sampler(&device, wgpu::AddressMode::Repeat, 1);
    let groups = [white.bind(&device, &image_layout, &sampler, 1)];
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter.draw(
        &mut encoder,
        &view,
        wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
        &groups,
        size,
    );
    queue.submit(std::iter::once(encoder.finish()));
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("the GPU never caught up");
}

// The whole point of the non-sRGB view: a `BackgroundTransparency` of 0.5
// over an encoded grey of 200 has to leave 100, the way Studio composites
// it. The same quad blended in linear light would leave 146.
#[test]
fn a_half_transparent_black_quad_halves_the_encoded_pixel_under_it() {
    let Some((device, queue)) = crate::gpu::for_tests() else {
        return;
    };
    const GREY: u8 = 200;
    let size = (64u32, 64u32);
    let format = wgpu::TextureFormat::Rgba8UnormSrgb;
    let viewport_layout = pipeline::viewport_layout(&device);
    let image_layout = texture::layout(&device);
    let mut painter = Painter::new(&device, &queue, format, &viewport_layout, &image_layout);

    let element = GuiElement {
        referent: rbx_dom::Ref::new(0),
        editable: Default::default(),
        rect: GuiRect {
            x: 0.0,
            y: 0.0,
            width: size.0 as f32,
            height: size.1 as f32,
        },
        clip: None,
        rotation: 0.0,
        background: [0.0, 0.0, 0.0],
        background_alpha: 0.5,
        border: None,
        image: None,
        border_inset: 0.0,
        z_index: 1,
        corner_radii: [0.0; 4],
        strokes: Vec::new(),
        gradient: None,
        text: None,
        viewport: None,
        group: None,
        scroll: None,
    };
    painter.prepare(
        &device,
        &queue,
        &[element],
        &HashMap::new(),
        size,
        &mut Typesetter::new(),
    );

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
        view_formats: &[pipeline::encoded(format)],
    });
    let white = texture::Uploaded::color(
        &device,
        &queue,
        &crate::assets::Image {
            width: 1,
            height: 1,
            pixels: vec![255; 4],
        },
    );
    let sampler = texture::sampler(&device, wgpu::AddressMode::Repeat, 1);
    let groups = [white.bind(&device, &image_layout, &sampler, 1)];

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    painter.draw(
        &mut encoder,
        &pipeline::encoded_view(&target),
        // Cleared through that same non-sRGB view, so this is the literal
        // byte the quad blends over.
        wgpu::LoadOp::Clear(wgpu::Color {
            r: f64::from(GREY) / 255.0,
            g: f64::from(GREY) / 255.0,
            b: f64::from(GREY) / 255.0,
            a: 1.0,
        }),
        &groups,
        size,
    );
    let pixels = read_back(&device, &queue, encoder, &target, size);

    // The centre, clear of the one pixel of coverage the fill ramps over
    // at its edges.
    let centre = ((size.1 / 2) * size.0 + size.0 / 2) as usize * 4;
    for channel in 0..3 {
        let value = pixels[centre + channel];
        assert!(
            (GREY / 2).abs_diff(value) <= 1,
            "channel {channel} read {value}, expected about {}",
            GREY / 2
        );
    }
}

/// An RGBA8 copy of `target`, one row of `size.0` pixels after another.
fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mut encoder: wgpu::CommandEncoder,
    target: &wgpu::Texture,
    size: (u32, u32),
) -> Vec<u8> {
    // No row padding: only sizes whose rows already meet
    // `COPY_BYTES_PER_ROW_ALIGNMENT`, which is all this test needs.
    let row = size.0 * 4;
    assert_eq!(row % wgpu::COPY_BYTES_PER_ROW_ALIGNMENT, 0);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row) * u64::from(size.1),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
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
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| ());
    device
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: None,
        })
        .expect("the GPU never caught up");
    let pixels = buffer
        .slice(..)
        .get_mapped_range()
        .expect("the readback buffer never mapped")
        .to_vec();
    buffer.unmap();
    pixels
}
