use super::*;
use crate::scene::tests_support::bounds_from;

/// The map size the top quality level asks for, which is what every fit below
/// is measured in.
const RESOLUTION: u32 = 8192;

/// A cube of eight corners standing in for a truncated view frustum.
fn frustum(center: Vec3, half: f32) -> [Vec3; 8] {
    std::array::from_fn(|index| {
        let sign = |bit: usize| if index & (1 << bit) == 0 { -half } else { half };
        center + Vec3::new(sign(0), sign(1), sign(2))
    })
}

fn afternoon_sun() -> Vec3 {
    Vec3::new(0.5, 0.72, -0.48).normalize()
}

#[test]
fn the_ortho_encloses_every_frustum_corner() {
    let bounds = bounds_from(Vec3::splat(-2000.0), Vec3::splat(2000.0));
    let corners = frustum(Vec3::new(120.0, 30.0, -40.0), 220.0);

    let fit = fit(afternoon_sun(), &corners, &bounds, RESOLUTION);

    for corner in corners {
        let clip = fit.view_projection * corner.extend(1.0);
        let ndc = clip.truncate() / clip.w;
        // A texel of snapping slack: the map is aligned to a world grid, which
        // moves its edges by up to half a texel either way.
        let slack = 2.0 / RESOLUTION as f32;
        assert!(
            ndc.x.abs() <= 1.0 + slack && ndc.y.abs() <= 1.0 + slack,
            "corner {corner} lands at {ndc}"
        );
    }
}

// A caster anywhere in the scene has to be inside the depth range, including one
// standing between the sun and the visible region — those are off-screen and
// still cast into it.
#[test]
fn the_depth_range_covers_the_whole_scene() {
    let bounds = bounds_from(
        Vec3::new(-800.0, -20.0, -800.0),
        Vec3::new(800.0, 300.0, 800.0),
    );
    let fit = fit(
        afternoon_sun(),
        &frustum(Vec3::new(0.0, 20.0, 0.0), 250.0),
        &bounds,
        RESOLUTION,
    );

    for corner in bounds.corners() {
        let clip = fit.view_projection * corner.extend(1.0);
        let depth = clip.z / clip.w;
        assert!(
            (0.0..=1.0).contains(&depth),
            "corner {corner} at depth {depth}"
        );
    }
}

/// Where the world origin lands, measured in shadow-map texels from the middle
/// of the map. An origin snapped to the texel grid puts it on a whole number.
fn origin_in_texels(fit: &Fit) -> (f32, f32) {
    let clip = fit.view_projection * glam::Vec4::W;
    let ndc = clip.truncate() / clip.w;
    let half = RESOLUTION as f32 / 2.0;
    (ndc.x * half, ndc.y * half)
}

// The whole point of snapping: the map's texels stay pinned to a world grid as
// the camera moves, so a shadow edge steps a texel at a time instead of
// crawling. A moving grid is exactly what shimmering looks like.
#[test]
fn the_texel_grid_stays_pinned_to_the_world_as_the_camera_steps() {
    let bounds = bounds_from(Vec3::splat(-2000.0), Vec3::splat(2000.0));
    let reference = fit(
        afternoon_sun(),
        &frustum(Vec3::ZERO, 220.0),
        &bounds,
        RESOLUTION,
    );

    for step in 0..24 {
        let offset = Vec3::new(0.3, 0.0, 0.17) * step as f32;
        let moved = fit(
            afternoon_sun(),
            &frustum(offset, 220.0),
            &bounds,
            RESOLUTION,
        );

        // The extent never changes, so neither does the grid the origin is
        // rounded onto.
        assert_eq!(moved.texel_studs, reference.texel_studs);
        let (x, y) = origin_in_texels(&moved);
        assert!(
            (x - x.round()).abs() < 1e-2 && (y - y.round()).abs() < 1e-2,
            "step {step} lands the world origin at ({x}, {y}) texels"
        );
    }
}

// Below one texel of movement nothing may change at all, or a hovering camera
// would flicker between two rounding outcomes every frame.
#[test]
fn a_sub_texel_step_leaves_the_map_exactly_where_it_was() {
    let bounds = bounds_from(Vec3::splat(-2000.0), Vec3::splat(2000.0));
    let still = fit(
        afternoon_sun(),
        &frustum(Vec3::ZERO, 220.0),
        &bounds,
        RESOLUTION,
    );
    let nudged = fit(
        afternoon_sun(),
        &frustum(Vec3::splat(still.texel_studs / 100.0), 220.0),
        &bounds,
        RESOLUTION,
    );

    assert_eq!(still.view_projection, nudged.view_projection);
}

