use glam::Vec3;
use rbx_dom::{Ref, Variant, WeakDom};
use rbx_viewer::pick::Ray;

use super::{
    bounds, clear_overlaps, count_scripts, face_of, land, subtree, Aim, OPEN_AIR_DISTANCE,
};

fn vec3(x: f32, y: f32, z: f32) -> rbx_dom::Vector3Data {
    rbx_dom::Vector3Data { x, y, z }
}

fn cframe(x: f32, y: f32, z: f32) -> rbx_dom::CFrameData {
    rbx_dom::CFrameData {
        position: vec3(x, y, z),
        rotation: [1., 0., 0., 0., 1., 0., 0., 0., 1.],
    }
}

fn add_part(dom: &mut WeakDom, parent: Option<Ref>, at: (f32, f32, f32)) -> Ref {
    let part = dom.new_instance("Part", "Part", parent);
    dom.set_property(part, "CFrame", Variant::CFrame(cframe(at.0, at.1, at.2)))
        .unwrap();
    // The DOM keeps `BasePart.Size` under the binary format's lowercase name.
    dom.set_property(part, "size", Variant::Vector3(vec3(4., 2., 4.)))
        .unwrap();
    part
}

fn position(dom: &WeakDom, r: Ref) -> [f32; 3] {
    match dom.get(r).and_then(|i| i.properties().get("CFrame")) {
        Some(Variant::CFrame(c)) => [c.position.x, c.position.y, c.position.z],
        other => panic!("no CFrame: {other:?}"),
    }
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

fn down_from(origin: Vec3) -> Ray {
    Ray::new(origin, Vec3::new(0., -1., 0.))
}

#[test]
fn a_part_lands_on_the_hit_point() {
    let mut dom = WeakDom::new();
    let part = add_part(&mut dom, None, (5., 10., 7.));
    let aim = Aim {
        ray: Some(down_from(Vec3::new(0., 50., 0.))),
        part: None,
        distance: Some(20.),
        normal: None,
    };
    land(&mut dom, &[part], &aim);
    let p = position(&dom, part);
    assert!(near(p[0], 0.) && near(p[1], 31.) && near(p[2], 0.), "{p:?}");
}

#[test]
fn a_part_in_open_air_lands_64_studs_along_the_ray() {
    let mut dom = WeakDom::new();
    let part = add_part(&mut dom, None, (5., 10., 7.));
    let aim = Aim {
        ray: Some(Ray::new(Vec3::new(0., 5., 0.), Vec3::new(0., 0., -1.))),
        part: None,
        distance: None,
        normal: None,
    };
    land(&mut dom, &[part], &aim);
    let p = position(&dom, part);
    assert!(
        near(p[0], 0.) && near(p[1], 5.) && near(p[2], -OPEN_AIR_DISTANCE),
        "{p:?}"
    );
}

#[test]
fn a_model_moves_its_parts_and_pivot_together() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let a = add_part(&mut dom, Some(model), (0., 3., 0.));
    let b = add_part(&mut dom, Some(model), (10., 3., 0.));
    dom.set_property(
        model,
        "WorldPivotData",
        Variant::OptionalCFrame(Some(cframe(5., 3., 0.))),
    )
    .unwrap();
    let aim = Aim {
        ray: Some(down_from(Vec3::new(0., 20., 0.))),
        part: None,
        distance: Some(10.),
        normal: None,
    };
    land(&mut dom, &[model], &aim);
    let (pa, pb) = (position(&dom, a), position(&dom, b));
    assert!(
        near(pa[0], -5.) && near(pa[1], 11.) && near(pa[2], 0.),
        "{pa:?}"
    );
    assert!(
        near(pb[0], 5.) && near(pb[1], 11.) && near(pb[2], 0.),
        "{pb:?}"
    );
    assert!(near(pb[0] - pa[0], 10.));
    match dom
        .get(model)
        .and_then(|i| i.properties().get("WorldPivotData"))
    {
        Some(Variant::OptionalCFrame(Some(c))) => {
            let p = c.position;
            assert!(near(p.x, 0.) && near(p.y, 11.) && near(p.z, 0.));
        }
        other => panic!("pivot lost: {other:?}"),
    }
}

