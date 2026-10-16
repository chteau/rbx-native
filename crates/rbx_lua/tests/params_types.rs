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

fn run(src: &str) -> Vec<String> {
    runtime().run(src).unwrap().lines().to_vec()
}

#[test]
fn raycast_params_defaults() {
    let lines = run(r#"
        local p = RaycastParams.new()
        print(p.FilterType.Name, p.IgnoreWater, p.CollisionGroup, p.RespectCanCollide, p.BruteForceAllSlow, #p.FilterDescendantsInstances)
    "#);
    assert_eq!(lines, ["Exclude false Default false false 0"]);
}

#[test]
fn raycast_params_round_trip() {
    let lines = run(r#"
        local p = RaycastParams.new()
        p.FilterType = Enum.RaycastFilterType.Include
        p.IgnoreWater = true
        p.CollisionGroup = "Players"
        p.RespectCanCollide = true
        p.BruteForceAllSlow = true
        p.FilterDescendantsInstances = { workspace }
        local folder = Instance.new("Folder")
        p:AddToFilter(folder)
        print(p.FilterType.Name, p.IgnoreWater, p.CollisionGroup, p.RespectCanCollide, p.BruteForceAllSlow)
        local list = p.FilterDescendantsInstances
        print(#list, list[1] == workspace, list[2] == folder)
    "#);
    assert_eq!(lines, ["Include true Players true true", "2 true true"]);
}

#[test]
fn add_to_filter_accepts_a_table() {
    let lines = run(r#"
        local p = RaycastParams.new()
        p:AddToFilter({ workspace, Instance.new("Folder") })
        print(#p.FilterDescendantsInstances)
    "#);
    assert_eq!(lines, ["2"]);
}

#[test]
fn overlap_params_round_trip() {
    let lines = run(r#"
        local p = OverlapParams.new()
        print(p.MaxParts, p.FilterType.Name, p.CollisionGroup)
        p.MaxParts = 7
        p.FilterType = Enum.RaycastFilterType.Include
        p.FilterDescendantsInstances = { workspace }
        print(p.MaxParts, p.FilterType.Name, #p.FilterDescendantsInstances)
    "#);
    assert_eq!(lines, ["0 Exclude Default", "7 Include 1"]);
}

#[test]
fn filter_type_rejects_other_enums() {
    let mut rt = runtime();
    assert!(rt
        .run("RaycastParams.new().FilterType = Enum.Axis.X")
        .is_err());
}

#[test]
fn region3_fields() {
    let lines = run(r#"
        local r = Region3.new(Vector3.new(0, 0, 0), Vector3.new(4, 6, 8))
        print(r.CFrame.Position, r.Size)
        local g = Region3.new(Vector3.new(0.5, 1.5, -0.5), Vector3.new(3.2, 5, 1)):ExpandToGrid(4)
        print(g.Size, g.CFrame.Position)
    "#);
    assert_eq!(lines, ["2, 3, 4 4, 6, 8", "4, 8, 8 2, 4, 0"]);
}

#[test]
fn faces_construction_and_fields() {
    let lines = run(r#"
        local f = Faces.new(Enum.NormalId.Top, Enum.NormalId.Front)
        print(f.Top, f.Bottom, f.Left, f.Right, f.Back, f.Front)
        f.Bottom = true
        f.Top = false
        print(f.Top, f.Bottom)
    "#);
    assert_eq!(lines, ["true false false false false true", "false true"]);
}

#[test]
fn axes_construction_and_fields() {
    let lines = run(r#"
        local a = Axes.new(Enum.Axis.X, Enum.NormalId.Top)
        print(a.X, a.Y, a.Z, a.Right, a.Top, a.Front)
        a.Z = true
        print(a.Z, a.Back)
    "#);
    assert_eq!(lines, ["true true false true true false", "true true"]);
}
