use std::cell::RefCell;
use std::rc::Rc;

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::{Event, Physics, ServerVm};

struct NoPhysics;

impl Physics for NoPhysics {
    fn mass(&self, _: Ref) -> Option<f64> {
        Some(2.5)
    }
    fn assembly_mass(&self, _: Ref) -> Option<f64> {
        None
    }
    fn apply_impulse(&self, _: Ref, _: [f64; 3], _: bool) {}
}

fn place(scripts: &[(&str, &str, &str)]) -> (Rc<RefCell<WeakDom>>, Vec<Ref>) {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let sss = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let mut refs = Vec::new();
    for (class, name, source) in scripts {
        let r = dom.new_instance(class, name, Some(sss));
        dom.set_property(r, "Source", Variant::String((*source).into()))
            .unwrap();
        refs.push(r);
    }
    dom.new_instance("Part", "Part", Some(workspace));
    dom.take_changes();
    (Rc::new(RefCell::new(dom)), refs)
}

fn vm(dom: Rc<RefCell<WeakDom>>) -> ServerVm {
    let db = ReflectionDatabase::shared().clone();
    ServerVm::new(dom, db, Rc::new(NoPhysics)).unwrap()
}

#[test]
fn scripts_run_with_their_own_script_and_require() {
    let (dom, refs) = place(&[
        ("ModuleScript", "Mod", "return { n = 41 }"),
        (
            "Script",
            "Main",
            "print(script.Name, require(script.Parent.Mod).n + 1, game:GetService('RunService'):IsServer())",
        ),
    ]);
    let mut vm = vm(dom);
    vm.start_script(refs[1]).unwrap();
    vm.resume();
    assert_eq!(vm.take_output().lines(), ["Main 42 true"]);
}

#[test]
fn task_wait_follows_the_frame_clock() {
    let (dom, refs) = place(&[(
        "Script",
        "S",
        "local beats = 0
         game:GetService('RunService').Heartbeat:Connect(function(dt) beats += 1 end)
         local waited = task.wait(0.05)
         print(beats, string.format('%.4f', waited))",
    )]);
    let mut vm = vm(dom);
    vm.start_script(refs[0]).unwrap();
    vm.resume();
    for frame in 1..=4 {
        vm.set_time(frame as f64 / 60.0);
        vm.fire(Event::RunService("Heartbeat", vec![1.0 / 60.0]));
        vm.resume();
    }
    // The first frame strictly past 0.05 s: the fourth (WaitRounding golden).
    assert_eq!(vm.take_output().lines(), ["4 0.0667"]);
}

#[test]
fn touched_and_mass_reach_scripts() {
    let (dom, refs) = place(&[(
        "Script",
        "S",
        "local p = workspace.Part
         p.Touched:Connect(function(o) print('touched', o.Name, p:GetMass()) end)",
    )]);
    let part = dom
        .borrow()
        .root_refs()
        .iter()
        .find_map(|r| {
            let dom = dom.borrow();
            dom.get(*r)
                .filter(|i| i.class() == "Workspace")
                .map(|i| i.children()[0])
        })
        .unwrap();
    let mut vm = vm(dom);
    vm.start_script(refs[0]).unwrap();
    vm.resume();
    vm.fire(Event::Touched { part, other: part });
    assert_eq!(vm.take_output().lines(), ["touched Part 2.5"]);
}

#[test]
fn collision_groups_register_and_pair() {
    let (dom, refs) = place(&[(
        "Script",
        "S",
        "local ps = game:GetService('PhysicsService')
         ps:RegisterCollisionGroup('A')
         ps:CollisionGroupSetCollidable('A', 'Default', false)
         print(ps:CollisionGroupsAreCollidable('A', 'Default'), ps:IsCollisionGroupRegistered('A'))",
    )]);
    let mut vm = vm(dom);
    vm.start_script(refs[0]).unwrap();
    vm.resume();
    assert_eq!(vm.take_output().lines(), ["false true"]);
    assert!(!vm.collision_groups().borrow().collides("A", "Default"));
}
