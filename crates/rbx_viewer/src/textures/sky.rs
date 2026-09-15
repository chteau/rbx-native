//! The six panels of a `Sky`, the orientation each one is pasted with, and the
//! sun and moon discs that hang in front of them.

pub(super) mod default;
#[cfg(test)]
mod seams;
pub(super) mod stars;

use glam::Vec3;
use rbx_assets::AssetRef;
use rbx_dom::{Ref, Variant, WeakDom};

use super::{asset_uri, Quad};

// Studio's defaults for a `Sky` that leaves them out.
const DEFAULT_SUN_TEXTURE: &str = "rbxasset://sky/sun.jpg";
const DEFAULT_MOON_TEXTURE: &str = "rbxasset://sky/moon.jpg";
const DEFAULT_SUN_ANGULAR_SIZE: f32 = 21.0;
const DEFAULT_MOON_ANGULAR_SIZE: f32 = 11.0;

/// The sun or the moon: a disc of `angular_size` degrees pasted on the sky, at
/// the sun's own direction or directly opposite it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Body {
    pub(crate) angular_size: f32,
    pub(crate) toward_sun: bool,
}

/// One side of the skybox, named after the `Sky` property that carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkyFace {
    Rt,
    Lf,
    Up,
    Dn,
    Bk,
    Ft,
}

pub(crate) const SKY_FACES: [SkyFace; 6] = [
    SkyFace::Rt,
    SkyFace::Lf,
    SkyFace::Up,
    SkyFace::Dn,
    SkyFace::Bk,
    SkyFace::Ft,
];

impl SkyFace {
    pub(crate) fn property(self) -> &'static str {
        match self {
            SkyFace::Rt => "SkyboxRt",
            SkyFace::Lf => "SkyboxLf",
            SkyFace::Up => "SkyboxUp",
            SkyFace::Dn => "SkyboxDn",
            SkyFace::Bk => "SkyboxBk",
            SkyFace::Ft => "SkyboxFt",
        }
    }

    /// Outward direction and image axes (right, then down) of each sky panel.
    ///
    /// Measured by comparing panel borders: each cube edge has exactly one pairing
    /// that agrees pixel-for-pixel, pinning the assembly uniquely. The results are
    /// listed in [`seams::ADJACENT_BORDERS`]. Mirror ambiguity is resolved by
    /// keeping Roblox's own axis directions. Uses `u × v = -normal` (outside
    /// perspective), which is the standard panorama-to-cubemap layout.
    fn basis(self) -> (Vec3, Vec3, Vec3) {
        match self {
            SkyFace::Rt => (Vec3::X, -Vec3::Z, -Vec3::Y),
            SkyFace::Lf => (-Vec3::X, Vec3::Z, -Vec3::Y),
            SkyFace::Up => (Vec3::Y, -Vec3::Z, Vec3::X),
            SkyFace::Dn => (-Vec3::Y, -Vec3::Z, -Vec3::X),
            SkyFace::Bk => (Vec3::Z, Vec3::X, -Vec3::Y),
            SkyFace::Ft => (-Vec3::Z, -Vec3::X, -Vec3::Y),
        }
    }
}

/// The sun and the moon a `Sky` asks for, unless `CelestialBodiesShown` is off.
///
/// Both textures have a Studio default, so a `Sky` that never set them still
/// gets a sun — which is why these are read as "missing means the default"
/// rather than "missing means nothing".
///
pub(super) fn celestial_bodies(dom: &WeakDom, referent: Ref) -> Vec<(AssetRef, Body)> {
    let Some(properties) = dom.get(referent).map(|instance| instance.properties()) else {
        return Vec::new();
    };
    if properties.get("CelestialBodiesShown") == Some(&Variant::Bool(false)) {
        return Vec::new();
    }

    let texture = |name: &str, default: &str| {
        let uri = properties.get(name).and_then(asset_uri).unwrap_or(default);
        AssetRef::parse(uri)
            .ok()
            .filter(|reference| *reference != AssetRef::Empty)
    };
    let size = |name: &str, default: f32| match properties.get(name) {
        Some(&Variant::Float32(degrees)) if degrees > 0.0 => degrees,
        _ => default,
    };

    [
        (
            texture("SunTextureId", DEFAULT_SUN_TEXTURE),
            size("SunAngularSize", DEFAULT_SUN_ANGULAR_SIZE),
            true,
        ),
        (
            texture("MoonTextureId", DEFAULT_MOON_TEXTURE),
            size("MoonAngularSize", DEFAULT_MOON_ANGULAR_SIZE),
            false,
        ),
    ]
    .into_iter()
    .filter_map(|(reference, angular_size, toward_sun)| {
        Some((
            reference?,
            Body {
                angular_size,
                toward_sun,
            },
        ))
    })
    .collect()
}

