//! The blended pass for shapes: every part whose `Transparency` sits strictly
//! between 0 and 1, drawn after all the opaque geometry.
//!
//! Alpha blending is order-dependent, so the instances are re-sorted back to
//! front every frame and uploaded in that order into one shared buffer. Runs of
//! neighbouring instances that happen to share a shape become one draw call
//! each, which is how a single sorted sequence can still be instanced across six
//! different unit meshes.

use std::collections::HashMap;

use glam::Vec3;
use rbx_dom::Ref;
use wgpu::util::DeviceExt;

use super::cull::MainCull;
use super::geometry::Meshes;
use super::instance::InstanceRaw;
use crate::scene::{of_part, Part, ShapeKind};

/// One translucent instance, kept on the CPU: the sort order depends on the
/// camera, so the GPU copy is rewritten every frame rather than built once.
struct Item {
    referent: Ref,
    kind: ShapeKind,
    center: Vec3,
    /// World-space bounding radius, for the same cull test the opaque pass
    /// uses (see `super::cull::MainCull`) — checked in [`Translucent::prepare`]
    /// before an item is even considered for sorting.
    radius: f32,
    instance: InstanceRaw,
}

/// A stretch of the sorted buffer sharing one shape, i.e. one draw call.
struct Run {
    kind: ShapeKind,
    instances: std::ops::Range<u32>,
}

pub(super) struct Translucent {
    items: Vec<Item>,
    /// Where each referent's `Item` sits in `items` — flat, not grouped by
    /// shape (unlike `renderer::shaped::Shaped`), since [`Translucent::prepare`]
    /// already re-sorts and re-uploads the whole buffer every frame; an edit
    /// here only has to update the CPU-side item, not write the GPU directly.
    part_index: HashMap<Ref, usize>,
    /// Rebuilt every frame; kept around so the sort allocates nothing.
    order: Vec<usize>,
    uploaded: Vec<InstanceRaw>,
    runs: Vec<Run>,
    /// Sized for `capacity` items, which is at least `items.len()`: an item
    /// added past it (see [`Translucent::sync`]) reallocates rather than
    /// overrunning the next `prepare`'s upload.
    instances: Option<wgpu::Buffer>,
    capacity: usize,
}

