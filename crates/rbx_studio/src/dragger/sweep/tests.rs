use std::time::Instant;

use glam::{Mat4, Vec3};

use super::*;

fn pose() -> Pose {
    Pose {
        position: Vec3::new(0.0, 10.0, 30.0),
        yaw: 0.0,
        pitch: -0.3,
        fov_degrees: 70.0,
        ortho_scale: 10.0,
    }
}

fn part(centre: Vec3, size: Vec3) -> Mat4 {
    Mat4::from_translation(centre) * Mat4::from_scale(size)
}

/// A 2 × 1 × 2 crate on the origin, dragged along world X.
fn crate_slab() -> Slab {
    Slab::new(
        Vec3::ZERO,
        [Vec3::X, Vec3::Y, Vec3::Z],
        0,
        Vec3::new(2.0, 1.0, 2.0),
    )
}

fn search(candidates: &[Mat4]) -> Vec<SoftSnap> {
    let slab = crate_slab();
    let offsets = offsets(&slab, &[part(Vec3::ZERO, Vec3::new(2.0, 1.0, 2.0))]);
    soft_snaps(
        &slab,
        &offsets,
        candidates,
        32,
        Instant::now(),
        pose(),
        false,
    )
}

fn distances(snaps: &[SoftSnap]) -> Vec<f32> {
    let mut distances: Vec<f32> = snaps.iter().map(|snap| snap.distance).collect();
    distances.sort_by(f32::total_cmp);
    distances
}

#[test]
fn the_selections_own_faces_and_pivot_are_the_offsets() {
    let slab = crate_slab();
    let offsets = offsets(&slab, &[part(Vec3::ZERO, Vec3::new(2.0, 1.0, 2.0))]);
    assert_eq!(offsets, vec![-1.0, 1.0, 0.0]);
}

#[test]
fn a_part_ahead_offers_its_near_and_far_faces_to_each_offset() {
    // Spanning x = 6…14, low enough to sit in the slab's height.
    let plate = part(Vec3::new(10.0, -0.25, 0.0), Vec3::new(8.0, 0.5, 8.0));
    let snaps = search(&[plate]);
    assert_eq!(
        distances(&snaps),
        vec![5.0, 6.0, 7.0, 13.0, 14.0, 15.0],
        "leading face, pivot and trailing face onto x = 6 and x = 14"
    );
    // One dot per face, on the axis line at the face's plane.
    let mut points: Vec<f32> = snaps.iter().map(|snap| snap.point.x).collect();
    points.dedup();
    assert_eq!(points, vec![6.0, 14.0]);
    assert!(snaps
        .iter()
        .all(|snap| snap.point.y == 0.0 && snap.point.z == 0.0));
}

#[test]
fn a_part_the_slab_starts_inside_offers_only_its_far_faces() {
    // The baseplate under the crate: the slab reaches a tenth of a stud into
    // it, so no cast ever enters it — only the passes coming back meet its
    // edges.
    let ground = part(Vec3::new(0.0, -1.0, 0.0), Vec3::new(512.0, 1.0, 512.0));
    let snaps = search(&[ground]);
    let faces: Vec<f32> = distances(&snaps);
    assert_eq!(faces, vec![-257.0, -256.0, -255.0, 255.0, 256.0, 257.0]);
}

#[test]
fn a_part_beside_the_cross_section_is_never_a_candidate() {
    let beside = part(Vec3::new(10.0, 0.0, 3.0), Vec3::splat(2.0));
    assert!(search(&[beside]).is_empty());
    // Just touching the slab's margin, it is.
    let grazing = part(Vec3::new(10.0, 0.0, 2.05), Vec3::splat(2.0));
    assert!(!search(&[grazing]).is_empty());
}

