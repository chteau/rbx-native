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