// A scene smaller than the frustum must not waste the map on empty space around
// it: the extent shrinks to the scene, which is what buys a 10-stud place
// pin-sharp shadows.
#[test]
fn a_small_scene_shrinks_the_map_onto_itself() {
    let bounds = bounds_from(Vec3::splat(-30.0), Vec3::splat(30.0));
    let fit = fit(
        afternoon_sun(),
        &frustum(Vec3::ZERO, 400.0),
        &bounds,
        RESOLUTION,
    );

    assert!(
        fit.texel_studs < 2.0 * 60.0 / RESOLUTION as f32 + 1e-6,
        "texel {} is wider than the scene's own span",
        fit.texel_studs
    );
}

// Noon at the equator puts the sun straight overhead, where `Vec3::Y` is no
// longer a usable up vector for the light's view matrix.
#[test]
fn a_vertical_sun_still_produces_a_finite_matrix() {
    let bounds = bounds_from(Vec3::splat(-500.0), Vec3::splat(500.0));

    let fit = fit(Vec3::Y, &frustum(Vec3::ZERO, 200.0), &bounds, RESOLUTION);

    assert!(fit.view_projection.is_finite());
    assert!(fit.texel_studs.is_finite() && fit.texel_studs > 0.0);
    assert!(fit.depth_studs > 0.0);
}

#[test]
fn a_caster_within_the_fitted_footprint_is_visible() {
    let bounds = bounds_from(Vec3::splat(-2000.0), Vec3::splat(2000.0));
    let fit = fit(
        afternoon_sun(),
        &frustum(Vec3::ZERO, 220.0),
        &bounds,
        RESOLUTION,
    );

    assert!(fit.visible(Vec3::ZERO, 1.0));
}

#[test]
fn a_caster_far_outside_the_fitted_footprint_is_culled() {
    let bounds = bounds_from(Vec3::splat(-2000.0), Vec3::splat(2000.0));
    let fit = fit(
        afternoon_sun(),
        &frustum(Vec3::ZERO, 220.0),
        &bounds,
        RESOLUTION,
    );

    assert!(!fit.visible(Vec3::new(1900.0, 0.0, 0.0), 1.0));
}

// The whole point of a separate shadow-pass test: the fitted box is a sphere
// around the camera frustum, not the frustum's own tapered shape, so a point
// well outside the frustum's own corners in x alone can still sit inside it.
#[test]
fn a_caster_outside_the_camera_frustum_can_still_be_shadow_visible() {
    let bounds = bounds_from(Vec3::splat(-2000.0), Vec3::splat(2000.0));
    let fit = fit(
        afternoon_sun(),
        &frustum(Vec3::ZERO, 220.0),
        &bounds,
        RESOLUTION,
    );

    assert!(fit.visible(Vec3::new(300.0, 0.0, 0.0), 1.0));
}

// A sphere sitting exactly on the fitted box's own edge must not be culled by
// float rounding alone.
#[test]
fn a_caster_exactly_on_the_fitted_edge_stays_visible() {
    let bounds = bounds_from(Vec3::splat(-2000.0), Vec3::splat(2000.0));
    let fit = fit(
        afternoon_sun(),
        &frustum(Vec3::ZERO, 220.0),
        &bounds,
        RESOLUTION,
    );

    let edge = fit
        .light_view
        .inverse()
        .transform_point3(Vec3::new(fit.x + fit.half, fit.y, 0.0));

    assert!(fit.visible(edge, 0.0));
}

#[test]
fn a_window_wider_than_its_span_is_centred_on_it() {
    assert_eq!(clamp_span(900.0, -10.0, 10.0, 50.0), 0.0);
    assert_eq!(clamp_span(0.0, 0.0, 100.0, 10.0), 10.0);
    assert_eq!(clamp_span(50.0, 0.0, 100.0, 10.0), 50.0);
    assert_eq!(clamp_span(1000.0, 0.0, 100.0, 10.0), 90.0);
}