#[test]
fn parts_are_met_in_order_along_both_directions() {
    let ahead = part(Vec3::new(5.0, 0.0, 0.0), Vec3::splat(2.0));
    let behind = part(Vec3::new(-8.0, 0.0, 0.0), Vec3::splat(2.0));
    let snaps = search(&[ahead, behind]);
    let mut points: Vec<f32> = snaps.iter().map(|snap| snap.point.x).collect();
    points.sort_by(f32::total_cmp);
    points.dedup();
    assert_eq!(points, vec![-9.0, -7.0, 4.0, 6.0]);
}

#[test]
fn a_direction_stops_after_half_the_maximum_near_faces() {
    let row: Vec<Mat4> = (1..=30)
        .map(|i| part(Vec3::new(i as f32 * 3.3, 0.0, 0.0), Vec3::ONE))
        .collect();
    let snaps = search(&row);
    let mut points: Vec<f32> = snaps.iter().map(|snap| snap.point.x).collect();
    points.dedup();
    // Sixteen near faces, the last at x = 52.3; the pass back starts right
    // there and finds the fifteen far faces behind it.
    assert_eq!(points.len(), 16 + 15);
    let furthest = points.iter().copied().fold(0.0, f32::max);
    assert!((furthest - 52.3).abs() < 1e-3, "{furthest}");
}

#[test]
fn a_glancing_first_contact_is_passed_over() {
    // A long bar turned 10° off the axis, clipped by the slab's side: the
    // slab first meets its long side, 80° off square to the drag.
    let turn = Mat4::from_rotation_y(10f32.to_radians());
    let bar = Mat4::from_translation(Vec3::new(10.0, 0.0, 2.0))
        * turn
        * Mat4::from_scale(Vec3::new(20.0, 1.0, 1.0));
    let snaps = search(&[bar]);
    assert!(
        snaps
            .iter()
            .all(|snap| !(11.0..13.5).contains(&snap.point.x)),
        "{snaps:?}"
    );
}

#[test]
fn a_turned_box_is_entered_at_its_corner() {
    let turned = Mat4::from_translation(Vec3::new(10.0, 0.0, 0.0))
        * Mat4::from_rotation_y(std::f32::consts::FRAC_PI_4)
        * Mat4::from_scale(Vec3::splat(2.0));
    let snaps = search(&[turned]);
    let nearest = snaps
        .iter()
        .map(|snap| snap.point.x)
        .fold(f32::INFINITY, f32::min);
    assert!(
        (nearest - (10.0 - std::f32::consts::SQRT_2)).abs() < 1e-4,
        "{nearest}"
    );
}

fn snap(distance: f32, reach: f32) -> SoftSnap {
    SoftSnap {
        point: Vec3::X * distance,
        distance,
        reach,
    }
}

#[test]
fn with_a_grid_a_snap_wins_only_when_no_further_than_the_grid_is() {
    let snaps = [snap(5.0, 0.3)];
    // The grid would move 4.6 by 0.4 and the snap by 0.4: a tie goes to the
    // snap.
    assert_eq!(choose(&snaps, 4.6, 1.0), Some(0));
    // 4.05 is 0.05 off the grid and 0.2 off the snap.
    assert_eq!(choose(&[snap(4.25, 0.3)], 4.05, 1.0), None);
    // Within half a step of the snap and nearer it than the grid.
    assert_eq!(choose(&[snap(4.7, 0.3)], 4.4, 1.0), Some(0));
    // More than half a step away is out of reach whatever the grid says.
    assert_eq!(choose(&[snap(5.2, 0.3)], 4.6, 0.5), None);
}

#[test]
fn without_a_grid_each_snap_keeps_its_own_screen_constant_reach() {
    let snaps = [snap(5.0, 0.3), snap(9.0, 0.3)];
    assert_eq!(choose(&snaps, 4.8, 0.0), Some(0));
    assert_eq!(choose(&snaps, 4.6, 0.0), None);
    assert_eq!(choose(&snaps, 8.9, 0.0), Some(1));
    assert_eq!(choose(&[], 1.0, 0.0), None);
}

