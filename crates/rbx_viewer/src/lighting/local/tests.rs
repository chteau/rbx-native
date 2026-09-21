use rbx_dom::{CFrameData, Color3Data, Ref, Vector3Data};

use super::*;

pub(super) const EPSILON: f32 = 1e-4;
pub(super) const IDENTITY_ROTATION: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
/// A quarter turn about +Y, which sends the part's local -Z (Front) to -X.
pub(super) const YAW_90_ROTATION: [f32; 9] = [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0];

pub(super) fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

pub(super) fn close(actual: Vec3, expected: Vec3) -> bool {
    (actual - expected).length() < EPSILON
}

pub(super) fn cframe(position: Vec3, rotation: [f32; 9]) -> Variant {
    Variant::CFrame(CFrameData {
        position: Vector3Data {
            x: position.x,
            y: position.y,
            z: position.z,
        },
        rotation,
    })
}

fn vector3(size: Vec3) -> Variant {
    Variant::Vector3(Vector3Data {
        x: size.x,
        y: size.y,
        z: size.z,
    })
}

/// A DOM under construction: parts and the lights hanging off them, each with
/// its own referent, all parented under a synthetic `Workspace` — `local_lights`
/// only ever looks there now (see [`super::local_lights`]).
pub(super) struct Fixture {
    pub(super) dom: WeakDom,
    next: u32,
    pub(super) workspace: Ref,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let mut dom = WeakDom::new();
        let workspace = Ref::new(1);
        dom.insert(Instance::new(workspace, "Workspace", "Workspace"));
        dom.set_parent(workspace, None);
        Fixture {
            dom,
            next: 2,
            workspace,
        }
    }

    pub(super) fn insert(
        &mut self,
        class: &str,
        parent: Option<Ref>,
        properties: &[(&str, Variant)],
    ) -> Ref {
        let referent = Ref::new(self.next);
        self.next += 1;
        let mut instance = Instance::new(referent, class, class);
        for (name, value) in properties {
            instance
                .properties_mut()
                .insert((*name).to_string(), value.clone());
        }
        self.dom.insert(instance);
        self.dom.set_parent(referent, parent);
        referent
    }

    pub(super) fn part(&mut self, position: Vec3, size: Vec3, rotation: [f32; 9]) -> Ref {
        self.insert(
            "Part",
            Some(self.workspace),
            &[
                ("CFrame", cframe(position, rotation)),
                ("size", vector3(size)),
            ],
        )
    }

    fn lights(&self) -> Vec<LocalLight> {
        local_lights(&self.dom, &database(), Vec3::ZERO)
    }
}

/// One light of `class` on an unrotated 2-stud part at the origin.
fn one(class: &str, properties: &[(&str, Variant)]) -> Vec<LocalLight> {
    let mut fixture = Fixture::new();
    let part = fixture.part(Vec3::ZERO, Vec3::splat(2.0), IDENTITY_ROTATION);
    fixture.insert(class, Some(part), properties);
    fixture.lights()
}

#[test]
fn a_point_light_defaults_to_studios_own_property_sheet() {
    let lights = one("PointLight", &[]);

    assert_eq!(lights.len(), 1);
    let light = lights[0];
    assert_eq!(light.range, 8.0);
    assert!(close(light.color, Vec3::splat(RADIANCE_SCALE)));
    assert_eq!(light.position, Vec3::ZERO);
    assert_eq!(light.near, 0.0);
    // No cone at all: every direction has to come out fully lit.
    assert_eq!(light.direction, Vec3::ZERO);
    assert!(light.cos_outer < -1.0 && light.cos_outer < light.cos_inner);
}