/// How many stars this `Sky` asks for.
///
/// Tied to `CelestialBodiesShown` like the sun and the moon are: Roblox hides
/// the whole field with them, not just the two discs.
pub(super) fn star_count(dom: &WeakDom, referent: Ref) -> u32 {
    let Some(properties) = dom.get(referent).map(|instance| instance.properties()) else {
        return 0;
    };
    if properties.get("CelestialBodiesShown") == Some(&Variant::Bool(false)) {
        return 0;
    }

    match properties.get("StarCount") {
        Some(&Variant::Int32(count)) => count.clamp(0, stars::MAX_COUNT as i32) as u32,
        _ => stars::DEFAULT_COUNT,
    }
}

/// Reads the six skybox panels off a `Sky`, or `None` if any of them is empty
/// or unparseable — a partial sky is worse than the plain black background.
pub(super) fn panels(dom: &WeakDom, referent: Ref) -> Option<Vec<(AssetRef, Quad)>> {
    let properties = dom.get(referent)?.properties();

    SKY_FACES
        .into_iter()
        .map(|face: SkyFace| {
            let reference = AssetRef::parse(asset_uri(properties.get(face.property())?)?).ok()?;
            (reference != AssetRef::Empty).then(|| (reference, quad(face)))
        })
        .collect()
}

