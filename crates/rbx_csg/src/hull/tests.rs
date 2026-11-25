use glam::DVec3;

use super::Hull;

#[test]
fn a_cube_with_points_inside_and_on_its_faces_keeps_its_corners() {
    let mut points = Vec::new();
    for x in [-1.0, 0.0, 1.0] {
        for y in [-1.0, 0.0, 1.0] {
            for z in [-1.0, 0.5, 1.0] {
                points.push(DVec3::new(x, y, z));
            }
        }
    }
    let hull = Hull::new(&points).unwrap();
    assert_eq!(hull.points.len(), 8);
    assert_eq!(hull.triangles.len(), 12);
    assert!((hull.volume() - 8.0).abs() < 1e-9);
}

#[test]
fn flat_or_too_few_points_have_no_hull() {
    assert!(Hull::new(&[DVec3::ZERO, DVec3::X, DVec3::Y]).is_none());
    let flat: Vec<DVec3> = (0..10)
        .map(|i| DVec3::new(f64::from(i), f64::from(i * i % 7), 0.0))
        .collect();
    assert!(Hull::new(&flat).is_none());
}

#[test]
fn a_sphere_of_points_is_closed_and_outward() {
    let points: Vec<DVec3> = (0..200)
        .map(|i| {
            let t = f64::from(i) * 2.399_963;
            let y = 1.0 - 2.0 * (f64::from(i) + 0.5) / 200.0;
            let r = (1.0 - y * y).sqrt();
            DVec3::new(r * t.cos(), y, r * t.sin())
        })
        .collect();
    let hull = Hull::new(&points).unwrap();
    assert_eq!(hull.points.len(), 200);
    assert_eq!(hull.triangles.len(), 2 * 200 - 4);
    let volume = hull.volume();
    assert!(volume > 4.0 && volume < 4.0 * std::f64::consts::PI / 3.0);
}
