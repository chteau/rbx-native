//! The adornment passes themselves: the depth-tested layer the resolve lays
//! over the frame, the always-on-top pass onto the finished frame, and the
//! order each one paints in (see `super`).

use glam::Mat4;

use super::pipelines::{Layer, Set, Top};
use super::{Adornments, Draw, Entry};
use crate::renderer::post::Targets;

impl Adornments {
    /// Draws the depth-tested adornments into the layer the resolve lays
    /// over the frame, or, `drawn` false or with none to draw, clears
    /// whatever an earlier frame left there.
    pub(in crate::renderer) fn draw_layer(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        targets: &Targets,
        frame: &wgpu::BindGroup,
        drawn: bool,
    ) {
        let drawing = drawn && !self.layered.is_empty();
        self.depth_written = drawing;
        if !drawing && !self.layer_dirty {
            return;
        }
        self.layer_dirty = drawing;
        let samples = targets.samples();
        if drawing
            && self
                .layer_gpu
                .as_ref()
                .is_none_or(|(made_for, _)| *made_for != samples)
        {
            self.layer_gpu = Some((samples, Layer::new(device, samples, &self.image_layout)));
        }
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview adornment layer"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: targets.adornment_layer(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: targets.adornment_depth(),
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let Some((_, gpu)) = self.layer_gpu.as_ref().filter(|_| drawing) else {
            return;
        };
        pass.set_bind_group(0, frame, &[]);
        pass.set_bind_group(3, targets.scene_depth(), &[]);
        for &index in &self.layered {
            let entry = &self.entries[index];
            self.draw_entry(&mut pass, gpu.set(entry.opaque), entry);
        }
    }

    /// Draws the always-on-top adornments onto `target`, the frame the
    /// resolve just finished, through its non-sRGB view so they blend in
    /// sRGB values.
    pub(in crate::renderer) fn draw_on_top(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::Texture,
        targets: &Targets,
        frame: &wgpu::BindGroup,
        drawn: bool,
    ) {
        if !drawn || self.on_top.is_empty() {
            return;
        }
        let format = target.format().remove_srgb_suffix();
        if self
            .top_gpu
            .as_ref()
            .is_none_or(|(made_for, _)| *made_for != format)
        {
            self.top_gpu = Some((format, Top::new(device, format, &self.image_layout)));
        }
        let Some((_, gpu)) = &self.top_gpu else {
            return;
        };
        let view = target.create_view(&wgpu::TextureViewDescriptor {
            format: Some(format),
            ..Default::default()
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("rbxview adornments on top"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: targets.adornment_depth(),
                depth_ops: Some(wgpu::Operations {
                    load: if self.depth_written {
                        wgpu::LoadOp::Load
                    } else {
                        wgpu::LoadOp::Clear(0.0)
                    },
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_bind_group(0, frame, &[]);
        for &index in &self.on_top {
            self.draw_entry(&mut pass, &gpu.0, &self.entries[index]);
        }
    }

    fn draw_entry(&self, pass: &mut wgpu::RenderPass<'_>, set: &Set, entry: &Entry) {
        for draw in &entry.draws {
            let (pipeline, buffer, range) = match draw {
                Draw::Solid(range) => (&set.solid, &self.solids, range.clone()),
                Draw::Line(range) => (&set.line, &self.lines, range.clone()),
                Draw::Ring(ring) => (
                    &set.line,
                    &self.ring_vertices,
                    self.rings[*ring].range.clone(),
                ),
                Draw::Picture(image, range) => {
                    pass.set_bind_group(1, &self.images[*image], &[]);
                    (&set.image, &self.pictures, range.clone())
                }
            };
            let Some(buffer) = buffer else {
                continue;
            };
            pass.set_pipeline(pipeline);
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(range, 0..1);
        }
    }
}

/// This frame's draw order, by index into `entries`: the depth-tested ones,
/// opaque first in creation order, then the blended ones far to near; and
/// the always-on-top ones by `ZIndex`, then far to near. "Far" is depth
/// along the view, not distance from the eye (Studio draws the later of two
/// side-by-side coplanar adornments over the earlier, whichever is nearer
/// the eye), and a tie keeps creation order, so the later one is drawn
/// over.
pub(super) fn draw_order(entries: &[Entry], view_projection: Mat4) -> (Vec<usize>, Vec<usize>) {
    // Clip w is the depth along the view for a perspective camera, and the
    // same for everything in an orthographic one, which leaves creation
    // order to decide.
    let distance = |index: usize| (view_projection * entries[index].centre.extend(1.0)).w;
    let far_first = |a: &usize, b: &usize| distance(*b).total_cmp(&distance(*a));
    let (mut on_top, mut layered): (Vec<usize>, Vec<usize>) =
        (0..entries.len()).partition(|&index| entries[index].on_top);
    layered.sort_by(|a, b| {
        entries[*b]
            .opaque
            .cmp(&entries[*a].opaque)
            .then_with(|| match entries[*a].opaque {
                true => std::cmp::Ordering::Equal,
                false => far_first(a, b),
            })
    });
    on_top.sort_by(|a, b| {
        entries[*a]
            .order
            .cmp(&entries[*b].order)
            .then_with(|| far_first(a, b))
    });
    (layered, on_top)
}