#[test]
fn the_snapped_face_is_yellow_and_larger_and_the_rest_white() {
    let snaps = [snap(4.0, 0.3), snap(5.0, 0.3), snap(6.0, 0.3)];
    let drawn = dots(&snaps, Some(1), pose(), false);
    assert_eq!(drawn.len(), 3);
    let current = drawn.last().unwrap();
    assert_eq!((current.centre, current.color), (Vec3::X * 5.0, ACTIVE));
    assert!(drawn[..2]
        .iter()
        .all(|dot| dot.color == PASSIVE && dot.radius < current.radius));
    assert!(dots(&snaps, None, pose(), false)
        .iter()
        .all(|dot| dot.color == PASSIVE));
}

#[test]
fn the_axis_line_breaks_over_the_dragged_arrow() {
    let [out, back] = axis_line(Vec3::new(1.0, 0.0, 0.0), Vec3::X, 3.0);
    assert_eq!(out.from, Vec3::new(4.0, 0.0, 0.0));
    assert_eq!(back.to, Vec3::new(1.0, 0.0, 0.0));
    assert!(out.to.x > 1000.0 && back.from.x < -1000.0);
    assert_eq!((out.under, out.over, out.color), (1.0, 0.0, PASSIVE));
    let line = extrude_line(Vec3::ZERO, Vec3::Y);
    assert!(line.from.y < -1000.0 && line.to.y > 1000.0);
}

/// Fifty thousand parts in a 224-wide square, the crate sweeping through its
/// middle: how long the whole search takes when there is a lot to reject.
#[test]
#[ignore]
fn soft_snap_budget_on_fifty_thousand_parts() {
    let crowd: Vec<Mat4> = (0..50_000)
        .map(|i| {
            let (x, z) = (
                (i % 224) as f32 * 3.0 - 336.0,
                (i / 224) as f32 * 3.0 - 336.0,
            );
            part(Vec3::new(x, 0.0, z + 0.4), Vec3::new(1.0, 2.0, 1.0))
        })
        .collect();
    let started = Instant::now();
    let snaps = search(&crowd);
    let spent = started.elapsed();
    eprintln!("{} parts, {} snaps in {spent:?}", crowd.len(), snaps.len());
    assert!(spent < BUDGET, "took {spent:?}");
}

/// The search over a real place stays inside Studio's own 10 ms budget:
/// every fiftieth part dragged along each world axis against all the others.
/// `RBX_SOFT_SNAP_FIXTURE=<place>` to run it (`cargo test -p rbx_studio --
/// --ignored soft_snap_budget --nocapture`).
#[test]
#[ignore]
fn soft_snap_budget_on_a_real_place() {
    let path = std::env::var("RBX_SOFT_SNAP_FIXTURE").expect("RBX_SOFT_SNAP_FIXTURE");
    let dom = rbx_viewer::read_place(std::path::Path::new(&path)).expect("the place reads");
    let database = rbx_reflection::ReflectionDatabase::embedded();
    let models: Vec<Mat4> = rbx_viewer::pick::drawable_parts(&dom, &database)
        .filter_map(|part| rbx_viewer::pick::model_of(&dom, part))
        .collect();
    assert!(!models.is_empty());
    let mut worst = std::time::Duration::ZERO;
    for (index, dragged) in models
        .iter()
        .enumerate()
        .step_by((models.len() / 50).max(1))
    {
        let others: Vec<Mat4> = models
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, model)| *model)
            .collect();
        let basis = [Vec3::X, Vec3::Y, Vec3::Z];
        let size = Vec3::from(basis.map(|axis| {
            (0..3)
                .map(|column| dragged.col(column).truncate().dot(axis).abs())
                .sum::<f32>()
        }));
        for axis in 0..3 {
            let slab = Slab::new(dragged.w_axis.truncate(), basis, axis, size);
            let offsets = offsets(&slab, &[*dragged]);
            let started = Instant::now();
            soft_snaps(&slab, &offsets, &others, 32, started, pose(), false);
            worst = worst.max(started.elapsed());
        }
    }
    eprintln!("{} parts, worst search {worst:?}", models.len());
    assert!(worst < BUDGET, "worst search took {worst:?}");
}
