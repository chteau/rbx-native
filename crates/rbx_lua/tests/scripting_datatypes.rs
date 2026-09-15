//! Integration tests for the datatypes added on top of the base bridge:
//! `NumberSequence`/`ColorSequence`, `NumberRange`, `Rect`,
//! `PhysicalProperties`, `Font` and `Content`. Split from `scripting.rs`
//! to keep both files under the project's line-length limit.

use rbx_lua::Runtime;
use rbx_reflection::ReflectionDatabase;

const PLACE: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/tests/TestPlace.rbxl"
));

fn runtime() -> Runtime {
    let dom = rbx_binary::deserialize(PLACE).expect("fixture must deserialize");
    Runtime::new(dom, ReflectionDatabase::embedded()).expect("runtime must start")
}

#[test]
fn number_range_new_round_trips_through_a_real_property() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            local emitter = Instance.new("ParticleEmitter")
            emitter.Speed = NumberRange.new(2, 9)
            print(emitter.Speed.Min, emitter.Speed.Max)
            print(NumberRange.new(4).Min, NumberRange.new(4).Max)
        "#,
        )
        .expect("script must run");

    assert_eq!(output.lines(), ["2 9", "4 4"]);
}

#[test]
fn number_sequence_and_color_sequence_round_trip_through_a_real_property() {
    let mut runtime = runtime();

    // `UIGradient` in the fixture starts with two identical keypoints.
    let output = runtime
        .run(
            r#"
            local grad = game:FindFirstChild("UIGradient", true)
            print(grad.Transparency.Keypoints[1].Value, grad.Transparency.Keypoints[2].Value)

            grad.Transparency = NumberSequence.new(0.25)
            print(#grad.Transparency.Keypoints, grad.Transparency.Keypoints[1].Value,
                grad.Transparency.Keypoints[2].Value)

            grad.Color = ColorSequence.new(Color3.new(1, 0, 0), Color3.new(0, 0, 1))
            print(grad.Color.Keypoints[1].Value.R, grad.Color.Keypoints[2].Value.B)

            local built = NumberSequence.new({
                NumberSequenceKeypoint.new(0, 1),
                NumberSequenceKeypoint.new(1, 2),
            })
            grad.Transparency = built
            print(#grad.Transparency.Keypoints, grad.Transparency.Keypoints[2].Value)
        "#,
        )
        .expect("script must run");

    assert_eq!(output.lines(), ["0 0", "2 0.25 0.25", "1 1", "2 2"]);
}

#[test]
fn keypoint_constructors_expose_their_fields() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            local kp = NumberSequenceKeypoint.new(0.5, 0.75, 0.25)
            print(kp.Time, kp.Value, kp.Envelope)
            local ckp = ColorSequenceKeypoint.new(0.5, Color3.new(0, 1, 0))
            print(ckp.Time, ckp.Value.G, ckp.Envelope)
        "#,
        )
        .expect("script must run");

    assert_eq!(output.lines(), ["0.5 0.75 0.25", "0.5 1 0"]);
}

#[test]
fn rect_new_accepts_numbers_or_two_vector2s_and_round_trips() {
    let mut runtime = runtime();

    let output = runtime
        .run(
            r#"
            local r = Rect.new(1, 2, 3, 4)
            print(r.Min.X, r.Min.Y, r.Max.X, r.Max.Y, r.Width, r.Height)

            local r2 = Rect.new(Vector2.new(0, 0), Vector2.new(10, 20))
            print(r2.Width, r2.Height)

            local img = Instance.new("ImageLabel")
            img.SliceCenter = r
            print(img.SliceCenter.Min.X, img.SliceCenter.Max.Y)
        "#,
        )
        .expect("script must run");

    assert_eq!(output.lines(), ["1 2 3 4 2 2", "10 20", "1 4"]);
}

