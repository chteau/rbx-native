//! Roblox behaviours real plugins lean on that the bindings first got
//! wrong: one userdata per instance and per enum item, binding errors that
//! reach `pcall` as strings, numbers written into string properties, the
//! deprecated lowercase vector fields, and the ancestor/class finds.

use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

const PLACE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/tests/TestPlace.rbxl"
));

fn lines(source: &str) -> Vec<String> {
    let dom = rbx_binary::deserialize(PLACE).expect("fixture must deserialize");
    let mut runtime =
        Runtime::new(dom, ReflectionDatabase::embedded()).expect("runtime must start");
    let output = runtime.run(source).expect("script must run");
    output.lines().to_vec()
}

#[test]
fn an_instance_is_one_value_however_it_is_reached() {
    let out = lines(
        r#"
        local a = workspace.Baseplate
        local keyed = { [a] = "found" }
        print(rawequal(a, workspace:FindFirstChild("Baseplate")), keyed[workspace.Baseplate])
        print(table.find(workspace:GetChildren(), a) ~= nil)
        "#,
    );
    assert_eq!(out, ["true found", "true"]);
}

#[test]
fn an_enum_item_is_one_value_however_it_is_reached() {
    let out = lines(
        r#"
        local names = {}
        for _, item in ipairs(Enum.NormalId:GetEnumItems()) do
            names[item] = item.Name
        end
        print(names[Enum.NormalId.Top], rawequal(Enum.Material.Plastic, Enum.Material:FromName("Plastic")))
        "#,
    );
    assert_eq!(out, ["Top true"]);
}

#[test]
fn a_binding_error_reaches_pcall_as_its_message() {
    let out = lines(
        r#"
        local ok, err = pcall(function() return workspace.Nope end)
        print(ok, type(err), err)
        local _, handled = xpcall(function() Instance.new("NotAClass") end, function(e) return type(e) end)
        print(handled)
        "#,
    );
    assert_eq!(
        out,
        [
            "false string Nope is not a valid member of Workspace",
            "string"
        ]
    );
}

#[test]
fn a_number_is_written_into_a_string_property_as_text() {
    let out = lines(
        r#"
        local label = Instance.new("TextLabel")
        label.Text = 5
        print(label.Text, type(label.Text))
        "#,
    );
    assert_eq!(out, ["5 string"]);
}

#[test]
fn vectors_answer_to_the_lowercase_spellings() {
    let out = lines(
        r#"
        local v = Vector3.new(3, 4, 0)
        print(v.x, v.y, v.z, v.magnitude, Vector2.new(6, 8).y, Vector2.new(6, 8).magnitude)
        "#,
    );
    assert_eq!(out, ["3 4 0 5 8 10"]);
}

#[test]
fn ancestor_and_class_finds() {
    let out = lines(
        r#"
        local model = Instance.new("Model", workspace)
        model.Name = "Outer"
        local folder = Instance.new("Folder", model)
        local part = Instance.new("Part", folder)
        print(part:FindFirstAncestor("Outer") == model, part:FindFirstAncestorOfClass("Model") == model)
        print(part:FindFirstAncestorWhichIsA("PVInstance") == model, part:IsDescendantOf(workspace), model:IsAncestorOf(part))
        print(model:FindFirstChildOfClass("Folder") == folder, model:FindFirstChildWhichIsA("BasePart", true) == part)
        model:ClearAllChildren()
        print(#model:GetChildren(), part:IsDescendantOf(game))
        "#,
    );
    assert_eq!(out, ["true true", "true true true", "true true", "0 false"]);
}