#[test]
fn scripts_are_counted_at_any_depth() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    dom.new_instance("Script", "S", Some(model));
    let folder = dom.new_instance("Folder", "F", Some(model));
    dom.new_instance("LocalScript", "L", Some(folder));
    let deep = dom.new_instance("Folder", "D", Some(folder));
    dom.new_instance("ModuleScript", "M", Some(deep));
    dom.new_instance("Part", "P", Some(deep));
    dom.new_instance("StringValue", "V", Some(deep));
    assert_eq!(count_scripts(&dom, &[model]), 3);
}

#[test]
fn nothing_moves_without_a_ray() {
    let mut dom = WeakDom::new();
    let part = add_part(&mut dom, None, (5., 10., 7.));
    land(&mut dom, &[part], &Aim::default());
    let p = position(&dom, part);
    assert!(near(p[0], 5.) && near(p[1], 10.) && near(p[2], 7.), "{p:?}");
}

#[test]
fn a_subtree_holds_the_root_and_all_descendants() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Model", None);
    let child = dom.new_instance("Folder", "F", Some(model));
    let leaf = dom.new_instance("Part", "P", Some(child));
    let all = subtree(&dom, model);
    assert_eq!(all.len(), 3);
    assert!(all.contains(&model) && all.contains(&child) && all.contains(&leaf));
}

#[test]
fn bounds_wrap_a_scaled_box_around_its_centre() {
    let model = glam::Mat4::from_scale_rotation_translation(
        Vec3::new(4., 2., 6.),
        glam::Quat::IDENTITY,
        Vec3::new(1., 2., 3.),
    );
    let (min, max) = bounds(model);
    assert!(near(min.x, -1.) && near(min.y, 1.) && near(min.z, 0.));
    assert!(near(max.x, 3.) && near(max.y, 3.) && near(max.z, 6.));
}

#[test]
fn a_decal_takes_the_face_the_view_hits() {
    let mut dom = WeakDom::new();
    let part = add_part(&mut dom, None, (0., 0., 0.));
    assert_eq!(face_of(&dom, part, Vec3::Y), Some(1), "Top");
    assert_eq!(face_of(&dom, part, Vec3::NEG_Z), Some(5), "Front");
    assert_eq!(face_of(&dom, part, Vec3::X), Some(0), "Right");
    // A part turned a quarter about Y: its Right face looks down -Z.
    dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(rbx_dom::CFrameData {
            position: vec3(0., 0., 0.),
            rotation: [0., 0., 1., 0., 1., 0., -1., 0., 0.],
        }),
    )
    .unwrap();
    assert_eq!(face_of(&dom, part, Vec3::NEG_Z), Some(0), "turned Right");
}