/// What a light inserted from the Explorer holds: nothing stored at all, so
/// every value is its class default — the ones the Properties sheet shows.
#[test]
fn a_cone_light_with_nothing_stored_shines_from_its_front_with_studios_defaults() {
    for class in ["SpotLight", "SurfaceLight"] {
        let lights = one(class, &[]);

        assert_eq!(lights.len(), 1, "{class}");
        assert_eq!(lights[0].range, 16.0, "{class}");
        assert!(close(lights[0].direction, -Vec3::Z), "{class} faces Front");
        // A 90 degree cone reaches 45 degrees off its axis.
        assert!((lights[0].cos_outer - 45f32.to_radians().cos()).abs() < EPSILON);
        assert!(close(lights[0].color, Vec3::splat(RADIANCE_SCALE)));
    }
}

#[test]
fn brightness_and_color_become_one_linear_radiance() {
    let lights = one(
        "PointLight",
        &[
            ("Brightness", Variant::Float32(2.0)),
            (
                "Color",
                Variant::Color3(Color3Data {
                    r: 1.0,
                    g: 0.5,
                    b: 0.0,
                }),
            ),
        ],
    );

    let expected = Vec3::new(
        crate::scene::srgb_to_linear(1.0),
        crate::scene::srgb_to_linear(0.5),
        crate::scene::srgb_to_linear(0.0),
    ) * 2.0
        * RADIANCE_SCALE;
    assert!(close(lights[0].color, expected));
}

#[test]
fn a_disabled_light_never_reaches_the_gpu() {
    assert!(one("PointLight", &[("Enabled", Variant::Bool(false))]).is_empty());
    // Absent means enabled: Studio only serializes the property once it is off.
    assert_eq!(one("PointLight", &[]).len(), 1);
}

#[test]
fn range_is_clamped_to_what_studios_slider_allows() {
    assert_eq!(
        one("PointLight", &[("Range", Variant::Float32(600.0))])[0].range,
        MAX_RANGE
    );
    assert_eq!(
        one("PointLight", &[("Range", Variant::Float32(-4.0))])[0].range,
        0.0
    );
    // A negative Brightness would subtract light from the scene.
    assert_eq!(
        one("PointLight", &[("Brightness", Variant::Float32(-1.0))])[0].color,
        Vec3::ZERO
    );
}

#[test]
fn a_light_whose_parent_is_not_drawable_is_skipped() {
    let mut fixture = Fixture::new();
    let folder = fixture.insert("Folder", Some(fixture.workspace), &[]);
    fixture.insert("PointLight", Some(folder), &[]);
    assert!(fixture.lights().is_empty());

    // Terrain is a BasePart this renderer never draws (see `scene`).
    let mut fixture = Fixture::new();
    let terrain = fixture.insert(
        "Terrain",
        Some(fixture.workspace),
        &[
            ("CFrame", cframe(Vec3::ZERO, IDENTITY_ROTATION)),
            ("size", vector3(Vec3::splat(4.0))),
        ],
    );
    fixture.insert("PointLight", Some(terrain), &[]);
    assert!(fixture.lights().is_empty());
}

// The class default: a fresh light casts no shadow until asked to.
#[test]
fn shadows_default_to_off_and_read_back_when_turned_on() {
    assert!(!one("SpotLight", &[])[0].shadows);
    assert!(!one("PointLight", &[])[0].shadows);
    assert!(one("SpotLight", &[("Shadows", Variant::Bool(true))])[0].shadows);
}

#[test]
fn a_cone_light_with_an_unreadable_face_is_skipped() {
    assert!(one("SurfaceLight", &[("Face", Variant::Enum(9))]).is_empty());
}

// The whole point of reading `Face` through the part's CFrame: the cone has to
// follow the part when a builder rotates it.
#[test]
fn a_spot_aims_along_its_face_rotated_by_the_parts_cframe() {
    let unrotated = one("SpotLight", &[("Face", Variant::Enum(4))]);
    assert!(close(unrotated[0].direction, -Vec3::Y));

    let mut fixture = Fixture::new();
    let part = fixture.part(Vec3::new(10.0, 0.0, 0.0), Vec3::splat(2.0), YAW_90_ROTATION);
    // Front is the part's local -Z, which a quarter turn about +Y sends to -X.
    fixture.insert("SpotLight", Some(part), &[("Face", Variant::Enum(5))]);

    let lights = fixture.lights();
    assert!(close(lights[0].direction, -Vec3::X));
    assert!(close(lights[0].position, Vec3::new(10.0, 0.0, 0.0)));
}

