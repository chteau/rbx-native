//! `BasePart:UnionAsync`, `SubtractAsync` and `IntersectAsync` from edit
//! Luau, end to end.

use rbx_dom::{Instance, Ref, WeakDom};
use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

fn printed(source: &str) -> String {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
    let mut runtime =
        Runtime::new(dom, ReflectionDatabase::embedded()).expect("runtime must start");
    runtime
        .run(source)
        .unwrap_or_else(|err| panic!("{source} failed: {err}"))
        .text()
}

const WALL_AND_DOOR: &str = r#"
    local wall = Instance.new("Part")
    wall.Size = Vector3.new(12, 10, 1)
    wall.CFrame = CFrame.new(0, 5, 0)
    wall.Parent = workspace
    local door = Instance.new("Part")
    door.Size = Vector3.new(4, 7, 3)
    door.CFrame = CFrame.new(0, 3.5, 0)
    door.Parent = workspace
"#;

#[test]
fn subtract_cuts_a_door_and_leaves_the_parts() {
    let out = printed(&format!(
        "{WALL_AND_DOOR}
        local cut = wall:SubtractAsync({{door}})
        print(cut.ClassName, cut.Name, cut.Parent, wall.Parent == workspace, door.Parent == workspace)
        print(string.format('%g %g %g', cut.Size.X, cut.Size.Y, cut.Size.Z))"
    ));
    let mut lines = out.lines();
    assert_eq!(lines.next(), Some("UnionOperation Union nil true true"));
    assert_eq!(lines.next(), Some("12 10 1"));
}

#[test]
fn union_and_intersect_make_their_classes() {
    let out = printed(&format!(
        "{WALL_AND_DOOR}
        print(wall:UnionAsync({{door}}).ClassName, wall:IntersectAsync({{door}}).ClassName)
        print(pcall(function() return wall:UnionAsync({{workspace}}) end))"
    ));
    let mut lines = out.lines();
    assert_eq!(lines.next(), Some("UnionOperation IntersectOperation"));
    assert!(lines.next().unwrap().starts_with("false"));
}
