use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

const PLACE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/tests/TestPlace.rbxl"
));

fn run(src: &str) -> Vec<String> {
    let mut runtime = Runtime::new(
        rbx_binary::deserialize(PLACE).unwrap(),
        ReflectionDatabase::embedded(),
    )
    .unwrap();
    runtime.run(src).unwrap().lines().to_vec()
}

#[test]
fn vector2_methods() {
    let out = run(r#"
        local a, b = Vector2.new(3, 4), Vector2.new(1, 2)
        print(a:Dot(b), a:Cross(b), string.format("%.3f %.3f", a.Unit.X, a.Unit.Y))
        print(tostring(a:Lerp(Vector2.zero, 0.5)), tostring(-a), tostring(2 * a))
        print(tostring(Vector2.new(-1.5, 0):Abs()), tostring(Vector2.new(-1.5, 2.2):Floor()))
        print(tostring(Vector2.new(-1.5, 2.2):Ceil()), tostring(Vector2.new(-3, 0):Sign()))
        print(tostring(a:Min(b)), tostring(a:Max(b, Vector2.new(9, 0))))
        print(a:FuzzyEq(Vector2.new(3.000001, 4)), a:FuzzyEq(b))
        print(string.format("%.3f %.3f", Vector2.xAxis:Angle(Vector2.yAxis), Vector2.yAxis:Angle(Vector2.xAxis, true)))
        print(tostring(Vector2.one), tostring(Vector2.yAxis))
    "#);
    assert_eq!(out[0], "11 2 0.600 0.800");
    assert_eq!(out[1], "1.5, 2 -3, -4 6, 8");
    assert_eq!(out[2], "1.5, 0 -2, 2");
    assert_eq!(out[3], "-1, 3 -1, 0");
    assert_eq!(out[4], "1, 2 9, 4");
    assert_eq!(out[5], "true false");
    assert_eq!(out[6], "1.571 -1.571");
    assert_eq!(out[7], "1, 1 0, 1");
}

#[test]
fn udim_arithmetic() {
    let out = run(r#"
        local a, b = UDim.new(0.5, 10), UDim.new(0.25, 4)
        print(tostring(a + b), tostring(a - b), tostring(-a))
    "#);
    assert_eq!(out[0], "0.75, 14 0.25, 6 -0.5, -10");
}

#[test]
fn udim2_arithmetic_and_lerp() {
    let out = run(r#"
        local a, b = UDim2.new(0.5, 10, 1, 20), UDim2.fromOffset(2, 4)
        print(tostring(a + b), tostring(a - b), tostring(-a))
        print(a.Width == a.X, a.Height == a.Y, a.Height.Offset)
        print(tostring(UDim2.fromScale(0, 0):Lerp(a, 0.5)))
        print(tostring(UDim2.new(UDim.new(1, 2), UDim.new(3, 4))))
    "#);
    assert_eq!(out[0], "0.5, 12, 1, 24 0.5, 8, 1, 16 -0.5, -10, -1, -20");
    assert_eq!(out[1], "true true 20");
    assert_eq!(out[2], "0.25, 5, 0.5, 10");
    assert_eq!(out[3], "1, 2, 3, 4");
}
