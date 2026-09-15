//! Integration tests for `Instance.new`'s class defaults and the CFrame-backed
//! pseudo-properties (`Position`, `Orientation`, `Rotation`, `BrickColor`).

use rbx_dom::{Ref, Variant, Vector3Data, WeakDom};
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

fn find_named(dom: &WeakDom, name: &str) -> Option<Ref> {
    let mut stack = dom.root_refs().to_vec();
    while let Some(current) = stack.pop() {
        let Some(instance) = dom.get(current) else {
            continue;
        };
        if instance.name() == name {
            return Some(current);
        }
        stack.extend_from_slice(instance.children());
    }
    None
}

#[test]
fn instance_new_gets_studio_like_part_defaults() {
    let mut runtime = runtime();
    runtime
        .run(r#"Instance.new("Part", workspace).Name = "FreshPart""#)
        .expect("script must run");

    let dom = runtime.dom();
    let referent = find_named(&dom, "FreshPart").expect("the new Part exists");
    let part = dom.get(referent).expect("instance is in the DOM");

    // Exact file-storage keys, per `assets/tests/TestPlace.rbxl`'s own Part.
    assert_eq!(
        part.properties().get("size"),
        Some(&Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 1.2,
            z: 2.0
        }))
    );
    assert!(matches!(
        part.properties().get("CFrame"),
        Some(Variant::CFrame(frame))
            if frame.position == Vector3Data { x: 0.0, y: 0.0, z: 0.0 }
    ));
    assert_eq!(
        part.properties().get("Color3uint8"),
        Some(&Variant::Color3uint8 {
            r: 163,
            g: 162,
            b: 165
        })
    );
    assert_eq!(part.properties().get("Material"), Some(&Variant::Enum(256)));
    assert_eq!(part.properties().get("shape"), Some(&Variant::Enum(1)));
    assert_eq!(
        part.properties().get("Anchored"),
        Some(&Variant::Bool(false))
    );
    assert_eq!(
        part.properties().get("CanCollide"),
        Some(&Variant::Bool(true))
    );
}

#[test]
fn position_setter_keeps_the_existing_rotation() {
    let mut runtime = runtime();
    let output = runtime
        .run(
            r#"
            local p = Instance.new("Part")
            p.CFrame = CFrame.Angles(0, math.rad(90), 0)
            p.Position = Vector3.new(1, 2, 3)
            print(p.Position)
            local lv = p.CFrame.LookVector
            print(string.format("%.3f %.3f %.3f", lv.X, lv.Y, lv.Z))
        "#,
        )
        .expect("script must run");

    // A negated zero component keeps its sign, which Luau prints as "-0"
    // (same quirk the existing `cframe_composition_and_inverse` test notes).
    assert_eq!(output.lines(), ["1, 2, 3", "-1.000 -0.000 0.000"]);
}

#[test]
fn orientation_setter_matches_rotating_the_look_vector() {
    let mut runtime = runtime();
    let output = runtime
        .run(
            r#"
            local p = Instance.new("Part")
            p.Orientation = Vector3.new(0, 90, 0)
            local lv = p.CFrame.LookVector
            print(string.format("%.3f %.3f %.3f", lv.X, lv.Y, lv.Z))

            local o = p.Orientation
            print(string.format("%.3f %.3f %.3f", o.X, o.Y, o.Z))
            local r = p.Rotation
            print(string.format("%.3f %.3f %.3f", r.X, r.Y, r.Z))
        "#,
        )
        .expect("script must run");

    // Derived the same way the existing `cframe_composition_and_inverse` test
    // checks CFrame.Angles: rotating (0, 0, -1) 90 degrees around Y points -X.
    // Negated zero components keep their sign, same as that test's -0 quirk.
    assert_eq!(
        output.lines(),
        [
            "-1.000 -0.000 0.000",
            "-0.000 90.000 0.000",
            "-0.000 90.000 0.000",
        ]
    );
}

#[test]
fn size_and_color_are_explicit_property_aliases() {
    let mut runtime = runtime();
    runtime
        .run(
            r#"
            local p = Instance.new("Part", workspace)
            p.Name = "Aliased"
            p.Size = Vector3.new(5, 6, 7)
            p.Color = Color3.new(1, 0, 0)
        "#,
        )
        .expect("script must run");

    let dom = runtime.dom();
    let referent = find_named(&dom, "Aliased").expect("the part exists");
    let part = dom.get(referent).expect("instance is in the DOM");

    // `Size`/`Color` are scripting names; the file stores `size`/`Color3uint8`.
    assert_eq!(
        part.properties().get("size"),
        Some(&Variant::Vector3(Vector3Data {
            x: 5.0,
            y: 6.0,
            z: 7.0
        }))
    );
    assert_eq!(
        part.properties().get("Color3uint8"),
        Some(&Variant::Color3uint8 { r: 255, g: 0, b: 0 })
    );
}

#[test]
fn brick_color_new_accepts_a_known_name_or_a_number() {
    let mut runtime = runtime();
    let output = runtime
        .run(r#"print(BrickColor.new("Bright red").Number, BrickColor.new(21).Number)"#)
        .expect("script must run");

    assert_eq!(output.lines(), ["21 21"]);
}

#[test]
fn brick_color_new_rejects_an_unknown_name() {
    let mut runtime = runtime();
    let error = runtime
        .run(r#"BrickColor.new("Not A Real Color")"#)
        .expect_err("unknown BrickColor names must be rejected");

    assert!(error.to_string().contains("is not one of the names"));
}

#[test]
fn assigning_a_brick_color_writes_color3uint8() {
    let mut runtime = runtime();
    runtime
        .run(
            r#"
            local p = Instance.new("Part", workspace)
            p.Name = "Reddish"
            p.BrickColor = BrickColor.new("Bright red")
        "#,
        )
        .expect("script must run");

    let dom = runtime.dom();
    let referent = find_named(&dom, "Reddish").expect("the part exists");
    let part = dom.get(referent).expect("instance is in the DOM");

    assert_eq!(
        part.properties().get("Color3uint8"),
        Some(&Variant::Color3uint8 {
            r: 196,
            g: 40,
            b: 28
        })
    );
}

#[test]
fn instance_new_without_a_parent_can_be_reparented_later() {
    let mut runtime = runtime();
    let output = runtime
        .run(
            r#"
            local p = Instance.new("Part")
            p.Name = "Orphan"
            -- The DOM has no separate storage for "belongs to no tree": an
            -- unparented instance is a root, and the getter reports its
            -- parent as the pseudo-`DataModel` (`game`), same as it would
            -- for any other root (see `instance.rs`'s `parent_target` doc
            -- comment). `game` itself is a fresh userdata per read, so this
            -- checks its `ClassName` rather than object identity.
            local before = p.Parent
            p.Parent = workspace
            print(before.ClassName, p.Parent == workspace, workspace.Orphan == p)
        "#,
        )
        .expect("a parentless instance must still exist and be reparentable");

    assert_eq!(output.lines(), ["DataModel true true"]);
}