#[test]
fn physical_properties_round_trips_through_custom_physical_properties() {
    let mut runtime = runtime();

    // The fixture's `Baseplate` uses default physics, which Roblox (and this
    // bridge) reads back as `nil`, not an object.
    let output = runtime
        .run(
            r#"
            print(workspace.Baseplate.CustomPhysicalProperties == nil)

            workspace.Baseplate.CustomPhysicalProperties = PhysicalProperties.new(2, 0.5, 1)
            local pp = workspace.Baseplate.CustomPhysicalProperties
            print(pp.Density, pp.Friction, pp.Elasticity, pp.FrictionWeight, pp.ElasticityWeight)

            workspace.Baseplate.CustomPhysicalProperties = nil
            print(workspace.Baseplate.CustomPhysicalProperties == nil)
        "#,
        )
        .expect("script must run");

    assert_eq!(output.lines(), ["true", "2 0.5 1 1 1", "true"]);
}

#[test]
fn font_face_round_trips_while_the_legacy_font_enum_still_reads() {
    let mut runtime = runtime();

    // `BubbleChatConfiguration` carries both the legacy `Font` enum property and
    // the newer `FontFace` datatype property; writing one must not disturb the
    // other's read path.
    let output = runtime
        .run(
            r#"
            local cfg = game:FindFirstChild("BubbleChatConfiguration", true)
            print(cfg.FontFace.Family, cfg.FontFace.Weight, cfg.FontFace.Style)
            print(typeof(cfg.Font) == "number" or typeof(cfg.Font) == "EnumItem")

            cfg.FontFace = Font.new("rbx://foo", 700, "Italic")
            print(cfg.FontFace.Family, cfg.FontFace.Weight, cfg.FontFace.Style)
        "#,
        )
        .expect("script must run");

    assert_eq!(
        output.lines(),
        [
            "rbxasset://fonts/families/BuilderSans.json 500 Normal",
            "true",
            "rbx://foo 700 Italic",
        ]
    );
}

#[test]
fn content_property_accepts_a_plain_string_and_round_trips() {
    let mut runtime = runtime();

    // `Decal.Texture` is the everyday case: scripts assign a plain
    // `"rbxassetid://..."` string to a `Content`-typed property.
    let output = runtime
        .run(
            r#"
            local decal = Instance.new("Decal")
            decal.Texture = "rbxassetid://42"
            print(decal.Texture)
        "#,
        )
        .expect("script must run");

    assert_eq!(output.text(), "rbxassetid://42");
}

#[test]
fn wrong_argument_errors_for_new_datatype_constructors() {
    {
        let mut runtime = runtime();
        let error = runtime
            .run("NumberSequence.new()")
            .expect_err("no arguments is invalid");
        assert!(error.to_string().contains("NumberSequence.new expects"));
    }
    {
        let mut runtime = runtime();
        let error = runtime
            .run("ColorSequence.new()")
            .expect_err("no arguments is invalid");
        assert!(error.to_string().contains("ColorSequence.new expects"));
    }
    {
        let mut runtime = runtime();
        let error = runtime
            .run("NumberSequenceKeypoint.new(1)")
            .expect_err("value is required");
        assert!(error.to_string().contains("f32"));
    }
    {
        let mut runtime = runtime();
        let error = runtime
            .run("ColorSequenceKeypoint.new(1)")
            .expect_err("color is required");
        assert!(error.to_string().contains("Color3 expected"));
    }
    {
        let mut runtime = runtime();
        let error = runtime
            .run("Rect.new(1, 2, 3)")
            .expect_err("three numbers is not a valid overload");
        assert!(error.to_string().contains("Rect.new expects"));
    }
    {
        let mut runtime = runtime();
        let error = runtime
            .run("PhysicalProperties.new(1, 2)")
            .expect_err("elasticity is required");
        assert!(error.to_string().contains("f32"));
    }
    {
        let mut runtime = runtime();
        let error = runtime.run("Font.new()").expect_err("family is required");
        assert!(error.to_string().contains("String"));
    }
}