/// The panel as a unit-cube quad: its corner positions double as the view
/// directions the skybox shader projects onto the far plane, so the cube's size
/// never enters the picture.
///
/// The corners come out in image order (top-left, top-right, bottom-right,
/// bottom-left). Since the basis is the exterior-facing one, that order winds
/// both triangles *away* from the centre of the cube — which is why the sky
/// pipeline draws with culling disabled rather than relying on the winding.
pub(crate) fn quad(face: SkyFace) -> Quad {
    let (normal, u, v) = face.basis();
    let corner = |su: f32, sv: f32| normal + u * su + v * sv;

    Quad {
        positions: [
            corner(-1.0, -1.0).to_array(),
            corner(1.0, -1.0).to_array(),
            corner(1.0, 1.0).to_array(),
            corner(-1.0, 1.0).to_array(),
        ],
        uvs: [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        normal: normal.to_array(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom::Instance;

    fn sky_with(properties: &[(&str, Variant)]) -> (WeakDom, Ref) {
        let mut dom = WeakDom::new();
        let referent = Ref::new(1);
        let mut sky = Instance::new(referent, "Sky", "Sky");
        for (name, value) in properties {
            sky.properties_mut()
                .insert((*name).to_string(), value.clone());
        }
        dom.insert(sky);
        dom.set_parent(referent, None);
        (dom, referent)
    }

    // A `Sky` that never touched its sun still gets Studio's, which is why the
    // textures are read as "missing means the default" and not "means nothing".
    #[test]
    fn a_bare_sky_still_asks_for_studios_own_sun_and_moon() {
        let (dom, referent) = sky_with(&[]);

        let bodies = celestial_bodies(&dom, referent);

        assert_eq!(
            bodies
                .iter()
                .map(|(reference, _)| reference.clone())
                .collect::<Vec<_>>(),
            vec![
                AssetRef::Native("sky/sun.jpg".to_string()),
                AssetRef::Native("sky/moon.jpg".to_string()),
            ]
        );
        assert_eq!(bodies[0].1.angular_size, DEFAULT_SUN_ANGULAR_SIZE);
        assert_eq!(bodies[1].1.angular_size, DEFAULT_MOON_ANGULAR_SIZE);
        assert!(bodies[0].1.toward_sun);
        assert!(!bodies[1].1.toward_sun);
    }

    #[test]
    fn celestial_bodies_shown_off_draws_neither() {
        let (dom, referent) = sky_with(&[("CelestialBodiesShown", Variant::Bool(false))]);

        assert!(celestial_bodies(&dom, referent).is_empty());
    }

    #[test]
    fn an_angular_size_is_taken_from_the_sky_but_never_zero() {
        let (dom, referent) = sky_with(&[
            ("SunAngularSize", Variant::Float32(1.0)),
            ("MoonAngularSize", Variant::Float32(0.0)),
        ]);

        let bodies = celestial_bodies(&dom, referent);

        assert_eq!(bodies[0].1.angular_size, 1.0);
        // A zero-degree disc would be a quad with no area at all.
        assert_eq!(bodies[1].1.angular_size, DEFAULT_MOON_ANGULAR_SIZE);
    }

    #[test]
    fn an_emptied_texture_drops_only_its_own_body() {
        let (dom, referent) = sky_with(&[("SunTextureId", Variant::String(String::new()))]);

        let bodies = celestial_bodies(&dom, referent);

        assert_eq!(bodies.len(), 1);
        assert!(!bodies[0].1.toward_sun);
    }

    #[test]
    fn a_bare_sky_asks_for_studios_own_three_thousand_stars() {
        let (dom, referent) = sky_with(&[]);

        assert_eq!(star_count(&dom, referent), stars::DEFAULT_COUNT);
    }

    #[test]
    fn a_star_count_is_read_from_the_sky_and_never_runs_away_with_itself() {
        let (dom, referent) = sky_with(&[("StarCount", Variant::Int32(500))]);
        assert_eq!(star_count(&dom, referent), 500);

        let (dom, referent) = sky_with(&[("StarCount", Variant::Int32(-5))]);
        assert_eq!(star_count(&dom, referent), 0);

        let (dom, referent) = sky_with(&[("StarCount", Variant::Int32(i32::MAX))]);
        assert_eq!(star_count(&dom, referent), stars::MAX_COUNT);
    }

    // The whole sky goes with `CelestialBodiesShown`, stars included.
    #[test]
    fn celestial_bodies_shown_off_hides_the_star_field_too() {
        let (dom, referent) = sky_with(&[("CelestialBodiesShown", Variant::Bool(false))]);

        assert_eq!(star_count(&dom, referent), 0);
    }

    #[test]
    fn each_face_claims_one_axis_and_one_property() {
        let mut normals: Vec<[f32; 3]> = SKY_FACES
            .iter()
            .map(|f| f.basis().0.to_array())
            .collect::<Vec<_>>();
        normals.sort_by(|a, b| a.partial_cmp(b).unwrap());
        normals.dedup();
        assert_eq!(normals.len(), 6);

        let mut properties: Vec<&str> = SKY_FACES.iter().map(|f| f.property()).collect();
        properties.sort_unstable();
        properties.dedup();
        assert_eq!(properties.len(), 6);
    }

    #[test]
    fn roblox_names_map_to_roblox_axes() {
        assert_eq!(SkyFace::Ft.basis().0, -Vec3::Z);
        assert_eq!(SkyFace::Bk.basis().0, Vec3::Z);
        assert_eq!(SkyFace::Lf.basis().0, -Vec3::X);
        assert_eq!(SkyFace::Rt.basis().0, Vec3::X);
        assert_eq!(SkyFace::Up.basis().0, Vec3::Y);
        assert_eq!(SkyFace::Dn.basis().0, -Vec3::Y);
    }

    // The handedness the measured borders impose: a sky is stored the way a cube
    // map is, so right × down points *out* of the cube, not at the viewer.
    #[test]
    fn every_panel_carries_the_exterior_cube_map_handedness() {
        for face in SKY_FACES {
            let (normal, u, v) = face.basis();
            assert_eq!(u.cross(v), -normal, "{face:?}");
            assert_eq!(u.dot(v), 0.0, "{face:?}");
            assert_eq!(u.dot(normal), 0.0, "{face:?}");
            assert_eq!(v.dot(normal), 0.0, "{face:?}");
        }
    }

    // The seam test: a corner of the cube is a single point in three panels at
    // once, so all three must place one of their own corners exactly there.
    #[test]
    fn the_panels_meet_at_every_corner_of_the_cube() {
        let quads: Vec<Quad> = SKY_FACES.into_iter().map(quad).collect();

        for x in [-1.0f32, 1.0] {
            for y in [-1.0f32, 1.0] {
                for z in [-1.0f32, 1.0] {
                    let corner = [x, y, z];
                    let touching = quads
                        .iter()
                        .filter(|quad| quad.positions.contains(&corner))
                        .count();
                    assert_eq!(touching, 3, "corner {corner:?}");
                }
            }
        }
    }

    // Winding: with the exterior-facing basis the triangles face away from the
    // centre, so the sky pipeline cannot cull back faces — see `quad`.
    #[test]
    fn the_wound_triangles_face_away_from_the_centre() {
        for face in SKY_FACES {
            let quad = quad(face);
            let normal = Vec3::from(quad.normal);
            let corner =
                |i: usize| Vec3::from(quad.positions[super::super::QUAD_INDICES[i] as usize]);

            for triangle in 0..2 {
                let (a, b, c) = (
                    corner(triangle * 3),
                    corner(triangle * 3 + 1),
                    corner(triangle * 3 + 2),
                );
                assert!((b - a).cross(c - b).dot(normal) > 0.0, "{face:?}");
            }
        }
    }
}
