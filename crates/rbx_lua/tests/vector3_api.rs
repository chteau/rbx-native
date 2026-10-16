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
fn lerp_abs_floor_ceil_sign() {
    let out = run(r#"
        local a = Vector3.new(0, 0, 0)
        print(tostring(a:Lerp(Vector3.new(10, -4, 2), 0.5)))
        print(tostring(Vector3.new(-1, 2, -3):Abs()))
        print(tostring(Vector3.new(1.5, -1.5, 2):Floor()))
        print(tostring(Vector3.new(1.5, -1.5, 2):Ceil()))
        print(tostring(Vector3.new(-7, 0, 3):Sign()))
    "#);
    assert_eq!(
        out,
        ["5, -2, 1", "1, 2, 3", "1, -2, 2", "2, -1, 2", "-1, 0, 1"]
    );
}

#[test]
fn min_max_fuzzy() {
    let out = run(r#"
        local a = Vector3.new(1, 5, 3)
        print(tostring(a:Min(Vector3.new(2, 2, 2), Vector3.new(0, 9, 9))))
        print(tostring(a:Max(Vector3.new(2, 2, 2), Vector3.new(0, 9, 1))))
        print(a:FuzzyEq(Vector3.new(1.000001, 5, 3)))
        print(a:FuzzyEq(Vector3.new(1.1, 5, 3)))
        print(a:FuzzyEq(Vector3.new(1.1, 5, 3), 0.1))
    "#);
    assert_eq!(out, ["0, 2, 2", "2, 9, 3", "true", "false", "true"]);
}

#[test]
fn angle() {
    let out = run(r#"
        local x, y = Vector3.xAxis, Vector3.yAxis
        print(string.format("%.4f", x:Angle(y)))
        print(string.format("%.4f", x:Angle(y, Vector3.zAxis)))
        print(string.format("%.4f", x:Angle(y, -Vector3.zAxis)))
    "#);
    assert_eq!(out, ["1.5708", "1.5708", "-1.5708"]);
}

#[test]
fn scalar_arithmetic() {
    let out = run(r#"
        local v = Vector3.new(1, 2, 4)
        print(tostring(2 * v))
        print(tostring(v * 2))
        print(tostring(2 / v))
        print(tostring(v / 2))
        print(tostring(v * v))
        print(tostring(v / v))
        print(tostring(v // 3))
    "#);
    assert_eq!(
        out,
        [
            "2, 4, 8",
            "2, 4, 8",
            "2, 1, 0.5",
            "0.5, 1, 2",
            "1, 4, 16",
            "1, 1, 1",
            "0, 0, 1"
        ]
    );
}
