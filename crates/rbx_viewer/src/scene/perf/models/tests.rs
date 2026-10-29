use super::super::Report;
use super::*;
use rbx_assets::AssetRef;
use rbx_dom::{CFrameData, Vector3Data};

fn nothing(_: &AssetRef) -> Option<Vec<u8>> {
    None
}

fn world() -> (WeakDom, Ref) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    dom.take_changes();
    (dom, workspace)
}

fn add(dom: &mut WeakDom, parent: Ref, class: &str, props: Vec<(&str, Variant)>) -> Ref {
    let referent = dom.new_instance(class, class, Some(parent));
    for (name, value) in props {
        dom.set_property(referent, name, value).unwrap();
    }
    referent
}

fn anchored() -> (&'static str, Variant) {
    ("Anchored", Variant::Bool(true))
}

fn part(dom: &mut WeakDom, parent: Ref, mut props: Vec<(&str, Variant)>) -> Ref {
    props.push(anchored());
    props.push((
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    ));
    add(dom, parent, "Part", props)
}

fn union(dom: &mut WeakDom, parent: Ref, triangles: i32, mut props: Vec<(&str, Variant)>) -> Ref {
    props.push(anchored());
    props.push(("TriangleCount", Variant::Int32(triangles)));
    add(dom, parent, "UnionOperation", props)
}

fn cost(dom: &WeakDom, part: Ref) -> Breakdown {
    Metrics::new(dom, &mut nothing).breakdowns()[&part]
}

fn factor(cost: &Breakdown, name: &str) -> f32 {
    cost.factors()
        .into_iter()
        .find(|(label, _)| *label == name)
        .map(|(_, points)| (points * 1e4).round() / 1e4)
        .unwrap()
}

#[test]
fn triangles_are_a_point_per_thousand() {
    let (mut dom, workspace) = world();
    let shape = union(&mut dom, workspace, 3_000, vec![]);
    assert_eq!(factor(&cost(&dom, shape), "Triangles"), 3.0);
}

#[test]
fn texture_memory_is_a_point_per_mebibyte() {
    let (mut dom, workspace) = world();
    let block = part(&mut dom, workspace, vec![]);
    add(
        &mut dom,
        block,
        "Decal",
        vec![("Texture", Variant::String("rbxassetid://512".into()))],
    );
    let mut fetch = |asset: &AssetRef| match asset {
        // 512² RGBA8 is exactly one MiB.
        AssetRef::Id(512) => Some(png(512)),
        _ => None,
    };
    let costs = Metrics::new(&dom, &mut fetch).breakdowns();
    assert_eq!(factor(&costs[&block], "Texture memory"), 1.0);
}

#[test]
fn instances_are_a_point_per_fifty() {
    let (mut dom, workspace) = world();
    let block = part(&mut dom, workspace, vec![]);
    for _ in 0..49 {
        add(&mut dom, block, "Attachment", vec![]);
    }
    let cost = cost(&dom, block);
    assert_eq!(cost.instances, 50.0);
    assert_eq!(factor(&cost, "Instances"), 1.0);
}

#[test]
fn physics_counts_simulated_parts_and_splits_constraints() {
    let (mut dom, workspace) = world();
    let fixed = part(&mut dom, workspace, vec![]);
    let loose = add(&mut dom, workspace, "Part", vec![]);
    let a = add(&mut dom, fixed, "Attachment", vec![]);
    let b = add(&mut dom, loose, "Attachment", vec![]);
    add(
        &mut dom,
        workspace,
        "RopeConstraint",
        vec![
            ("Attachment0", Variant::Ref(a)),
            ("Attachment1", Variant::Ref(b)),
        ],
    );
    let costs = Metrics::new(&dom, &mut nothing).breakdowns();
    assert_eq!(costs[&fixed].simulated, 0);
    assert_eq!(costs[&fixed].constraints, 0.5);
    assert_eq!(costs[&loose].simulated, 1);
    assert_eq!(costs[&loose].physics(), 1.0 + 0.25);
}

#[test]
fn precise_fidelity_costs_unless_the_part_never_interacts() {
    let precise = ("CollisionFidelity", Variant::Enum(3));
    let (mut dom, workspace) = world();
    let solid = union(&mut dom, workspace, 100, vec![precise.clone()]);
    let ghost = union(
        &mut dom,
        workspace,
        100,
        vec![
            precise,
            ("CanCollide", Variant::Bool(false)),
            ("CanTouch", Variant::Bool(false)),
            ("CanQuery", Variant::Bool(false)),
        ],
    );
    let costs = Metrics::new(&dom, &mut nothing).breakdowns();
    assert_eq!(costs[&solid].fidelity, 2.0);
    assert_eq!(costs[&ghost].fidelity, 0.0);
}

#[test]
fn default_fidelity_costs_only_on_a_complex_mesh() {
    let (mut dom, workspace) = world();
    let complex = union(&mut dom, workspace, 2_000, vec![]);
    let simple = union(&mut dom, workspace, 500, vec![]);
    let costs = Metrics::new(&dom, &mut nothing).breakdowns();
    assert_eq!(costs[&complex].fidelity, 1.0);
    assert_eq!(costs[&simple].fidelity, 0.0);
}

