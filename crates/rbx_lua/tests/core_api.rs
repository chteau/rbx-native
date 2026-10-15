use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

const PLACE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/tests/TestPlace.rbxl"
));

fn runtime() -> Runtime {
    Runtime::new(
        rbx_binary::deserialize(PLACE).unwrap(),
        ReflectionDatabase::embedded(),
    )
    .unwrap()
}

fn lines(src: &str) -> Vec<String> {
    runtime().run(src).unwrap().lines().to_vec()
}

#[test]
fn enum_api() {
    let out = lines(
        r#"
        local item = Enum.Material.Plastic
        print(item:IsA("Material"), item:IsA("Font"))
        print(Enum.Material:FromName("Plastic") == item, Enum.Material:FromName("Nope"))
        print(Enum.Material:FromValue(item.Value) == item, Enum.Material:FromValue(-5))
        print(typeof(Enum.Material), typeof(item), typeof(Enum))
        "#,
    );
    assert_eq!(out[0], "true false");
    assert_eq!(out[1], "true nil");
    assert_eq!(out[2], "true nil");
    assert_eq!(out[3], "Enum EnumItem Enums");
}

#[test]
fn game_api() {
    let out = lines(
        r#"
        print(game:IsA("DataModel"), game:IsA("ServiceProvider"), game:IsA("Instance"), game:IsA("Part"))
        print(game:FindService("Workspace") == workspace, game:FindService("Players") == nil or true)
        print(game:GetFullName(), game.PlaceId, game.GameId, game.CreatorId, game.PlaceVersion)
        print(game.JobId == "", game.CreatorType == Enum.CreatorType.User)
        "#,
    );
    assert_eq!(out[0], "true true true false");
    assert_eq!(out[1], "true true");
    assert_eq!(out[2], "Game 0 0 0 0");
    assert_eq!(out[3], "true true");
}

#[test]
fn version_global() {
    let out = lines("print(version())");
    assert_eq!(out[0], env!("CARGO_PKG_VERSION"));
}

#[test]
fn debug_id_is_stable() {
    let out = lines(
        r#"
        local a = Instance.new("Part")
        local b = Instance.new("Part")
        print(a:GetDebugId() == a:GetDebugId(), a:GetDebugId() ~= b:GetDebugId())
        "#,
    );
    assert_eq!(out[0], "true true");
}

#[test]
fn model_bounds() {
    let out = lines(
        r#"
        local model = Instance.new("Model")
        local a = Instance.new("Part")
        a.Size = Vector3.new(2, 2, 2)
        a.CFrame = CFrame.new(0, 0, 0)
        a.Parent = model
        local b = Instance.new("Part")
        b.Size = Vector3.new(2, 2, 2)
        b.CFrame = CFrame.new(10, 0, 0)
        b.Parent = model
        local size = model:GetExtentsSize()
        print(string.format("%.3f %.3f %.3f", size.X, size.Y, size.Z))
        local cf, s = model:GetBoundingBox()
        print(string.format("%.3f %.3f %.3f", cf.X, cf.Y, cf.Z))
        print(string.format("%.3f", s.X))
        "#,
    );
    assert_eq!(out[0], "12.000 2.000 2.000");
    assert_eq!(out[1], "5.000 0.000 0.000");
    assert_eq!(out[2], "12.000");
}

#[test]
fn part_queries_and_non_models() {
    let out = lines(
        r#"
        local p = Instance.new("Part")
        print(#p:GetTouchingParts(), #p:GetConnectedParts())
        print(pcall(function() return p:GetBoundingBox() end))
        "#,
    );
    assert_eq!(out[0], "0 0");
    assert!(out[1].starts_with("false"));
    assert!(out[1].contains("GetBoundingBox is not a valid member of Part"));
}