impl Translucent {
    pub(super) fn new(device: &wgpu::Device, parts: &[Part]) -> Self {
        let items: Vec<Item> = parts
            .iter()
            .filter(|part| belongs(part))
            .map(Item::of)
            .collect();
        let part_index = items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.referent, index))
            .collect();

        let instances = (!items.is_empty()).then(|| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("rbxview translucent instances"),
                contents: bytemuck::cast_slice(
                    &items.iter().map(|item| item.instance).collect::<Vec<_>>(),
                ),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            })
        });

        Translucent {
            order: Vec::with_capacity(items.len()),
            uploaded: Vec::with_capacity(items.len()),
            runs: Vec::new(),
            capacity: items.len(),
            items,
            part_index,
            instances,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Brings this pass in line with one edited part: its item rewritten,
    /// added or dropped on the CPU side (see [`Translucent::place`]), and the
    /// buffer the next [`Translucent::prepare`] uploads into grown if the
    /// item is one more than it can hold.
    pub(super) fn sync(&mut self, device: &wgpu::Device, part: &Part) {
        self.place(part);
        if self.items.len() <= self.capacity {
            return;
        }
        self.capacity = (self.capacity * 2).max(self.items.len());
        self.instances = Some(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("rbxview translucent instances"),
            size: (self.capacity * std::mem::size_of::<InstanceRaw>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }

    /// Takes one part out of this pass — it stopped blending, or is gone.
    /// CPU-side only, like [`Translucent::place`]; a no-op for a referent
    /// not held.
    pub(super) fn remove(&mut self, referent: Ref) {
        let Some(index) = self.part_index.remove(&referent) else {
            return;
        };
        self.items.swap_remove(index);
        if let Some(moved) = self.items.get(index) {
            self.part_index.insert(moved.referent, index);
        }
    }

    /// The CPU half of [`Translucent::sync`], which touches no buffer: the
    /// next `prepare` re-sorts and re-uploads every item anyway, so all an
    /// edit has to keep straight is `items` and the index into it. A removal
    /// is a swap-remove, re-indexing whichever item dropped into the hole.
    fn place(&mut self, part: &Part) {
        match (self.part_index.get(&part.referent).copied(), belongs(part)) {
            (Some(index), true) => self.items[index] = Item::of(part),
            (Some(index), false) => {
                self.part_index.remove(&part.referent);
                self.items.swap_remove(index);
                if let Some(moved) = self.items.get(index) {
                    self.part_index.insert(moved.referent, index);
                }
            }
            (None, true) => {
                self.items.push(Item::of(part));
                self.part_index.insert(part.referent, self.items.len() - 1);
            }
            (None, false) => {}
        }
    }

    /// Re-sorts and re-uploads for this frame's camera. Must run before
    /// [`Translucent::draw`], and outside the render pass since it writes a
    /// buffer the pass reads.
    ///
    /// `cull` is checked before sorting even begins: an item it rules out
    /// never enters `order`, so it costs neither a sort comparison nor a spot
    /// in the uploaded buffer, let alone a draw call.
    pub(super) fn prepare(&mut self, queue: &wgpu::Queue, eye: Vec3, cull: &MainCull<'_>) {
        let Some(buffer) = &self.instances else {
            return;
        };

        back_to_front(
            &self.items,
            eye,
            |item| cull.visible(item.center, item.radius),
            &mut self.order,
        );
        self.runs = runs(&self.items, &self.order);
        self.uploaded.clear();
        self.uploaded
            .extend(self.order.iter().map(|&index| self.items[index].instance));

        queue.write_buffer(buffer, 0, bytemuck::cast_slice(&self.uploaded));
    }

    /// Draws the sorted runs. The caller owns the pipeline and bind group 0.
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, meshes: &Meshes) {
        let Some(buffer) = &self.instances else {
            return;
        };

        pass.set_vertex_buffer(1, buffer.slice(..));
        for run in &self.runs {
            if let Some(mesh) = meshes.get(run.kind) {
                mesh.draw_range(pass, run.instances.clone());
            }
        }
    }
}

impl Item {
    fn of(part: &Part) -> Self {
        let instance = InstanceRaw::from_part(part);
        Item {
            referent: part.referent,
            kind: part.kind,
            center: instance.center(),
            radius: of_part(part).radius(),
            instance,
        }
    }
}

fn belongs(part: &Part) -> bool {
    part.is_drawn() && part.is_translucent()
}

/// Orders the visible items furthest first, so nearer surfaces blend over what
/// is already behind them.
///
/// `visible` filters before the sort rather than after: an item it rejects
/// never becomes a comparison, let alone an upload. Squared distance: sorting
/// orders the same as the real one, and a square root per instance per frame
/// buys nothing. A stable sort keeps instances at equal distance in the order
/// the DOM listed them, so a frame never flickers between two equally valid
/// orders.
fn back_to_front(
    items: &[Item],
    eye: Vec3,
    visible: impl Fn(&Item) -> bool,
    order: &mut Vec<usize>,
) {
    order.clear();
    order.extend((0..items.len()).filter(|&index| visible(&items[index])));
    order.sort_by(|&left, &right| {
        let far = |index: usize| (items[index].center - eye).length_squared();
        far(right).total_cmp(&far(left))
    });
}

/// Splits a sorted order into the longest possible runs of one shape.
///
/// The sort comes first and the batching second — never the other way round, or
/// a near window would blend under a far one just because they were different
/// shapes.
fn runs(items: &[Item], order: &[usize]) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (position, &index) in order.iter().enumerate() {
        let next = position as u32;
        match runs.last_mut() {
            Some(run) if run.kind == items[index].kind => run.instances.end = next + 1,
            _ => runs.push(Run {
                kind: items[index].kind,
                instances: next..next + 1,
            }),
        }
    }
    runs
}

#[cfg(test)]
#[path = "translucent/tests.rs"]
mod tests;