#[test]
fn a_dense_mesh_casting_shadows_is_costed_again() {
    let (mut dom, workspace) = world();
    let casting = union(&mut dom, workspace, 8_000, vec![]);
    let quiet = union(
        &mut dom,
        workspace,
        8_000,
        vec![("CastShadow", Variant::Bool(false))],
    );
    let light = union(&mut dom, workspace, 4_000, vec![]);
    let costs = Metrics::new(&dom, &mut nothing).breakdowns();
    assert_eq!(factor(&costs[&casting], "Shadows"), 4.0);
    assert_eq!(factor(&costs[&quiet], "Shadows"), 0.0);
    assert_eq!(factor(&costs[&light], "Shadows"), 0.0);
}

#[test]
fn only_enabled_lights_with_shadows_count() {
    let (mut dom, workspace) = world();
    let lamp = part(&mut dom, workspace, vec![]);
    let shadows = ("Shadows", Variant::Bool(true));
    add(&mut dom, lamp, "PointLight", vec![shadows.clone()]);
    add(&mut dom, lamp, "SpotLight", vec![]);
    add(
        &mut dom,
        lamp,
        "SurfaceLight",
        vec![shadows, ("Enabled", Variant::Bool(false))],
    );
    let cost = cost(&dom, lamp);
    assert_eq!(cost.shadow_lights, 1);
    assert_eq!(factor(&cost, "Shadows"), 3.0);
}

#[test]
fn translucent_and_glass_parts_overdraw_by_their_surface() {
    let size = (
        "size",
        Variant::Vector3(Vector3Data {
            x: 10.0,
            y: 10.0,
            z: 10.0,
        }),
    );
    let (mut dom, workspace) = world();
    let half = part(
        &mut dom,
        workspace,
        vec![size.clone(), ("Transparency", Variant::Float32(0.5))],
    );
    let glass = part(
        &mut dom,
        workspace,
        vec![size.clone(), ("Material", Variant::Enum(1568))],
    );
    let gone = part(
        &mut dom,
        workspace,
        vec![size.clone(), ("Transparency", Variant::Float32(1.0))],
    );
    let solid = part(&mut dom, workspace, vec![size]);
    let costs = Metrics::new(&dom, &mut nothing).breakdowns();
    assert_eq!(costs[&half].blended, 600.0);
    assert_eq!(factor(&costs[&half], "Overdraw"), 1.2);
    assert_eq!(costs[&glass].blended, 600.0);
    assert_eq!(costs[&gone].blended, 0.0);
    assert_eq!(costs[&solid].blended, 0.0);
}

#[test]
fn total_is_the_sum_of_the_factors() {
    let (mut dom, workspace) = world();
    let shape = union(&mut dom, workspace, 8_000, vec![]);
    let report = Metrics::new(&dom, &mut nothing).report(Metric::Total);
    // 8 triangles + 4 shadows + 1 default fidelity + 0.02 for the instance.
    let value = report.readout(shape).unwrap().value;
    assert!((value - 13.02).abs() < 1e-4, "{value}");
}

fn ranked(report: &Report) -> Vec<(&str, f32)> {
    report
        .models
        .iter()
        .map(|row| (row.name.as_str(), row.cost.total()))
        .collect()
}

#[test]
fn models_rank_by_the_sum_of_their_parts_nested_ones_folded_in() {
    let (mut dom, workspace) = world();
    let city = add(&mut dom, workspace, "Model", vec![]);
    dom.set_name(city, "City").unwrap();
    let block = add(&mut dom, city, "Model", vec![]);
    union(&mut dom, block, 4_000, vec![]);
    union(&mut dom, city, 1_000, vec![]);
    let car = add(&mut dom, workspace, "Model", vec![]);
    dom.set_name(car, "Car").unwrap();
    union(&mut dom, car, 2_000, vec![]);
    let loose = union(&mut dom, workspace, 3_000, vec![]);
    dom.set_name(loose, "Rock").unwrap();

    let report = Metrics::new(&dom, &mut nothing).report(Metric::Total);
    let names: Vec<&str> = ranked(&report).iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["City", "Rock", "Car"]);
    let city_row = &report.models[0];
    assert_eq!(city_row.referent, city);
    assert_eq!(city_row.cost.parts, 2);
    assert_eq!(city_row.cost.triangles, 5_000.0);
}

#[test]
fn physics_ranks_by_physics_and_other_metrics_rank_nothing() {
    let (mut dom, workspace) = world();
    let heavy = add(&mut dom, workspace, "Model", vec![]);
    dom.set_name(heavy, "Heavy").unwrap();
    union(
        &mut dom,
        heavy,
        50_000,
        vec![("CollisionFidelity", Variant::Enum(2))],
    );
    let cart = add(&mut dom, workspace, "Model", vec![]);
    dom.set_name(cart, "Cart").unwrap();
    add(&mut dom, cart, "Part", vec![]);

    let metrics = Metrics::new(&dom, &mut nothing);
    let physics = metrics.report(Metric::Physics);
    assert_eq!(physics.models.len(), 1);
    assert_eq!(physics.models[0].name, "Cart");
    assert!(metrics.report(Metric::Triangles).models.is_empty());
}

#[test]
fn renaming_a_model_follows_through_a_patch() {
    let (mut dom, workspace) = world();
    let model = add(&mut dom, workspace, "Model", vec![]);
    union(&mut dom, model, 1_000, vec![]);
    let mut metrics = Metrics::new(&dom, &mut nothing);

    dom.set_name(model, "Tower").unwrap();
    let changes = dom.take_changes();
    metrics.patch(&dom, &changes, &mut nothing);

    assert_eq!(metrics.report(Metric::Total).models[0].name, "Tower");
}

fn png(side: u32) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, side, side);
    encoder.set_color(png::ColorType::Grayscale);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&vec![0; (side * side) as usize])
        .unwrap();
    writer.finish().unwrap();
    out
}
