//! The transform gizmo's draggers, drawn as solid arrows over the scene.
//!
//! Where [`crate::gizmo`] says where the handles *are* — the half the editor's
//! UI thread hit-tests the cursor against — this is the half that turns those
//! same [`Handles`] into triangles. One arrow per axis in each direction,
//! coloured red/green/blue, rebuilt every frame because the arms are scaled to
//! keep a constant size on screen and so change with every camera move.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::gizmo::{Axis, Handles, HEAD_RADIUS, HEAD_START, SHAFT_RADIUS, SHAFT_START};

use super::pipeline::{self, Surface, Target};

const SHADER: &str = include_str!("gizmo.wgsl");

/// How many segments go round a shaft or an arrowhead. Eight already reads as
/// round at the size a dragger occupies on screen, and the whole gizmo is one
/// small vertex buffer rewritten every frame — there is nothing to gain from
/// more.
const SEGMENTS: usize = 8;
/// Three axes, each with an arm in both directions.
const ARMS: usize = 6;
/// Per arm: the shaft's sides and its open end's cap, then the arrowhead's
/// sides and base.
const VERTICES_PER_ARM: usize = SEGMENTS * (6 + 3 + 3 + 3);
const CAPACITY: usize = ARMS * VERTICES_PER_ARM;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
}

/// Named rather than written inline in [`Vertex::layout`]: a two-attribute
/// array is not const-promoted the way a one-attribute one is, so the
/// temporary would not outlive the layout that borrows it.
const ATTRIBUTES: [wgpu::VertexAttribute; 2] =
    wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];

impl Vertex {
    const fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &ATTRIBUTES,
        }
    }
}

/// The six arms of one gizmo, furthest from `eye` first.
///
/// Back-to-front because the draggers are drawn with the depth test off (a
/// handle inside the part it moves still has to be grabbable, so it cannot
/// lose to the geometry around it) — which leaves paint order as the only
/// thing deciding which arm covers which where two overlap on screen. Sorting
/// here is enough: the arms never intersect each other, so a painter's order
/// over six convex pieces is exact rather than approximate.
fn arms(handles: &Handles, eye: Vec3) -> Vec<Vertex> {
    let mut arms: Vec<(f32, Axis, f32)> = Axis::ALL
        .into_iter()
        .flat_map(|axis| [(axis, 1.0f32), (axis, -1.0f32)])
        .map(|(axis, sign)| {
            let tip = handles.origin() + handles.direction(axis) * handles.arm() * sign;
            ((tip - eye).length(), axis, sign)
        })
        .collect();
    arms.sort_by(|(a, ..), (b, ..)| b.total_cmp(a));

    let mut vertices = Vec::with_capacity(CAPACITY);
    for (_, axis, sign) in arms {
        arrow(
            &mut vertices,
            handles.origin(),
            handles.direction(axis) * sign,
            handles.arm(),
            axis.color(),
        );
    }
    vertices
}

/// One arrow: a thin shaft capped at the end nearest the gizmo's centre, then
/// a cone for the head.
///
/// Every triangle is wound counter-clockwise seen from outside, so back-face
/// culling removes the far side of each piece. That is what keeps an arrow
/// from painting over itself once the depth test is off.
fn arrow(vertices: &mut Vec<Vertex>, origin: Vec3, direction: Vec3, arm: f32, color: [f32; 3]) {
    // `direction` is the arrow's own "up"; `across`/`round` complete a
    // right-handed frame (`across × round == direction`), which is what makes
    // the ring below run counter-clockwise seen from the tip.
    let across = direction.any_orthonormal_vector();
    let round = direction.cross(across);
    let ring = |radius: f32, offset: f32, segment: usize| {
        let angle = std::f32::consts::TAU * segment as f32 / SEGMENTS as f32;
        origin + direction * offset + (across * angle.cos() + round * angle.sin()) * radius
    };

    let shaft = SHAFT_RADIUS * arm;
    let head = HEAD_RADIUS * arm;
    let (start, neck, tip) = (SHAFT_START * arm, HEAD_START * arm, arm);
    let apex = origin + direction * tip;
    let base = origin + direction * start;

    let mut push = |position: Vec3| {
        vertices.push(Vertex {
            position: position.into(),
            color,
        })
    };
    for segment in 0..SEGMENTS {
        let (near, far) = (segment, segment + 1);

        // The shaft's side, as two triangles of one quad.
        let (a, b) = (ring(shaft, start, near), ring(shaft, start, far));
        let (c, d) = (ring(shaft, neck, near), ring(shaft, neck, far));
        for point in [a, b, d, a, d, c] {
            push(point);
        }

        // Its open end, facing back towards the gizmo's centre — hence the
        // reversed winding.
        for point in [base, b, a] {
            push(point);
        }

        // The arrowhead: a cone side and the disc it stands on, which faces
        // back down the shaft for the same reason.
        let (a, b) = (ring(head, neck, near), ring(head, neck, far));
        for point in [a, b, apex, origin + direction * neck, b, a] {
            push(point);
        }
    }
}

/// The gizmo's GPU state: one unlit, vertex-coloured triangle pipeline and the
/// small vertex buffer rewritten each frame.
pub(super) struct Draggers {
    pipeline: wgpu::RenderPipeline,
    vertices: wgpu::Buffer,
    count: u32,
}

impl Draggers {
    pub(super) fn new(
        device: &wgpu::Device,
        target: Target,
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let pipeline = pipeline::surface(
            device,
            target,
            &Surface {
                // Never occluded: a handle is a control, not scenery, and one
                // buried inside the part it moves would be impossible to
                // grab. `translucent` here is only borrowed for its second
                // effect — leaving the depth buffer alone, which the
                // depth-of-field and fog passes downstream still read as the
                // real scene's.
                compare: wgpu::CompareFunction::Always,
                translucent: true,
                ..Surface::new(
                    "rbxview gizmo",
                    SHADER,
                    &[Some(frame_layout)],
                    &[Some(Vertex::layout())],
                )
            },
        );

        Draggers {
            pipeline,
            vertices: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("rbxview gizmo"),
                size: (CAPACITY * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            count: 0,
        }
    }

    /// Rebuilds this frame's arrows, or draws none at all when nothing is
    /// selected or no transform tool is active.
    pub(super) fn update(&mut self, queue: &wgpu::Queue, handles: Option<Handles>, eye: Vec3) {
        let Some(handles) = handles else {
            self.count = 0;
            return;
        };

        let vertices = arms(&handles, eye);
        self.count = vertices.len() as u32;
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(&vertices));
    }

    pub(super) fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) {
        if self.count == 0 {
            return;
        }

        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, frame, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        pass.draw(0..self.count, 0..1);
    }
}

#[cfg(test)]
#[path = "gizmo/tests.rs"]
mod tests;