#[test]
fn a_surface_light_sits_on_its_face_with_a_near_field_across_it() {
    let mut fixture = Fixture::new();
    // A ceiling panel: 6 by 8 studs across, 2 thick, emitting downward.
    let part = fixture.part(
        Vec3::new(0.0, 20.0, 0.0),
        Vec3::new(6.0, 2.0, 8.0),
        IDENTITY_ROTATION,
    );
    fixture.insert(
        "SurfaceLight",
        Some(part),
        &[
            ("Face", Variant::Enum(4)),
            ("Range", Variant::Float32(40.0)),
        ],
    );

    let light = fixture.lights()[0];

    // On the bottom face, a stud below the part's centre, pointing down.
    assert!(close(light.position, Vec3::new(0.0, 19.0, 0.0)));
    assert!(close(light.direction, -Vec3::Y));
    // Half the shorter side of the 6 by 8 face.
    assert_eq!(light.near, 3.0);
}

// A face wider than its own reach would otherwise push the falloff past the
// Range and leave a hard-edged sphere of light.
#[test]
fn a_huge_face_keeps_its_falloff_inside_its_range() {
    let mut fixture = Fixture::new();
    let part = fixture.part(Vec3::ZERO, Vec3::new(128.0, 1.0, 78.0), IDENTITY_ROTATION);
    fixture.insert(
        "SurfaceLight",
        Some(part),
        &[
            ("Face", Variant::Enum(4)),
            ("Range", Variant::Float32(18.0)),
        ],
    );

    assert_eq!(fixture.lights()[0].near, 9.0);
}

#[test]
fn a_light_on_an_attachment_is_placed_in_the_parts_own_frame() {
    let mut fixture = Fixture::new();
    let part = fixture.part(Vec3::new(0.0, 10.0, 0.0), Vec3::splat(2.0), YAW_90_ROTATION);
    let attachment = fixture.insert(
        "Attachment",
        Some(part),
        &[(
            "CFrame",
            cframe(Vec3::new(0.0, 0.0, 4.0), IDENTITY_ROTATION),
        )],
    );
    fixture.insert("PointLight", Some(attachment), &[]);

    let light = fixture.lights()[0];

    // The quarter turn about +Y sends the attachment's local +Z to +X.
    assert!(close(light.position, Vec3::new(4.0, 10.0, 0.0)));
}

// Every cone is handed to `smoothstep(cos_outer, cos_inner, …)`, which divides
// by their difference: an Angle of 0 must not make that zero.
#[test]
fn the_cone_edges_never_coincide() {
    for angle in [0.0, 1.0, 90.0, 179.0, 180.0, 400.0] {
        let (outer, inner) = cone(Some(&Variant::Float32(angle)));
        // `- EPSILON`: an Angle of 0 lands the gap on MIN_CONE_GAP itself, and
        // the subtraction that produced it rounds a hair below.
        assert!(
            inner - outer >= MIN_CONE_GAP - f32::EPSILON,
            "angle {angle}"
        );
        assert!(inner <= 1.0, "angle {angle}");
    }
}

#[test]
fn past_the_cap_only_the_lights_nearest_the_centre_survive() {
    let mut fixture = Fixture::new();
    for index in 0..(MAX_LOCAL_LIGHTS + 4) {
        let part = fixture.part(
            Vec3::new(index as f32, 0.0, 0.0),
            Vec3::splat(1.0),
            IDENTITY_ROTATION,
        );
        fixture.insert("PointLight", Some(part), &[]);
    }

    let lights = local_lights(&fixture.dom, &database(), Vec3::ZERO);

    assert_eq!(lights.len(), MAX_LOCAL_LIGHTS);
    let farthest = lights
        .iter()
        .map(|light| light.position.x)
        .fold(0.0f32, f32::max);
    assert_eq!(farthest, (MAX_LOCAL_LIGHTS - 1) as f32);
}
