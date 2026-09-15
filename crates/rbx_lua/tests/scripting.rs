//! Integration tests: real place file in, scripted mutations out.

use rbx_dom::{Ref, Variant, WeakDom};
use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

const PLACE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/tests/TestPlace.rbxl"
));

fn runtime() -> Runtime {
    let dom = rbx_binary::deserialize(PLACE).expect("fixture must deserialize");
    Runtime::new(dom, ReflectionDatabase::embedded()).expect("runtime must start")
}

fn workspace_ref(dom: &WeakDom) -> Ref {
    dom.root_refs()
        .iter()
        .copied()
        .find(|referent| dom.get(*referent).is_some_and(|i| i.class() == "Workspace"))
        .expect("the place has a Workspace")
}

fn find_named(dom: &WeakDom, name: &str) -> Option<Ref> {
    let mut stack = dom.root_refs().to_vec();
    while let Some(current) = stack.pop() {
        let Some(instance) = dom.get(current) else {
            continue;
        };
        if instance.name() == name {
            return Some(current);
        }
        stack.extend_from_slice(instance.children());
    }
    None
}

#[test]
fn reads_a_vector3_component_through_a_serialized_alias() {
    let mut runtime = runtime();

    // `Size` is stored as `size` in the file; the bridge resolves the alias.
    let output = runtime
        .run("print(workspace.Baseplate.Size.X)")
        .expect("script must run");

    assert_eq!(output.text(), "2048");
}

#[test]
fn writes_a_float_property() {
    let mut runtime = runtime();

    runtime
        .run("workspace.Baseplate.Transparency = 0.5")
        .expect("script must run");

    let dom = runtime.dom();
    let baseplate = find_named(&dom, "Baseplate").expect("Baseplate exists");
    assert_eq!(
        dom.get(baseplate).unwrap().properties().get("Transparency"),
        Some(&Variant::Float32(0.5))
    );
}

#[test]
fn creates_an_instance_visible_in_the_dom() {
    let mut runtime = runtime();

    runtime
        .run(
            r#"
            local part = Instance.new("Part", workspace)
            part.Name = "Scripted"
        "#,
        )
        .expect("script must run");

    let dom = runtime.dom();
    let created = find_named(&dom, "Scripted").expect("the new part is in the DOM");
    let instance = dom.get(created).unwrap();
    assert_eq!(instance.class(), "Part");
    assert!(dom
        .get(workspace_ref(&dom))
        .unwrap()
        .children()
        .contains(&created));
}

#[test]
fn is_a_walks_the_class_hierarchy() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            local plate = workspace.Baseplate
            print(plate:IsA("BasePart"), plate:IsA("Part"), plate:IsA("Folder"))
        "#,
        )
        .expect("script must run");

    assert_eq!(output.text(), "true true false");
}

#[test]
fn get_children_sees_the_workspace_contents() {
    let mut runtime = runtime();

    let output = runtime
        .run("print(#workspace:GetChildren(), #workspace:GetDescendants())")
        .expect("script must run");

    // Camera, Baseplate, Terrain and SpawnLocation, the last two with children.
    assert_eq!(output.text(), "4 6");
}

#[test]
fn assigning_a_wrong_type_is_an_error() {
    let mut runtime = runtime();

    let error = runtime
        .run("workspace.Baseplate.Anchored = 5")
        .expect_err("a number is not a boolean");

    assert!(
        error.to_string().contains("boolean expected, got number"),
        "unexpected message: {error}"
    );
}

#[test]
fn unknown_members_are_rejected_on_both_sides() {
    let mut runtime = runtime();

    let read = runtime
        .run("print(workspace.Baseplate.Wobble)")
        .expect_err("reading an unknown member fails");
    assert!(
        read.to_string()
            .contains("Wobble is not a valid member of Part"),
        "unexpected message: {read}"
    );

    let write = runtime
        .run("workspace.Baseplate.Wobble = 1")
        .expect_err("writing an unknown member fails");
    assert!(
        write
            .to_string()
            .contains("Wobble is not a valid member of Part"),
        "unexpected message: {write}"
    );
}

#[test]
fn destroy_removes_the_whole_subtree() {
    let mut runtime = runtime();

    runtime
        .run(
            r#"
            local folder = Instance.new("Folder", workspace)
            folder.Name = "Doomed"
            local part = Instance.new("Part", folder)
            part.Name = "DoomedChild"
            folder:Destroy()
        "#,
        )
        .expect("script must run");

    let dom = runtime.dom();
    assert!(find_named(&dom, "Doomed").is_none());
    assert!(find_named(&dom, "DoomedChild").is_none());
}

#[test]
fn clone_copies_properties_under_a_fresh_ref() {
    let mut runtime = runtime();

    runtime
        .run(
            r#"
            local copy = workspace.Baseplate:Clone()
            copy.Name = "Copy"
            copy.Parent = workspace
        "#,
        )
        .expect("script must run");

    let dom = runtime.dom();
    let original = find_named(&dom, "Baseplate").expect("Baseplate exists");
    let copy = find_named(&dom, "Copy").expect("the clone exists");
    assert_ne!(original, copy);
    assert_eq!(
        dom.get(copy).unwrap().properties().get("size"),
        dom.get(original).unwrap().properties().get("size")
    );
}

