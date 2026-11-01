use super::*;

const SOURCE: &str = "local function add(a: number, b: number)\n\treturn a + b\nend\n\nlocal v = Vector3.new(1, 2, 3)\nprint(add(1, 2), v)\n";

fn options() -> Options {
    Options::default()
}

#[test]
fn the_bytecode_listing_names_the_functions_and_the_source() {
    let text = dump(SOURCE, View::Bytecode, &options()).unwrap();
    assert!(text.contains("Function 0 (add)"), "{text}");
    assert!(text.contains("return a + b"), "{text}");
    assert!(text.contains("RETURN"), "{text}");
}

#[test]
fn roblox_mode_makes_vector3_new_a_constant() {
    let roblox = dump(SOURCE, View::Bytecode, &options()).unwrap();
    assert!(roblox.contains("[1, 2, 3]"), "{roblox}");
    let plain = dump(
        SOURCE,
        View::Bytecode,
        &Options {
            roblox: false,
            ..options()
        },
    )
    .unwrap();
    assert!(plain.contains("Vector3.new"), "{plain}");
    assert!(!plain.contains("[1, 2, 3]"), "{plain}");
}

#[test]
fn level_two_inlines_and_says_so() {
    let text = dump(SOURCE, View::Remarks, &options()).unwrap();
    assert!(text.contains("inlining succeeded"), "{text}");
}

#[test]
fn every_target_produces_native_code() {
    for target in Target::ALL {
        let text = dump(
            SOURCE,
            View::Assembly,
            &Options {
                target,
                ..options()
            },
        )
        .unwrap();
        assert!(!text.trim().is_empty(), "{target:?}");
    }
    let x64 = dump(
        SOURCE,
        View::Assembly,
        &Options {
            target: Target::X64SystemV,
            ..options()
        },
    )
    .unwrap();
    assert!(x64.contains("mov"), "{x64}");
    let a64 = dump(
        SOURCE,
        View::Assembly,
        &Options {
            target: Target::A64,
            ..options()
        },
    )
    .unwrap();
    assert!(a64.contains("ldr") || a64.contains("str"), "{a64}");
}

#[test]
fn the_ir_carries_the_bytecode_it_lowers() {
    let text = dump(SOURCE, View::Ir, &options()).unwrap();
    assert!(text.contains("ADD"), "{text}");
}

#[test]
fn a_syntax_error_comes_back_with_its_position() {
    let error = dump("local x = (\n", View::Bytecode, &options()).unwrap_err();
    assert!(
        error.starts_with("2:1: ") || error.starts_with("1:"),
        "{error}"
    );
}
