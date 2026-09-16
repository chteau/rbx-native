//! Unit tests for [`super`]: the back-to-front sort, run batching and the
//! CPU-side bookkeeping a single-instance edit goes through.

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
        id: crate::scene::PartId::whole(rbx_dom::Ref::new(0)),
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
