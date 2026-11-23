//! `PVInstance:GetPivot` and `PivotTo` from Luau, end to end.

use rbx_dom::{Instance, Ref, WeakDom};
use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

fn runtime() -> Runtime {
    let mut dom = WeakDom::new();
    dom.insert(Instance::new(Ref::new(1), "Workspace", "Workspace"));
    Runtime::new(dom, ReflectionDatabase::embedded()).expect("runtime must start")
}

fn printed(runtime: &mut Runtime, source: &str) -> String {
    runtime
        .run(source)
        .unwrap_or_else(|err| panic!("{source} failed: {err}"))
        .text()
}

#[test]
fn a_part_moves_by_its_pivot_and_reports_it_back() {
    let mut runtime = runtime();
    let printed = printed(
        &mut runtime,
        r#"
        local part = Instance.new("Part")
        part.CFrame = CFrame.new(0, 0, 0)
        part.PivotOffset = CFrame.new(0, -2, 0)
        part.Parent = workspace
        part:PivotTo(CFrame.new(5, 0, 0))
        print(part.Position.X, part.Position.Y, part:GetPivot().Position.Y)
        "#,
    );
    assert_eq!(printed, "5 2 0");
}

#[test]
fn a_model_carries_its_parts_along() {
    let mut runtime = runtime();
    let printed = printed(
        &mut runtime,
        r#"
        local model = Instance.new("Model")
        local a = Instance.new("Part")
        a.CFrame = CFrame.new(0, 0, 0)
        a.Parent = model
        local b = Instance.new("Part")
        b.CFrame = CFrame.new(4, 0, 0)
        b.Parent = model
        model.PrimaryPart = a
        model.Parent = workspace
        model:PivotTo(CFrame.new(10, 1, 0))
        print(a.Position.X, b.Position.X, b.Position.Y)
        "#,
    );
    assert_eq!(printed, "10 14 1");
}

#[test]
fn something_without_a_pivot_has_no_such_member() {
    let mut runtime = runtime();
    let error = runtime
        .run(r#"Instance.new("Folder"):GetPivot()"#)
        .expect_err("a Folder has no pivot");
    assert!(
        error
            .to_string()
            .contains("GetPivot is not a valid member of Folder"),
        "{error}"
    );
}

#[test]
fn a_nested_model_s_world_pivot_is_carried_too() {
    let mut runtime = runtime();
    let printed = printed(
        &mut runtime,
        r#"
        local outer = Instance.new("Model")
        local inner = Instance.new("Model")
        local part = Instance.new("Part")
        part.CFrame = CFrame.new(4, 0, 0)
        part.Parent = inner
        inner.WorldPivot = CFrame.new(9, 0, 0)
        inner.Parent = outer
        outer.WorldPivot = CFrame.new(0, 0, 0)
        outer.Parent = workspace
        outer:PivotTo(CFrame.new(0, 3, 0))
        print(inner:GetPivot().Position.X, inner:GetPivot().Position.Y)
        "#,
    );
    assert_eq!(printed, "9 3");
}

#[test]
fn a_model_without_a_stored_pivot_reads_back_where_it_was_turned_to() {
    let mut runtime = runtime();
    let printed = printed(
        &mut runtime,
        r#"
        local model = Instance.new("Model")
        local part = Instance.new("Part")
        part.Parent = model
        model.Parent = workspace
        model:PivotTo(CFrame.new(1, 2, 3) * CFrame.Angles(0, math.rad(90), 0))
        local pivot = model:GetPivot()
        print(pivot.Position.X, pivot.Position.Y, pivot.Position.Z, math.round(pivot.LookVector.X))
        "#,
    );
    assert_eq!(printed, "1 2 3 -1");
}

#[test]
fn unassigning_a_primary_part_resets_the_pivot_to_the_box_centre() {
    let mut runtime = runtime();
    let printed = printed(
        &mut runtime,
        r#"
        local model = Instance.new("Model")
        local a = Instance.new("Part")
        a.CFrame = CFrame.new(0, 0, 0)
        a.Parent = model
        local b = Instance.new("Part")
        b.CFrame = CFrame.new(10, 0, 0)
        b.Parent = model
        model.WorldPivot = CFrame.new(-20, 4, 0)
        model.PrimaryPart = b
        model.PrimaryPart = nil
        print(model.PrimaryPart, model:GetPivot().Position.X, model:GetPivot().Position.Y)
        "#,
    );
    assert_eq!(printed, "nil 5 0");
}

#[test]
fn a_model_scales_around_its_pivot_relative_to_scale_one() {
    let mut runtime = runtime();
    let printed = printed(
        &mut runtime,
        r#"
        local model = Instance.new("Model")
        local part = Instance.new("Part")
        part.Size = Vector3.new(2, 2, 2)
        part.CFrame = CFrame.new(2, 1, 0)
        part.Parent = model
        model.WorldPivot = CFrame.new(0, 0, 0)
        model.Parent = workspace
        local before = model:GetScale()
        model:ScaleTo(2)
        local grown = string.format("%g %g %g", part.Size.X, part.Position.X, model:GetScale())
        model:ScaleTo(1)
        print(before, grown, part.Size.X, part.Position.X)
        print(pcall(function() model:ScaleTo(0) end))
        "#,
    );
    let mut lines = printed.lines();
    assert_eq!(lines.next(), Some("1 4 4 2 2 2"));
    assert!(lines.next().unwrap().starts_with("false"));
}
