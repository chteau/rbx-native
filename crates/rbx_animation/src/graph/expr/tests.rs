use super::*;

fn run(src: &str) -> Val {
    let params = |name: &str| match name {
        "speed" => Some(Value::Number(12.)),
        "grounded" => Some(Value::Bool(true)),
        "mode" => Some(Value::Text("run".into())),
        _ => None,
    };
    eval(src, &params).unwrap_or_else(|e| panic!("{src}: {e}"))
}

#[test]
fn arithmetic_follows_luau_precedence() {
    assert_eq!(run("1 + 2 * 3"), Val::Number(7.));
    assert_eq!(run("2 ^ 3 ^ 2"), Val::Number(512.));
    assert_eq!(run("-2 ^ 2"), Val::Number(-4.));
    assert_eq!(run("7 // 2"), Val::Number(3.));
    assert_eq!(run("-7 % 3"), Val::Number(2.));
    assert_eq!(run("(1 + 2) * 3"), Val::Number(9.));
    assert_eq!(run("0x10 + 1e2"), Val::Number(116.));
}

#[test]
fn parameters_comparisons_and_logic() {
    assert_eq!(run("speed > 10 and grounded"), Val::Bool(true));
    assert_eq!(run("speed < 10 or mode"), Val::Text("run".into()));
    assert_eq!(run("not grounded"), Val::Bool(false));
    assert_eq!(run("mode == \"run\""), Val::Bool(true));
    assert_eq!(run("missing"), Val::Nil);
    assert_eq!(run("missing ~= nil and missing > 1"), Val::Bool(false));
    assert_eq!(run("grounded or missing + 1"), Val::Bool(true));
}

#[test]
fn if_expressions_and_bodies() {
    assert_eq!(
        run("if speed > 20 then 2 elseif speed > 10 then 1 else 0"),
        Val::Number(1.)
    );
    assert_eq!(run("if false then missing + 1 else 3"), Val::Number(3.));
    assert_eq!(
        run("local fast = speed > 8\nlocal half = speed / 2\nreturn if fast then half else 0"),
        Val::Number(6.)
    );
    assert_eq!(run("'a' .. 1 .. \"b\""), Val::Text("a1b".into()));
    assert_eq!(run("-- comment\n4"), Val::Number(4.));
}

#[test]
fn math_library() {
    assert_eq!(run("math.clamp(speed, 0, 10)"), Val::Number(10.));
    assert_eq!(run("math.max(1, 5, 3)"), Val::Number(5.));
    assert_eq!(run("math.floor(math.pi)"), Val::Number(3.));
    assert_eq!(run("math.lerp(0, 10, 0.25)"), Val::Number(2.5));
    assert_eq!(run("math.sign(-3)"), Val::Number(-1.));
}

#[test]
fn errors_are_reported() {
    let none = |_: &str| None;
    for bad in [
        "1 +",
        "true + 1",
        "math.noise(1)",
        "\"open",
        "1 2",
        "local = 3",
        "if true then 1",
    ] {
        assert!(eval(bad, &none).is_err(), "{bad}");
    }
    assert_eq!(eval("", &none), Ok(Val::Nil));
}

fn close(src: &str, want: f64) {
    match run(src) {
        Val::Number(n) => assert!((n - want).abs() < 1e-9, "{src}: {n}"),
        other => panic!("{src}: {other:?}"),
    }
}

#[test]
fn vectors_are_built_combined_and_read() {
    close("Vector2.new(3, 4).Magnitude", 5.);
    close("Vector3.new(1, 2, 3).Z + 1", 4.);
    close("(Vector3.new(1, 0, 0) * 2 + Vector3.one).X", 3.);
    close("Vector3.zero.Magnitude", 0.);
    close("Vector2.new(0, 2).Unit.Y", 1.);
    close("Vector3.xAxis:Dot(Vector3.new(5, 1, 1))", 5.);
    close("Vector3.xAxis:Cross(Vector3.yAxis).Z", 1.);
    close("(-Vector2.new(1, 2)).Y", -2.);
    close(
        "math.atan2(Vector2.new(1, 1).Y, Vector2.new(1, 1).X)",
        std::f64::consts::FRAC_PI_4,
    );
    assert_eq!(
        run("Vector2.new(1, 2) == Vector2.new(1, 2)"),
        Val::Bool(true)
    );
}

#[test]
fn colours_and_cframes_are_built_and_read() {
    close("Color3.fromRGB(255, 128, 0).G", 128. / 255.);
    close("Color3.new(0.5, 0, 1).B", 1.);
    close("Color3.fromHSV(0, 1, 1).R", 1.);
    close("CFrame.new(1, 2, 3).Y", 2.);
    close("CFrame.identity.LookVector.Z", -1.);
    close(
        "(CFrame.new(1, 0, 0) * CFrame.new(0, 2, 0)).Position.Magnitude",
        5f64.sqrt(),
    );
    close("CFrame.Angles(0, math.pi / 2, 0).LookVector.X", -1.);
    close(
        "CFrame.new(Vector3.zero, Vector3.new(0, 0, 5)).LookVector.Z",
        1.,
    );
    close("(CFrame.new(0, 1, 0) + Vector3.new(0, 1, 0)).Y", 2.);
    close("(CFrame.new(0, 0, 4):Inverse() * Vector3.zero).Z", -4.);
    assert_eq!(
        run("Vector2.new(1, 2)").into_value(),
        Value::Text("1, 2".into())
    );
}

#[test]
fn bodies_may_branch_with_if_statements() {
    let src = "local s = speed / 4\nif s > 5 then\n  return \"fast\"\nelseif s > 2 then\n  return \"run\"\nend\nreturn \"walk\"";
    assert_eq!(run(src), Val::Text("run".into()));
    let src = "if speed < 0 then return 1 else 2 end";
    assert_eq!(run(src), Val::Number(2.));
    let src = "if not grounded then return 0 end\nspeed * 2";
    assert_eq!(run(src), Val::Number(24.));
    let none = |_: &str| None;
    for bad in ["if true then return 1", "if true then 1 end 2 3", "x.Nope"] {
        assert!(eval(bad, &none).is_err(), "{bad}");
    }
}
