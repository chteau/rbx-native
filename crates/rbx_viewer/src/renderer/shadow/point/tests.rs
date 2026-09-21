use super::*;

fn light(position: Vec3, direction: Vec3, shadows: bool) -> LocalLight {
    LocalLight {
        position,
        range: 20.0,
        color: Vec3::ONE,
        near: 0.0,
        direction,
        cos_outer: -1.0,
        cos_inner: -1.0,
        shadows,
    }
}

/// A `PointLight` is the light with no axis; everything else belongs to
/// `shadow::local`, and the two must never both claim one.
#[test]
fn only_axis_less_shadow_casting_lights_are_picked() {
    let lights = [
        light(Vec3::ZERO, Vec3::ZERO, true),
        light(Vec3::X, Vec3::Y, true),
        light(Vec3::Y, Vec3::ZERO, false),
    ];
    let selected = select(&lights, Vec3::ZERO, 8);
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].index, 0);
}

#[test]
fn the_nearest_lights_to_the_camera_get_the_cubes() {
    let lights = [
        light(Vec3::new(100.0, 0.0, 0.0), Vec3::ZERO, true),
        light(Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, true),
        light(Vec3::new(10.0, 0.0, 0.0), Vec3::ZERO, true),
    ];
    let selected = select(&lights, Vec3::ZERO, 2);
    assert_eq!(
        selected.iter().map(|one| one.index).collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn no_cubes_at_all_selects_nothing() {
    let lights = [light(Vec3::ZERO, Vec3::ZERO, true)];
    assert!(select(&lights, Vec3::ZERO, 0).is_empty());
}

/// Every direction out of the light has to land inside exactly the face the
/// shader will pick for it — the major axis of the direction — or a
/// fragment would be tested against a map that never saw it.
#[test]
fn every_direction_projects_inside_the_face_the_major_axis_names() {
    let light = light(Vec3::new(3.0, 4.0, 5.0), Vec3::ZERO, true);
    let faces = faces_of(&light);

    // A spread of directions, including the face centres, the diagonals
    // between two faces and the corners where three meet.
    let mut directions = Vec::new();
    for x in [-1.0f32, 0.0, 1.0] {
        for y in [-1.0f32, 0.0, 1.0] {
            for z in [-1.0f32, 0.0, 1.0] {
                let direction = Vec3::new(x, y, z);
                if direction != Vec3::ZERO {
                    directions.push(direction.normalize());
                }
            }
        }
    }

    for direction in directions {
        let point = light.position + direction * 5.0;
        let face = major_axis_face(direction);
        let clip = faces[face] * point.extend(1.0);
        assert!(clip.w > 0.0, "{direction} is in front of face {face}");
        let ndc = clip.truncate() / clip.w;
        assert!(
            ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0,
            "{direction} lands at {ndc} outside face {face}"
        );
        assert!((0.0..=1.0).contains(&ndc.z), "{direction} depth {}", ndc.z);
    }
}

/// The face the shader picks, mirrored here so the test above checks the
/// same choice `lights.wgsl` makes.
fn major_axis_face(direction: Vec3) -> usize {
    let absolute = direction.abs();
    if absolute.x >= absolute.y && absolute.x >= absolute.z {
        usize::from(direction.x < 0.0)
    } else if absolute.y >= absolute.z {
        2 + usize::from(direction.y < 0.0)
    } else {
        4 + usize::from(direction.z < 0.0)
    }
}

/// A point beyond the light's own `Range` is outside every face's depth
/// range, which is what keeps a distant caster from shadowing anything.
#[test]
fn a_caster_past_the_range_falls_outside_the_map() {
    let light = light(Vec3::ZERO, Vec3::ZERO, true);
    let faces = faces_of(&light);
    let clip = faces[0] * Vec3::new(light.range + 5.0, 0.0, 0.0).extend(1.0);
    assert!(clip.truncate().z / clip.w > 1.0);
}
