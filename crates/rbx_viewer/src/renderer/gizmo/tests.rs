use glam::Vec3;

use super::*;
use crate::gizmo::basis;

fn handles() -> Handles {
    Handles::new(Vec3::new(4.0, 1.0, -2.0), basis(None), 3.0)
}

/// Twice the enclosed volume, by the divergence theorem: positive only if
/// every triangle in the mesh is wound the same way round and facing outwards,
/// which is what the arrows rely on to keep back-face culling from eating
/// their front halves (or, worse, showing both).
fn signed_volume(vertices: &[Vertex]) -> f32 {
    vertices
        .chunks_exact(3)
        .map(|triangle| {
            let [a, b, c] = [0, 1, 2].map(|corner| Vec3::from(triangle[corner].position));
            a.dot(b.cross(c))
        })
        .sum()
}

#[test]
fn a_gizmo_fills_exactly_the_buffer_it_reserved() {
    let vertices = arms(&handles(), Vec3::new(40.0, 40.0, 40.0));
    assert_eq!(vertices.len(), CAPACITY);
    assert_eq!(vertices.len() % 3, 0, "the mesh is a triangle list");
}

#[test]
fn nothing_is_drawn_further_out_than_an_arm() {
    let handles = handles();
    let vertices = arms(&handles, Vec3::new(40.0, 40.0, 40.0));

    for vertex in &vertices {
        let reach = (Vec3::from(vertex.position) - handles.origin()).length();
        assert!(reach <= handles.arm() + 1e-4, "a vertex {reach} studs out");
    }
    // And something does reach the full arm: the arrowhead's point.
    let tip = vertices
        .iter()
        .map(|vertex| (Vec3::from(vertex.position) - handles.origin()).length())
        .fold(0.0f32, f32::max);
    assert!((tip - handles.arm()).abs() < 1e-4);
}

#[test]
fn every_arrow_is_wound_outwards() {
    let mut arrow = Vec::new();
    super::arrow(&mut arrow, Vec3::ZERO, Vec3::X, 2.0, [1.0, 0.0, 0.0]);
    assert!(
        signed_volume(&arrow) > 0.0,
        "an inside-out arrow: back-face culling would hollow it out"
    );

    // The same the other way along the axis, where the frame the ring is
    // built in flips with it.
    let mut backwards = Vec::new();
    super::arrow(
        &mut backwards,
        Vec3::ZERO,
        Vec3::NEG_X,
        2.0,
        [1.0, 0.0, 0.0],
    );
    assert!(signed_volume(&backwards) > 0.0);
}

#[test]
fn a_whole_gizmo_is_wound_outwards() {
    assert!(signed_volume(&arms(&handles(), Vec3::splat(40.0))) > 0.0);
}

#[test]
fn the_arms_are_painted_back_to_front() {
    // Looking down the X axis from +X: the arm pointing away from the eye is
    // painted first and the one reaching towards it last, so that with the
    // depth test off the near arm still ends up on top.
    let handles = Handles::new(Vec3::ZERO, basis(None), 1.0);
    let vertices = arms(&handles, Vec3::new(100.0, 0.0, 0.0));

    let mut chunks = vertices.chunks_exact(VERTICES_PER_ARM);
    let first = chunks.next().expect("six arms");
    let last = chunks.next_back().expect("six arms");

    let reach = |arm: &[Vertex], pick: fn(f32, f32) -> f32, seed: f32| {
        arm.iter().map(|vertex| vertex.position[0]).fold(seed, pick)
    };
    assert!(
        reach(first, f32::min, 0.0) < -0.9,
        "the far arm is painted first"
    );
    assert!(
        reach(last, f32::max, 0.0) > 0.9,
        "the near arm is painted last"
    );
}

#[test]
fn each_arm_carries_its_axis_colour() {
    let vertices = arms(&handles(), Vec3::splat(40.0));
    let colors: std::collections::HashSet<[u32; 3]> = vertices
        .iter()
        .map(|vertex| vertex.color.map(f32::to_bits))
        .collect();

    assert_eq!(colors.len(), 3, "three axes, three colours");
    for axis in Axis::ALL {
        assert!(colors.contains(&axis.color().map(f32::to_bits)));
    }
}

#[test]
fn a_local_gizmo_points_along_the_parts_own_axes() {
    let rotation = glam::Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let handles = Handles::new(Vec3::ZERO, basis(Some(rotation)), 1.0);
    let vertices = arms(&handles, Vec3::splat(40.0));

    // The red (X) arrow's tip now stands on world -Z, not world +X.
    let red = Axis::X.color().map(f32::to_bits);
    let reach = vertices
        .iter()
        .filter(|vertex| vertex.color.map(f32::to_bits) == red)
        .map(|vertex| Vec3::from(vertex.position))
        .fold(Vec3::ZERO, |furthest, point| {
            if point.length() > furthest.length() {
                point
            } else {
                furthest
            }
        });
    assert!(reach.x.abs() < 1e-4);
    assert!(reach.z.abs() > 0.9);
}