#[test]
fn print_captures_every_argument_and_datatype() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            print("hello", 1, true)
            print(Vector3.new(1, 2, 3))
            print(workspace.Baseplate.Material)
        "#,
        )
        .expect("script must run");

    assert_eq!(
        output.lines(),
        ["hello 1 true", "1, 2, 3", "Enum.Material.Plastic"]
    );
}

#[test]
fn color3_keeps_the_files_uint8_representation() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            workspace.Baseplate.Color = Color3.fromRGB(255, 0, 0)
            print(workspace.Baseplate.Color.R, workspace.Baseplate.Color.B)
        "#,
        )
        .expect("script must run");

    assert_eq!(output.text(), "1 0");
    let dom = runtime.dom();
    let baseplate = find_named(&dom, "Baseplate").expect("Baseplate exists");
    assert_eq!(
        dom.get(baseplate).unwrap().properties().get("Color3uint8"),
        Some(&Variant::Color3uint8 { r: 255, g: 0, b: 0 })
    );
}

#[test]
fn cframe_transforms_a_vector3() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            local point = CFrame.new(1, 2, 3) * Vector3.new(10, 0, 0)
            print(point)
            print(CFrame.new(1, 2, 3).Position)
        "#,
        )
        .expect("script must run");

    assert_eq!(output.lines(), ["11, 2, 3", "1, 2, 3"]);
}

#[test]
fn enum_items_round_trip_through_a_property() {
    let mut runtime = runtime();

    runtime
        .run("workspace.Baseplate.Material = Enum.Material.Wood")
        .expect("script must run");

    let dom = runtime.dom();
    let baseplate = find_named(&dom, "Baseplate").expect("Baseplate exists");
    let stored = dom.get(baseplate).unwrap().properties().get("Material");
    assert_eq!(stored, Some(&Variant::Enum(512)));
}

#[test]
fn get_service_finds_a_root_service_by_class() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            print(game:GetService("Workspace").ClassName)
            print(workspace.Parent.ClassName, tostring(game))
        "#,
        )
        .expect("script must run");

    assert_eq!(output.lines(), ["Workspace", "DataModel game"]);
}

#[test]
fn services_are_not_creatable() {
    let mut runtime = runtime();

    let error = runtime
        .run(r#"Instance.new("Workspace")"#)
        .expect_err("services cannot be created");
    assert!(
        error.to_string().contains("not creatable"),
        "unexpected message: {error}"
    );
}

#[test]
fn into_dom_returns_the_mutated_tree() {
    let mut runtime = runtime();
    runtime
        .run(r#"Instance.new("Folder", workspace).Name = "Kept""#)
        .expect("script must run");

    let dom = runtime.into_dom();

    assert!(find_named(&dom, "Kept").is_some());
}

#[test]
fn vector3_operators_behave_like_luau() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            local a = Vector3.new(1, 2, 3)
            local b = Vector3.new(4, 5, 6)
            print(a + b, a - b, a * 2, b / 2)
            print(a:Dot(b), a:Cross(b))
            print(Vector3.new(0, 3, 4).Magnitude, Vector3.new(0, 3, 4).Unit)
            print(a == Vector3.new(1, 2, 3), -a)
        "#,
        )
        .expect("script must run");

    assert_eq!(
        output.lines(),
        [
            "5, 7, 9 -3, -3, -3 2, 4, 6 2, 2.5, 3",
            "32 -3, 6, -3",
            "5 0, 0.6, 0.8",
            "true -1, -2, -3",
        ]
    );
}

#[test]
fn cframe_composition_and_inverse() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            local moved = CFrame.new(Vector3.new(1, 0, 0)) * CFrame.new(0, 2, 0)
            print(moved.Position, moved.LookVector)
            print((moved * moved:Inverse()).Position)
            local turned = CFrame.Angles(0, math.pi / 2, 0)
            local spun = turned * Vector3.new(0, 0, -1)
            print(string.format("%.3f %.3f %.3f", spun.X, spun.Y, spun.Z))
        "#,
        )
        .expect("script must run");

    assert_eq!(
        output.lines(),
        // A negated zero column keeps its sign, which Luau prints as "-0".
        ["1, 2, 0 -0, -0, -1", "0, 0, 0", "-1.000 0.000 0.000"]
    );
}

#[test]
fn minimal_datatypes_expose_their_fields() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            local size = UDim2.new(0.5, 10, 0.25, 20)
            print(size.X.Scale, size.X.Offset, size.Y.Scale, size.Y.Offset)
            print(UDim.new(1, 2), Vector2.new(3, 4).Magnitude)
            print(NumberRange.new(1, 5).Min, NumberRange.new(2).Max, BrickColor.new(21).Number)
            print(Enum.Material.Wood.Value, Enum.Material.Wood.EnumType, #Enum.Font:GetEnumItems() > 0)
        "#,
        )
        .expect("script must run");

    assert_eq!(
        output.lines(),
        [
            "0.5 10 0.25 20",
            "1, 2 5",
            "1 2 21",
            "512 Enum.Material true",
        ]
    );
}
