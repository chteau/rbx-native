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

#[test]
fn typeof_names_roblox_types() {
    let out = lines(
        r#"
        print(typeof(workspace), typeof(game), typeof(Vector3.new()), typeof(CFrame.new()))
        print(typeof(UDim2.new()), typeof(Color3.new()), typeof(Vector3int16.new(1, 2, 3)))
        "#,
    );
    assert_eq!(
        out,
        [
            "Instance Instance Vector3 CFrame",
            "UDim2 Color3 Vector3int16"
        ]
    );
}

#[test]
fn deprecated_spellings_still_answer() {
    let out = lines(
        r#"
        local cf = CFrame.new(1, 2, 3)
        print(cf.p.X, cf.x, cf:inverse().X, Color3.new(1, 0, 0).r)
        print(Vector3.fromNormalId(Enum.NormalId.Top).Y, Vector3.FromNormalId("Right").X)
        print(workspace:findFirstChild("Baseplate") ~= nil, #workspace:children() > 0)
        local GetChildren = game.GetChildren
        print(#GetChildren(workspace) > 0, pcall(function() return game:Clone() end))
        "#,
    );
    assert_eq!(out[0], "1 1 -1 1");
    assert_eq!(out[1], "1 1");
    assert_eq!(out[2], "true true");
    assert!(out[3].starts_with("true false"), "{}", out[3]);
}

#[test]
fn tags_remove_and_tolerant_writes() {
    let out = lines(
        r#"
        local part = Instance.new("Part", workspace)
        part:AddTag("a") part:AddTag("b") part:RemoveTag("a")
        print(part:HasTag("b"), part:HasTag("a"), #part:GetTags())
        local frame = Instance.new("Frame")
        frame.Visible = nil
        print(frame.Visible, UDim2.new(UDim.new(0, 5), UDim2.new(0, 0)).X.Offset)
        part:Remove()
        print(part.Parent, part.Name)
        "#,
    );
    assert_eq!(out, ["true false 1", "false 5", "nil Part"]);
}

#[test]
fn graphemes_dates_and_fonts() {
    let out = lines(
        r#"
        local n = 0 for _ in utf8.graphemes("héllo") do n += 1 end
        local d = DateTime.fromUnixTimestamp(0)
        print(n, d:FormatUniversalTime("YYYY-MM-DD HH:mm", "en-us"), d:ToUniversalTime().Year)
        print(Font.fromName("Ubuntu").Family, Font.fromEnum(Enum.Font.SourceSansBold).Weight == Enum.FontWeight.Bold)
        "#,
    );
    assert_eq!(out[0], "5 1970-01-01 00:00 1970");
    assert_eq!(out[1], "rbxasset://fonts/families/Ubuntu.json true");
}
