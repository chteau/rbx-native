use super::*;
use rbx_dom::{Instance, Ref, Variant};

/// A `Terrain` carrying one `Clouds` child at most, in order, parented under
/// a `Workspace` — `read` only ever looks there now (see [`super::read`]).
fn terrain_with(children: &[(&str, &[(&str, Variant)])]) -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace_ref = Ref::new(1);
    dom.insert(Instance::new(workspace_ref, "Workspace", "Workspace"));
    dom.set_parent(workspace_ref, None);

    let terrain_ref = Ref::new(2);
    dom.insert(Instance::new(terrain_ref, "Terrain", "Terrain"));
    dom.set_parent(terrain_ref, Some(workspace_ref));

    for (index, (class, properties)) in children.iter().enumerate() {
        let child = Ref::new(index as u32 + 3);
        let mut instance = Instance::new(child, *class, *class);
        for (name, value) in *properties {
            instance
                .properties_mut()
                .insert((*name).to_string(), value.clone());
        }
        dom.insert(instance);
        dom.set_parent(child, Some(terrain_ref));
    }

    dom
}

fn read_clouds(children: &[(&str, &[(&str, Variant)])]) -> Option<Clouds> {
    let dom = terrain_with(children);
    read(&dom, &ReflectionDatabase::embedded())
}

#[test]
fn no_terrain_at_all_draws_no_clouds() {
    let dom = WeakDom::new();
    assert_eq!(read(&dom, &ReflectionDatabase::embedded()), None);
}

#[test]
fn a_terrain_with_no_clouds_child_draws_none() {
    assert_eq!(read_clouds(&[]), None);
}

// The "Dynamic Clouds" guide only promises rendering for one parented under
// `Terrain`; a place that keeps one elsewhere (e.g. directly under
// `Workspace`) gets nothing, which this asserts by never even offering one.
#[test]
fn a_clouds_instance_outside_terrain_is_not_found() {
    let mut dom = WeakDom::new();
    let workspace_ref = Ref::new(1);
    dom.insert(Instance::new(workspace_ref, "Workspace", "Workspace"));
    dom.set_parent(workspace_ref, None);
    let clouds_ref = Ref::new(2);
    dom.insert(Instance::new(clouds_ref, "Clouds", "Clouds"));
    dom.set_parent(clouds_ref, Some(workspace_ref));

    assert_eq!(read(&dom, &ReflectionDatabase::embedded()), None);
}

#[test]
fn a_disabled_clouds_instance_draws_nothing() {
    let clouds = read_clouds(&[("Clouds", &[("Enabled", Variant::Bool(false))])]);
    assert_eq!(clouds, None);
}

#[test]
fn an_enabled_clouds_instance_with_no_properties_gets_studios_own_defaults() {
    let clouds = read_clouds(&[("Clouds", &[])]).expect("enabled by default");

    assert_eq!(clouds.cover, DEFAULT_COVER);
    assert_eq!(clouds.density, DEFAULT_DENSITY);
    assert_eq!(clouds.color, Vec3::ONE);
}

#[test]
fn cover_density_and_color_are_read_property_by_property() {
    let clouds = read_clouds(&[(
        "Clouds",
        &[
            ("Cover", Variant::Float32(0.9)),
            ("Density", Variant::Float32(0.65)),
            (
                "Color",
                Variant::Color3(rbx_dom::Color3Data {
                    r: 1.0,
                    g: 0.0,
                    b: 0.0,
                }),
            ),
        ],
    )])
    .expect("enabled");

    assert_eq!(clouds.cover, 0.9);
    assert_eq!(clouds.density, 0.65);
    // Pure red is one of the two sRGB endpoints that survive linearization
    // untouched, so this checks the property is wired without needing the
    // curve itself.
    assert_eq!(clouds.color, Vec3::new(1.0, 0.0, 0.0));
}

#[test]
fn cover_and_density_are_clamped_to_zero_one() {
    let clouds = read_clouds(&[(
        "Clouds",
        &[
            ("Cover", Variant::Float32(4.0)),
            ("Density", Variant::Float32(-2.0)),
        ],
    )])
    .expect("enabled");

    assert_eq!(clouds.cover, 1.0);
    assert_eq!(clouds.density, 0.0);
}
