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
fn tween_info_defaults_and_fields() {
    let out = run(r#"
        local t = TweenInfo.new()
        print(typeof(t), t.Time, t.EasingStyle == Enum.EasingStyle.Quad,
            t.EasingDirection == Enum.EasingDirection.Out, t.RepeatCount, t.Reverses, t.DelayTime)
        local u = TweenInfo.new(2, Enum.EasingStyle.Linear, Enum.EasingDirection.In, 3, true, 0.5)
        print(u.Time, u.EasingStyle == Enum.EasingStyle.Linear,
            u.EasingDirection == Enum.EasingDirection.In, u.RepeatCount, u.Reverses, u.DelayTime)
        print(t == TweenInfo.new(), t == u)
        "#);
    assert_eq!(out[0], "TweenInfo 1 true true 0 false 0");
    assert_eq!(out[1], "2 true true 3 true 0.5");
    assert_eq!(out[2], "true false");
}

#[test]
fn ray_geometry() {
    let out = run(r#"
        local r = Ray.new(Vector3.new(0, 0, 0), Vector3.new(10, 0, 0))
        print(typeof(r), r.Origin.X, r.Direction.X, r.Unit.Direction.X)
        local p = r:ClosestPoint(Vector3.new(4, 5, 0))
        print(p.X, p.Y, r:Distance(Vector3.new(4, 5, 0)))
        print(r == Ray.new(Vector3.new(0, 0, 0), Vector3.new(10, 0, 0)))
        "#);
    assert_eq!(out[0], "Ray 0 10 1");
    assert_eq!(out[1], "4 0 5");
    assert_eq!(out[2], "true");
}

#[test]
fn random_is_deterministic_and_bounded() {
    let out = run(r#"
        local a, b = Random.new(42), Random.new(42)
        print(typeof(a), a:NextNumber() == b:NextNumber(), a:NextInteger(1, 6) == b:NextInteger(1, 6))
        local ok = true
        for _ = 1, 200 do
            local n = a:NextNumber(5, 6)
            local i = a:NextInteger(-2, 2)
            if n < 5 or n >= 6 or i < -2 or i > 2 then ok = false end
        end
        print(ok)
        print(string.format("%.3f", a:NextUnitVector().Magnitude))
        local t = {1, 2, 3, 4, 5}
        a:Shuffle(t)
        table.sort(t)
        print(table.concat(t, ","))
        local c = a:Clone()
        print(c:NextNumber() == a:NextNumber())
        "#);
    assert_eq!(out[0], "Random true true");
    assert_eq!(out[1], "true");
    assert_eq!(out[2], "1.000");
    assert_eq!(out[3], "1,2,3,4,5");
    assert_eq!(out[4], "true");
}

#[test]
fn date_time_conversions() {
    let out = run(r#"
        local d = DateTime.fromUnixTimestamp(86400 + 3661)
        print(typeof(d), d.UnixTimestamp, d.UnixTimestampMillis, d:ToIsoDate())
        local m = DateTime.fromUnixTimestampMillis(1500)
        print(m.UnixTimestamp, m.UnixTimestampMillis, m == DateTime.fromUnixTimestampMillis(1500))
        print(DateTime.now().UnixTimestamp > 1700000000)
        "#);
    assert_eq!(out[0], "DateTime 90061 90061000 1970-01-02T01:01:01Z");
    assert_eq!(out[1], "1 1500 true");
    assert_eq!(out[2], "true");
}
