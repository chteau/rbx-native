use glam::Mat4;

use super::*;

fn close(a: Vec3, b: Vec3) -> bool {
    (a - b).length() < 1e-3
}

fn frame(
    solid: Solid,
    model: Mat4,
    kind: TargetKind,
    corner: Vec3,
    y: Vec3,
    z: Vec3,
) -> SurfaceFrame {
    SurfaceFrame {
        corner,
        x: y.cross(z),
        y,
        z,
        size: Vec2::new(0.0, model.x_axis.truncate().length()),
        kind,
        part: Some((solid, model)),
    }
}

fn ends(lines: &[Line]) -> impl Iterator<Item = Vec3> + '_ {
    lines.iter().flat_map(|line| [line.from, line.to])
}

#[test]
fn studios_circle_has_twenty_four_points_densest_at_the_diagonals() {
    let points = circle();
    assert_eq!(points.len(), 28);
    let flat: Vec<Vec3> = points.iter().map(|v| v.extend(0.0)).collect();
    assert_eq!(path(&flat, true).len(), 24);
    assert!(points.iter().all(|v| (v.length() - 1.0).abs() < 1e-6));
    // 18.4° from an axis to its neighbour, 11.3° either side of a diagonal.
    let angle = |a: Vec2, b: Vec2| a.angle_to(b).abs().to_degrees();
    assert!((angle(points[3], points[4]) - 18.435).abs() < 0.01);
    assert!((angle(points[0], points[1]) - 11.31).abs() < 0.01);
}

#[test]
fn a_ball_has_three_great_circles_at_its_radius() {
    // 4 × 6 × 5: the radius is half the smallest size.
    let model = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0))
        * Mat4::from_scale(Vec3::new(4.0, 6.0, 5.0));
    let on = frame(
        Solid::Ball,
        model,
        TargetKind::Sphere,
        Vec3::ZERO,
        Vec3::Y,
        Vec3::Z,
    );
    let lines = major_lines(&on);
    assert_eq!(lines.len(), 3 * 24);
    let centre = Vec3::new(1.0, 2.0, 3.0);
    assert!(ends(&lines).all(|end| (end.distance(centre) - 2.0).abs() < 1e-4));
    assert!(lines
        .iter()
        .all(|line| line.color == PASSIVE && (line.under, line.over) == (1.0, 0.0)));
}

#[test]
fn a_cylinder_has_four_side_lines_one_ring_and_a_cross_on_each_end() {
    let model = Mat4::from_scale(Vec3::new(8.0, 2.0, 2.0));
    let on = frame(
        Solid::Cylinder,
        model,
        TargetKind::Cylinder,
        Vec3::ZERO,
        Vec3::Y,
        Vec3::NEG_X,
    );
    let lines = major_lines(&on);
    assert_eq!(lines.len(), 4 + 24 + 4);
    // The one ring stands at mid-length; nothing else is round.
    let ring: Vec<_> = lines
        .iter()
        .filter(|line| line.from.x == 0.0 && line.to.x == 0.0)
        .collect();
    assert_eq!(ring.len(), 24);
    assert!(ends(&lines).all(|end| Vec2::new(end.y, end.z).length() < 1.0 + 1e-4));
}

#[test]
fn a_box_has_no_major_lines() {
    let mut on = frame(
        Solid::Ball,
        Mat4::IDENTITY,
        TargetKind::Polygon,
        Vec3::ZERO,
        Vec3::Y,
        Vec3::Z,
    );
    on.part = None;
    assert!(major_lines(&on).is_empty());
}

#[test]
fn a_disc_lattice_clips_each_line_and_leaves_out_the_excluded_ones() {
    let lattice = Lattice {
        at: Vec3::ZERO,
        a: Vec3::X,
        b: Vec3::Z,
        min: Vec2::splat(-2.0),
        max: Vec2::splat(2.0),
        radius: Some(2.0),
        exclude: (0, 0),
    };
    let lines = lattice.lines();
    // ±1 each way, clipped to ±√3; ±2 each way touch the disc at a point.
    assert_eq!(lines.len(), 4);
    for line in &lines {
        assert!(((line.to - line.from).length() - 2.0 * 3f32.sqrt()).abs() < 1e-4);
        assert_eq!((line.under, line.over), (0.6, 0.15));
    }
}

