use std::path::Path;

fn render_fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/tests")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));
    let dom = rbx_binary::deserialize(&bytes)
        .unwrap_or_else(|err| panic!("failed to parse {path:?}: {err}"));
    let db = rbx_parser_cli::default_reflection_database();
    rbx_parser_cli::render_tree(&dom, &db)
}

#[test]
fn fps_rbxm_lists_expected_classes_and_resolved_enum() {
    let output = render_fixture("FPS.rbxm");

    assert!(output.contains("Part "));
    assert!(output.contains("Tool "));
    assert!(output.contains("ModuleScript "));
    assert!(
        output.contains("SmoothPlastic"),
        "expected a resolved Material enum name, got:\n{output}"
    );
}

// One assertion per newly decoded type: the renderer must not regress a decoded
// value back to the `Unknown(type=...)` placeholder.
#[test]
fn test_place_rbxl_renders_every_decoded_property_type() {
    let output = render_fixture("TestPlace.rbxl");

    for expected in [
        "TeamColor = BrickColor(194)",
        "Transparency = NumberSequence[0: 0 \u{b1}0, 1: 0 \u{b1}0]",
        "Color = ColorSequence[0: (1, 1, 1), 1: (1, 1, 1)]",
        "GameSettingsScaleRangeHeight = [0.9, 1.05]",
        "SliceCenter = {(0, 0), (0, 0)}",
        "WorldPivotData = pos=(0, 0, 0)",
        "UniqueId = 0000000209e550b200382dbe99a63c35",
        "FontFace = Font { family: \"rbxasset://fonts/families/BuilderSans.json\", weight: 700",
        "Capabilities = SecurityCapabilities(0x0)",
        "EmissiveMaskContent = Content(none)",
    ] {
        assert!(
            output.contains(expected),
            "missing {expected:?} in:\n{output}"
        );
    }

    // 0x01 String blobs that are not UTF-8 are the only intended survivors.
    assert_eq!(output.matches("Unknown(type=").count(), 2);
}

#[test]
fn fps_rbxm_renders_an_absent_optional_cframe() {
    let output = render_fixture("FPS.rbxm");

    assert!(output.contains("WorldPivotData = none"));
    assert!(!output.contains("Unknown(type="));
}

#[test]
fn test_place_rbxl_nests_spawn_location_under_workspace() {
    let output = render_fixture("TestPlace.rbxl");

    assert!(output.contains("Workspace "));
    assert!(output.contains("SpawnLocation "));

    let workspace_line = output
        .lines()
        .find(|l| l.contains("Workspace \""))
        .expect("Workspace instance present");
    let spawn_line = output
        .lines()
        .find(|l| l.contains("SpawnLocation \""))
        .expect("SpawnLocation instance present");

    let workspace_indent = workspace_line.len() - workspace_line.trim_start().len();
    let spawn_indent = spawn_line.len() - spawn_line.trim_start().len();
    assert!(
        spawn_indent > workspace_indent,
        "expected SpawnLocation to be indented deeper than Workspace (descendant), got:\n{output}"
    );
}

// Golden dumps: each fixture's full rendering, checked in under
// `assets/tests/dumps/`. The API dump only reaches this output through enum
// names, so the nightly API-dump sync reruns this against Studio's newest dump
// before committing it — a renamed or dropped enum item fails here rather than
// in a user's place. `UPDATE_DUMPS=1 cargo test -p rbx_parser_cli` rewrites
// them after an intended change.
#[test]
fn fixtures_render_as_their_reference_dumps() {
    let dumps = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/tests/dumps");
    let update = std::env::var_os("UPDATE_DUMPS").is_some();
    let mut drifted = Vec::new();

    for fixture in ["FPS.rbxm", "TestPlace.rbxl", "DEMO_LIGHTING_MATERIALS.rbxl"] {
        let actual = render_fixture(fixture);
        let path = dumps.join(format!("{fixture}.txt"));
        if update {
            std::fs::write(&path, &actual).unwrap();
            continue;
        }
        // A Windows checkout may have turned the reference's newlines into CRLF.
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"))
            .replace("\r\n", "\n");
        if let Some((line, (want, got))) = expected
            .lines()
            .zip(actual.lines())
            .enumerate()
            .find(|(_, (want, got))| want != got)
        {
            drifted.push(format!(
                "{fixture}:{}\n  reference: {want}\n  now:       {got}",
                line + 1
            ));
        } else if expected.lines().count() != actual.lines().count() {
            drifted.push(format!(
                "{fixture}: {} lines in the reference, {} now",
                expected.lines().count(),
                actual.lines().count()
            ));
        }
    }

    assert!(
        drifted.is_empty(),
        "parsed output drifted from assets/tests/dumps (first difference per file):\n{}",
        drifted.join("\n")
    );
}