/// Switching source (or opening a kit pack) clears the search field while it
/// isn't focused; that must not leave its caret blinking, which repainted the
/// whole window twice a second for as long as the Toolbox stayed open.
#[gpui_kit::test]
fn clearing_an_unfocused_search_field_leaves_it_idle(cx: &mut gpui_kit::TestAppContext) {
    use gpui_kit::component::input::{Input, InputState};
    use gpui_kit::{AppContext as _, Entity, IntoElement, Render};

    struct Field(Entity<InputState>);
    impl Render for Field {
        fn render(
            &mut self,
            _: &mut gpui_kit::Window,
            _: &mut gpui_kit::Context<Self>,
        ) -> impl IntoElement {
            Input::new(&self.0)
        }
    }

    cx.update(gpui_kit::init);
    let (view, cx) =
        cx.add_window_view(|window, cx| Field(cx.new(|cx| InputState::new(window, cx))));
    let input = view.read_with(cx, |field, _| field.0.clone());
    cx.update(|window, cx| input.update(cx, |state, cx| state.set_value("crate", window, cx)));
    cx.update(|window, cx| input.update(cx, |state, cx| state.set_value("", window, cx)));
    cx.run_until_parked();

    let notified = std::rc::Rc::new(std::cell::Cell::new(0));
    let _watch = cx.update({
        let notified = notified.clone();
        |_, cx| cx.observe(&input, move |_, _| notified.set(notified.get() + 1))
    });
    for _ in 0..6 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
    }
    assert_eq!(notified.get(), 0, "an unfocused field stays still");

    // Focused, the caret still blinks.
    cx.update(|window, cx| input.update(cx, |state, cx| state.focus(window, cx)));
    cx.run_until_parked();
    notified.set(0);
    for _ in 0..4 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(500));
        cx.run_until_parked();
    }
    assert!(notified.get() >= 3, "a focused field blinks");
}

/// Studio 0.748's Part button, measured: camera at (-19.70, 14.09, -19.30)
/// looking along (0.635, -0.442, 0.633) onto the 12 × 1 × 12 SpawnLocation
/// at the origin put the new 4 × 1 × 2 block at (-1, 1.5, -1): resting on the
/// SpawnLocation's top, its centre on the 1-stud grid.
#[test]
fn an_inserted_part_rests_where_the_view_points_on_the_grid() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    dom.set_property(part, "size", Variant::Vector3(vec3(4., 1., 2.)))
        .unwrap();
    dom.set_property(part, "CFrame", Variant::CFrame(cframe(0., 0., 0.)))
        .unwrap();
    let origin = Vec3::new(-19.696_094, 14.091_624, -19.303_886);
    let direction = Vec3::new(0.635_139_7, -0.442_004_4, 0.633_426_9).normalize();
    // Where that line meets the SpawnLocation's top, y = 1.
    let distance = (origin.y - 1.) / -direction.y;
    let aim = Aim {
        ray: Some(Ray::new(origin, direction)),
        part: None,
        distance: Some(distance),
        normal: Some(Vec3::Y),
    };
    land(&mut dom, &[part], &aim);
    crate::shell::keys::snap_footprint(&mut dom, part, 1.);
    let [x, y, z] = position(&dom, part);
    assert!(near(x, -1.) && near(y, 1.5) && near(z, -1.), "{x} {y} {z}");
}

/// Studio 0.748 put a second block inserted on the same view at
/// (-1, 2.5, -1), on top of the first at (-1, 1.5, -1), and a third at 3.5.
#[test]
fn a_part_inserted_where_one_sits_stacks_on_it() {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let block = |dom: &mut WeakDom, y: f32| {
        let part = dom.new_instance("Part", "Part", Some(workspace));
        dom.set_property(part, "size", Variant::Vector3(vec3(4., 1., 2.)))
            .unwrap();
        dom.set_property(part, "CFrame", Variant::CFrame(cframe(-1., y, -1.)))
            .unwrap();
        part
    };
    let spawn = dom.new_instance("SpawnLocation", "SpawnLocation", Some(workspace));
    dom.set_property(spawn, "size", Variant::Vector3(vec3(12., 1., 12.)))
        .unwrap();
    dom.set_property(spawn, "CFrame", Variant::CFrame(cframe(0., 0.5, 0.)))
        .unwrap();
    let first = block(&mut dom, 1.5);
    clear_overlaps(&mut dom, workspace, first);
    assert!(near(position(&dom, first)[1], 1.5));
    let second = block(&mut dom, 1.5);
    clear_overlaps(&mut dom, workspace, second);
    assert!(near(position(&dom, second)[1], 2.5));
    let third = block(&mut dom, 1.5);
    clear_overlaps(&mut dom, workspace, third);
    assert_eq!(position(&dom, third), [-1., 3.5, -1.]);
}