#[test]
fn a_cylinder_side_drag_draws_a_ladder_without_the_landing_rung_and_a_ring_there() {
    // An 8 × 2 × 2 cylinder, its side's frame on the top line at the +X
    // end; the cursor 2.8 in from that end.
    let model = Mat4::from_scale(Vec3::new(8.0, 2.0, 2.0));
    let on = frame(
        Solid::Cylinder,
        model,
        TargetKind::Cylinder,
        Vec3::new(4.0, 1.0, 0.0),
        Vec3::Y,
        Vec3::NEG_X,
    );
    let hit = Vec3::new(1.2, 1.0, 0.1);
    let guides = landed(&on, hit, 1.0, false, |_| 1.0).unwrap();
    let rungs: Vec<_> = guides
        .lines
        .iter()
        .filter(|line| line.over == 0.15 && (line.from.x - line.to.x).abs() < 1e-5)
        .map(|line| line.from.x)
        .collect();
    // Rungs every stud from the end to the middle, bar the one at 3 in.
    assert_eq!(rungs.len(), 4);
    assert!(rungs.iter().all(|x| (x - 1.0).abs() > 1e-4));
    // The yellow ring round the cylinder at x = 1.
    let ring: Vec<_> = guides
        .lines
        .iter()
        .filter(|line| {
            line.color == ACTIVE
                && (line.from.x - 1.0).abs() < 1e-4
                && (line.to.x - 1.0).abs() < 1e-4
        })
        .collect();
    assert_eq!(ring.len(), 24);
    // And the yellow line from the end to the middle.
    assert!(guides.lines.iter().any(|line| line.color == ACTIVE
        && close(line.from, Vec3::new(4.0, 1.0, 0.0))
        && close(line.to, Vec3::new(0.0, 1.0, 0.0))));
}

#[test]
fn a_ball_drag_draws_the_latitude_and_meridian_through_the_point() {
    // An 8-stud ball, the point one stud up it.
    let model = Mat4::from_scale(Vec3::splat(8.0));
    let at = Vec3::new(0.0, 1.0, 15f32.sqrt());
    let on = frame(
        Solid::Ball,
        model,
        TargetKind::Sphere,
        at,
        at.normalize(),
        Vec3::X,
    );
    let guides = landed(&on, at, 1.0, false, |_| 1.0).unwrap();
    let yellow: Vec<_> = guides
        .lines
        .iter()
        .filter(|line| line.color == ACTIVE)
        .collect();
    // The latitude ring at y = 1 and the meridian from the equator to the
    // north pole, both over everything.
    assert!(yellow
        .iter()
        .all(|line| line.over == 1.0 && line.under == 0.0));
    assert!(yellow
        .iter()
        .any(|line| close(line.from, Vec3::new(0.0, 0.0, 4.0))));
    assert!(yellow
        .iter()
        .any(|line| close(line.to, Vec3::new(0.0, 4.0, 0.0))));
    let at_latitude =
        |line: &&&Line| (line.from.y - 1.0).abs() < 1e-4 && (line.to.y - 1.0).abs() < 1e-4;
    assert_eq!(yellow.iter().filter(at_latitude).count(), 24);
    // A tick of two chords at every other grid latitude up to the pole —
    // not at y = 1, the latitude itself, nor at the pole, where it has no
    // length.
    let mut ticks: Vec<f32> = guides
        .lines
        .iter()
        .filter(|line| {
            line.color == PASSIVE && line.from.y > 1e-3 && (line.from.y - line.to.y).abs() < 1e-5
        })
        .map(|line| line.from.y)
        .collect();
    ticks.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
    assert_eq!(ticks, vec![2.0, 3.0]);
}

#[test]
fn a_pole_drag_draws_the_lattice_round_the_pole_and_its_centre() {
    let model = Mat4::from_scale(Vec3::splat(4.0));
    let pole = Vec3::new(0.0, 2.0, 0.0);
    let on = frame(
        Solid::Ball,
        model,
        TargetKind::Polygon,
        pole,
        Vec3::Y,
        Vec3::X,
    );
    let guides = landed(&on, Vec3::new(1.2, 2.0, 0.3), 1.0, false, |_| 1.0).unwrap();
    assert_eq!(guides.dots.len(), 1);
    assert!(close(guides.dots[0].centre, pole));
    // r = 2: the lattice reaches √(4 − 1) = 1.73 round the pole.
    let yellow: Vec<_> = guides
        .lines
        .iter()
        .filter(|line| line.color == ACTIVE)
        .collect();
    assert_eq!(yellow.len(), 2);
    for line in yellow {
        assert!(((line.to - line.from).length() - 2.0 * 3f32.sqrt()).abs() < 1e-3);
    }
    // With the grid off, only the ball's great circles.
    let bare = landed(&on, Vec3::new(1.2, 2.0, 0.3), 0.0, false, |_| 1.0).unwrap();
    assert_eq!(bare.lines.len(), 72);
    assert!(bare.dots.is_empty());
}

#[test]
fn a_part_that_is_not_round_has_no_guides_of_its_own() {
    let mut on = frame(
        Solid::Box,
        Mat4::IDENTITY,
        TargetKind::Polygon,
        Vec3::ZERO,
        Vec3::Y,
        Vec3::Z,
    );
    on.part = None;
    assert!(landed(&on, Vec3::ZERO, 1.0, false, |_| 1.0).is_none());
}
