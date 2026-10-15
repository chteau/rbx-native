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
fn from_hsv_primary_red() {
    assert_eq!(
        lines(
            r#"local c = Color3.fromHSV(0, 1, 1)
print(string.format("%.3f %.3f %.3f", c.R, c.G, c.B))"#
        ),
        ["1.000 0.000 0.000"]
    );
}

#[test]
fn from_hsv_third_is_green() {
    assert_eq!(
        lines(
            r#"local c = Color3.fromHSV(1/3, 1, 1)
print(string.format("%.3f %.3f %.3f", c.R, c.G, c.B))"#
        ),
        ["0.000 1.000 0.000"]
    );
}

#[test]
fn from_hsv_zero_saturation_is_grey() {
    assert_eq!(
        lines(
            r#"local c = Color3.fromHSV(0.25, 0, 0.5)
print(string.format("%.3f %.3f %.3f", c.R, c.G, c.B))"#
        ),
        ["0.500 0.500 0.500"]
    );
}

#[test]
fn from_hsv_hue_wraps() {
    assert_eq!(
        lines(
            r#"local c = Color3.fromHSV(1, 1, 1)
print(string.format("%.3f %.3f %.3f", c.R, c.G, c.B))"#
        ),
        ["1.000 0.000 0.000"]
    );
}

#[test]
fn to_hsv_method_on_blue_ish_color() {
    assert_eq!(
        lines(
            r#"local h, s, v = Color3.new(0, 0.5, 1):ToHSV()
print(string.format("%.3f %.3f %.3f", h, s, v))"#
        ),
        ["0.583 1.000 1.000"]
    );
}

#[test]
fn to_hsv_static_on_red() {
    assert_eq!(
        lines(
            r#"local h, s, v = Color3.toHSV(Color3.new(1, 0, 0))
print(string.format("%.3f %.3f %.3f", h, s, v))"#
        ),
        ["0.000 1.000 1.000"]
    );
}

#[test]
fn to_hsv_grey_has_zero_hue_and_saturation() {
    assert_eq!(
        lines(
            r#"local h, s, v = Color3.new(0.5, 0.5, 0.5):ToHSV()
print(string.format("%.3f %.3f %.3f", h, s, v))"#
        ),
        ["0.000 0.000 0.500"]
    );
}

#[test]
fn hsv_round_trip() {
    assert_eq!(
        lines(
            r#"local c = Color3.fromHSV(0.75, 0.4, 0.6)
local h, s, v = c:ToHSV()
print(string.format("%.3f %.3f %.3f", h, s, v))"#
        ),
        ["0.750 0.400 0.600"]
    );
}

#[test]
fn from_hex_six_digits_with_hash() {
    assert_eq!(
        lines(
            r##"local c = Color3.fromHex("#ff8000")
print(string.format("%.3f %.3f %.3f", c.R, c.G, c.B))"##
        ),
        ["1.000 0.502 0.000"]
    );
}

#[test]
fn from_hex_six_digits_without_hash_uppercase() {
    assert_eq!(
        lines(
            r#"local c = Color3.fromHex("FF0000")
print(c == Color3.new(1, 0, 0))"#
        ),
        ["true"]
    );
}

#[test]
fn from_hex_three_digits() {
    assert_eq!(
        lines(
            r##"local c = Color3.fromHex("#abc")
print(string.format("%.3f %.3f %.3f", c.R, c.G, c.B))"##
        ),
        ["0.667 0.733 0.800"]
    );
}

#[test]
fn from_hex_rejects_non_hex_characters() {
    assert_eq!(
        lines(
            r##"local ok, err = pcall(Color3.fromHex, "#xyz123")
print(ok, string.find(tostring(err), "Unable to convert characters to hex value", 1, true) ~= nil)"##
        ),
        ["false true"]
    );
}

#[test]
fn from_hex_rejects_bad_length() {
    assert_eq!(
        lines(
            r##"local ok, err = pcall(Color3.fromHex, "#12345")
print(ok, string.find(tostring(err), "Unable to convert characters to hex value", 1, true) ~= nil)"##
        ),
        ["false true"]
    );
}

#[test]
fn lerp_midpoint() {
    assert_eq!(
        lines(
            r#"local c = Color3.new(0, 0, 0):Lerp(Color3.new(1, 0.5, 0.25), 0.5)
print(string.format("%.3f %.3f %.3f", c.R, c.G, c.B))"#
        ),
        ["0.500 0.250 0.125"]
    );
}

#[test]
fn lerp_alpha_zero_keeps_self() {
    assert_eq!(
        lines(
            r#"local c = Color3.new(0.2, 0.4, 0.6):Lerp(Color3.new(1, 1, 1), 0)
print(string.format("%.3f %.3f %.3f", c.R, c.G, c.B))"#
        ),
        ["0.200 0.400 0.600"]
    );
}

#[test]
fn from_rgb_and_to_hex_still_work() {
    assert_eq!(
        lines(
            r#"local c = Color3.fromRGB(255, 128, 0)
print(c:ToHex(), tostring(Color3.new(1, 0, 0)))"#
        ),
        ["ff8000 1, 0, 0"]
    );
}
