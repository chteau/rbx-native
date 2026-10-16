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
    // Snap near-zero numbers so "-0.000" never appears in formatted output. The
    // global `string` library is read-only, so shadow it with a local wrapper.
    const PRELUDE: &str = "local sf = string.format\n\
        local string = setmetatable({ format = function(f, ...) \
        local a = table.pack(...); for i = 1, a.n do \
        if type(a[i]) == 'number' and math.abs(a[i]) < 5e-4 then a[i] = 0 end end \
        return sf(f, table.unpack(a, 1, a.n)) end }, { __index = string })\n";
    runtime()
        .run(&format!("{PRELUDE}{src}"))
        .unwrap()
        .lines()
        .to_vec()
}

#[test]
fn look_at_plus_x() {
    let out = run(r##"
        local cf = CFrame.lookAt(Vector3.new(0,0,0), Vector3.new(5,0,0))
        local l = cf.LookVector
        print(string.format("%.3f %.3f %.3f", l.X, l.Y, l.Z))
        local c2 = CFrame.new(Vector3.new(1,2,3), Vector3.new(1,2,-7))
        local l2 = c2.LookVector
        print(string.format("%.3f %.3f %.3f %.1f", l2.X, l2.Y, l2.Z, c2.X))
    "##);
    assert_eq!(out[0], "1.000 0.000 0.000");
    assert_eq!(out[1], "0.000 0.000 -1.000 1.0");
}

#[test]
fn new_overloads() {
    let out = run(r##"
        local a = CFrame.new(1,2,3,0,0,0,1)
        local b = CFrame.new(1,2,3,1,0,0,0,1,0,0,0,1)
        print(a == b, a.Position == Vector3.new(1,2,3))
        local q = CFrame.new(0,0,0,0,math.sin(math.pi/4),0,math.cos(math.pi/4))
        local l = q.LookVector
        print(string.format("%.3f %.3f %.3f", l.X, l.Y, l.Z))
    "##);
    assert_eq!(out[0], "true true");
    assert_eq!(out[1], "-1.000 0.000 0.000");
}

#[test]
fn from_matrix_and_axes() {
    let out = run(r##"
        local cf = CFrame.fromMatrix(Vector3.new(1,2,3), Vector3.new(0,1,0), Vector3.new(-1,0,0))
        local z = cf.ZVector
        print(string.format("%.3f %.3f %.3f", z.X, z.Y, z.Z))
        local x = cf.XVector
        print(string.format("%.3f %.3f %.3f", x.X, x.Y, x.Z))
        print(cf.Rotation.Position == Vector3.new(0,0,0))
    "##);
    assert_eq!(out[0], "0.000 0.000 1.000");
    assert_eq!(out[1], "0.000 1.000 0.000");
    assert_eq!(out[2], "true");
}

#[test]
fn space_conversion_round_trip() {
    let out = run(r##"
        local cf = CFrame.new(1,2,3) * CFrame.Angles(0.3, 0.5, 0.7)
        local other = CFrame.new(4,5,6) * CFrame.Angles(0.1, 0.2, 0.3)
        print(cf:ToObjectSpace(cf:ToWorldSpace(other)):FuzzyEq(other, 1e-4))
        local p1, p2 = cf:PointToObjectSpace(Vector3.new(1,2,3), Vector3.new(2,2,3))
        print(string.format("%.3f %.3f", p1.Magnitude, p2.X))
        local w = CFrame.new(10,0,0):PointToWorldSpace(Vector3.new(1,1,1))
        print(w.X)
        local v = CFrame.Angles(0, math.pi/2, 0):VectorToWorldSpace(Vector3.new(0,0,-1))
        print(string.format("%.3f %.3f %.3f", v.X, v.Y, v.Z))
        local back = CFrame.Angles(0, math.pi/2, 0):VectorToObjectSpace(v)
        print(string.format("%.3f %.3f %.3f", back.X, back.Y, back.Z))
    "##);
    assert_eq!(out[0], "true");
    assert_eq!(out[1].split(' ').next().unwrap(), "0.000");
    assert_eq!(out[2], "11");
    assert_eq!(out[3], "-1.000 0.000 0.000");
    assert_eq!(out[4], "0.000 0.000 -1.000");
}

#[test]
fn components_and_angles() {
    let out = run(r##"
        local cf = CFrame.new(1,2,3)
        print(select("#", cf:GetComponents()), select("#", cf:components()))
        local rx, ry, rz = CFrame.fromOrientation(0.2, 0.4, 0.6):ToOrientation()
        print(string.format("%.3f %.3f %.3f", rx, ry, rz))
        local a, b, c = CFrame.Angles(0.2, 0.4, 0.6):ToEulerAnglesXYZ()
        print(string.format("%.3f %.3f %.3f", a, b, c))
        local x, y, z = CFrame.fromEulerAnglesYXZ(0.1, 0.2, 0.3):ToEulerAnglesYXZ()
        print(string.format("%.3f %.3f %.3f", x, y, z))
    "##);
    assert_eq!(out[0], "12 12");
    assert_eq!(out[1], "0.200 0.400 0.600");
    assert_eq!(out[2], "0.200 0.400 0.600");
    assert_eq!(out[3], "0.100 0.200 0.300");
}

#[test]
fn axis_angle_round_trip() {
    let out = run(r##"
        local cf = CFrame.fromAxisAngle(Vector3.new(0,1,0), math.pi/2)
        local l = cf.LookVector
        print(string.format("%.3f %.3f %.3f", l.X, l.Y, l.Z))
        local axis, angle = cf:ToAxisAngle()
        print(string.format("%.3f %.3f %.3f %.3f", axis.X, axis.Y, axis.Z, angle))
    "##);
    assert_eq!(out[0], "-1.000 0.000 0.000");
    assert_eq!(out[1], "0.000 1.000 0.000 1.571");
}

#[test]
fn lerp_halfway_is_45_degrees() {
    let out = run(r##"
        local a = CFrame.new(0,0,0)
        local b = CFrame.new(10,0,0) * CFrame.Angles(0, math.pi/2, 0)
        local m = a:Lerp(b, 0.5)
        local _, ry, _ = m:ToOrientation()
        print(string.format("%.3f %.3f", m.X, math.deg(ry)))
    "##);
    assert_eq!(out[0], "5.000 45.000");
}

#[test]
fn orthonormalize_fuzzy_and_translate() {
    let out = run(r##"
        local skew = CFrame.new(0,0,0,2,0,0,0,3,0,0,0,4)
        local o = skew:Orthonormalize()
        print(o:FuzzyEq(CFrame.new()))
        print(CFrame.new(1,1,1):FuzzyEq(CFrame.new(1,1,1.5), 0.1))
        local t = CFrame.new(1,2,3) + Vector3.new(1,1,1)
        local u = CFrame.new(1,2,3) - Vector3.new(1,1,1)
        print(t.X, t.Y, t.Z, u.X, u.Y, u.Z)
    "##);
    assert_eq!(out[0], "true");
    assert_eq!(out[1], "false");
    assert_eq!(out[2], "2 3 4 0 1 2");
}
