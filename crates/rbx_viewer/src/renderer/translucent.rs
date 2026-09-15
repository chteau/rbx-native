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
    /// already re-sorts and re-uploads the whole buffer every frame; a patch
    /// here only has to update the CPU-side item, not write the GPU directly.
    part_index: HashMap<Ref, usize>,
    /// Rebuilt every frame; kept around so the sort allocates nothing.
    order: Vec<usize>,
    uploaded: Vec<InstanceRaw>,
    runs: Vec<Run>,
    instances: Option<wgpu::Buffer>,
}

impl Translucent {
    pub(super) fn new(device: &wgpu::Device, parts: &[Part]) -> Self {
        let items: Vec<Item> = parts
            .iter()
            .filter(|part| part.is_drawn() && part.is_translucent())
            .map(|part| {
                let instance = InstanceRaw::from_part(part);
                Item {
                    referent: part.referent,
                    kind: part.kind,
                    center: instance.center(),
                    radius: of_part(part).radius(),
                    instance,
                }
            })
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
            items,
            part_index,
            instances,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Updates one item's CPU-side record in place — the next [`Translucent::prepare`]
    /// picks it up when it re-sorts and re-uploads, so this writes no GPU
    /// buffer itself. `false` when `referent` was never a translucent
    /// instance, the caller's cue to fall back to a full reload.
    pub(super) fn patch(&mut self, part: &Part) -> bool {
        let Some(&index) = self.part_index.get(&part.referent) else {
            return false;
        };
        let instance = InstanceRaw::from_part(part);
        self.items[index] = Item {
            referent: part.referent,
            kind: part.kind,
            center: instance.center(),
            radius: of_part(part).radius(),
            instance,
        };
        true
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
mod tests {
    use super::*;

    fn plastic() -> crate::scene::Slot {
        crate::scene::Slot {
            layer: 0,
            kind: crate::scene::Kind::Plastic,
            studs_per_tile: 10.0,
        }
    }

    fn item(kind: ShapeKind, center: Vec3) -> Item {
        let mut model = [[0.0; 4]; 4];
        model[3] = [center.x, center.y, center.z, 1.0];
        Item {
            referent: Ref::new(0),
            kind,
            center,
            radius: 1.0,
            instance: InstanceRaw::new(model, [1.0; 3], 0.5, 0.0, plastic()),
        }
    }

    fn order_of(items: &[Item], eye: Vec3) -> Vec<usize> {
        let mut order = Vec::new();
        back_to_front(items, eye, |_| true, &mut order);
        order
    }

    #[test]
    fn instances_are_ordered_back_to_front() {
        let items = [
            item(ShapeKind::Box, Vec3::new(0.0, 0.0, 10.0)),
            item(ShapeKind::Box, Vec3::new(0.0, 0.0, 100.0)),
            item(ShapeKind::Box, Vec3::new(0.0, 0.0, 50.0)),
        ];

        assert_eq!(order_of(&items, Vec3::ZERO), vec![1, 2, 0]);
        // Fly past all three and the order reverses.
        assert_eq!(order_of(&items, Vec3::new(0.0, 0.0, 200.0)), vec![0, 2, 1]);
    }

    #[test]
    fn instances_at_the_same_distance_keep_the_order_the_dom_gave_them() {
        let items = [
            item(ShapeKind::Box, Vec3::X),
            item(ShapeKind::Box, -Vec3::X),
            item(ShapeKind::Box, Vec3::Z),
        ];

        assert_eq!(order_of(&items, Vec3::ZERO), vec![0, 1, 2]);
    }

    // The sort wins over batching: a run breaks wherever the shape changes,
    // however many draw calls that costs.
    #[test]
    fn runs_cover_the_sorted_order_without_reordering_it() {
        let items = [
            item(ShapeKind::Box, Vec3::new(0.0, 0.0, 30.0)),
            item(ShapeKind::Ball, Vec3::new(0.0, 0.0, 20.0)),
            item(ShapeKind::Box, Vec3::new(0.0, 0.0, 10.0)),
            item(ShapeKind::Box, Vec3::new(0.0, 0.0, 40.0)),
        ];

        let order = order_of(&items, Vec3::ZERO);
        let runs = runs(&items, &order);

        assert_eq!(order, vec![3, 0, 1, 2]);
        let spans: Vec<(ShapeKind, u32, u32)> = runs
            .iter()
            .map(|run| (run.kind, run.instances.start, run.instances.end))
            .collect();
        assert_eq!(
            spans,
            vec![
                (ShapeKind::Box, 0, 2),
                (ShapeKind::Ball, 2, 3),
                (ShapeKind::Box, 3, 4),
            ]
        );
    }

    #[test]
    fn nothing_translucent_means_no_runs_at_all() {
        assert!(runs(&[], &[]).is_empty());
    }

    // The cull test is applied before the sort, not after: an item it rejects
    // must never occupy a slot in `order` at all.
    #[test]
    fn items_the_visibility_test_rejects_never_enter_the_order() {
        let items = [
            item(ShapeKind::Box, Vec3::new(0.0, 0.0, 10.0)),
            item(ShapeKind::Box, Vec3::new(0.0, 0.0, 100.0)),
            item(ShapeKind::Box, Vec3::new(0.0, 0.0, 50.0)),
        ];
        let mut order = Vec::new();

        back_to_front(&items, Vec3::ZERO, |item| item.center.z < 60.0, &mut order);

        assert_eq!(order, vec![2, 0]);
    }
}
