use rbx_play::launch::{ClientOptions, Launcher};
use rbx_play::sober_config::{merged, recover, split_header, SoberConfigOverlay};
use serde_json::{json, Value};

const SOBER_FILE: &str = "// !!! STOP !!!\n// Not meant to be edited by hand.\n{\n  \"touch_mode\": \"off\",\n  \"use_opengl\": false\n}\n";

fn body(text: &str) -> Value {
    serde_json::from_str(split_header(text).1).unwrap()
}

#[test]
fn the_comment_header_is_split_from_the_json() {
    let (header, rest) = split_header(SOBER_FILE);
    assert!(header.starts_with("// !!! STOP"));
    assert!(rest.starts_with('{'));
}

#[test]
fn merged_sets_keys_and_keeps_the_rest_and_the_header() {
    let text = merged(SOBER_FILE, &[("touch_mode".into(), json!("on"))]);
    assert!(text.starts_with("// !!! STOP !!!\n// Not meant"));
    let value = body(&text);
    assert_eq!(value["touch_mode"], "on");
    assert_eq!(value["use_opengl"], false);
}

#[test]
fn merged_over_garbage_keeps_only_the_changes() {
    let text = merged("not json", &[("a".into(), json!(1))]);
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap(),
        json!({"a": 1})
    );
}

#[test]
fn apply_then_restore_puts_the_exact_bytes_back() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, SOBER_FILE).unwrap();
    let mut overlay = SoberConfigOverlay::apply(&path, &[("touch_mode".into(), json!("on"))])
        .unwrap()
        .unwrap();
    assert_eq!(
        body(&std::fs::read_to_string(&path).unwrap())["touch_mode"],
        "on"
    );
    overlay.restore().unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SOBER_FILE);
    assert!(!dir.path().join("config.json.rbx-native").exists());
}

#[test]
fn dropping_the_overlay_restores() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, SOBER_FILE).unwrap();
    drop(SoberConfigOverlay::apply(&path, &[("touch_mode".into(), json!("on"))]).unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SOBER_FILE);
}

#[test]
fn no_changes_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, SOBER_FILE).unwrap();
    assert!(SoberConfigOverlay::apply(&path, &[]).unwrap().is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SOBER_FILE);
}

#[test]
fn a_missing_file_is_removed_again_on_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sober/config.json");
    let mut overlay = SoberConfigOverlay::apply(&path, &[("a".into(), json!(true))])
        .unwrap()
        .unwrap();
    assert!(path.exists());
    overlay.restore().unwrap();
    assert!(!path.exists());
}

#[test]
fn a_backup_left_by_a_crash_is_recovered_before_the_next_overlay() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(&path, "{\"touch_mode\": \"on\"}").unwrap();
    std::fs::write(dir.path().join("config.json.rbx-native"), SOBER_FILE).unwrap();
    recover(&path).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SOBER_FILE);
    assert!(!dir.path().join("config.json.rbx-native").exists());
}

#[test]
fn sober_runs_under_the_wrapper_with_flatpak_args_before_the_app_id() {
    let options = ClientOptions {
        sober_config: vec![],
        flatpak_args: vec!["--device=input".into()],
        wrapper: vec!["nsenter".into(), "-t".into(), "42".into()],
    };
    let cmd = Launcher::Sober.command_with("roblox://x", &options);
    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(cmd.get_program(), "nsenter");
    assert_eq!(
        args,
        [
            "-t",
            "42",
            "flatpak",
            "run",
            "--device=input",
            "org.vinegarhq.Sober",
            "roblox://x"
        ]
    );
}

#[test]
fn default_options_keep_the_plain_command() {
    let cmd = Launcher::Sober.command_with("roblox://x", &ClientOptions::default());
    assert_eq!(cmd.get_program(), "flatpak");
    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["run", "org.vinegarhq.Sober", "roblox://x"]);
}

#[test]
fn master_volume_reads_the_in_game_slider() {
    use rbx_play::sober_config::master_volume_in;
    let xml = r#"<Item class="UserGameSettings"><Properties>
			<float name="MasterVolume">0.100000001</float>
			<float name="MasterVolumeStudio">1</float>
		</Properties></Item>"#;
    assert_eq!(master_volume_in(xml), Some(0.1));
    assert_eq!(
        master_volume_in("<float name=\"MasterVolumeStudio\">1</float>"),
        None
    );
    assert_eq!(master_volume_in(""), None);
}

#[test]
fn a_quiet_client_gets_a_notice_and_a_loud_one_does_not() {
    use rbx_play::sober_config::quiet_volume_notice;
    let text = quiet_volume_notice(0.1).expect("10% is quiet");
    assert!(text.contains("10%"), "{text}");
    assert!(text.contains("Settings"), "{text}");
    assert_eq!(
        quiet_volume_notice(0.0).map(|t| t.contains("0%")),
        Some(true)
    );
    assert_eq!(quiet_volume_notice(0.5), None);
    assert_eq!(quiet_volume_notice(1.0), None);
}

/// Play's Sober launch adds nothing that cuts the client off from the
/// sound server: no env, no `--nosocket`, no socket or filesystem override.
#[test]
fn the_sober_launch_keeps_the_sound_socket() {
    let cmd = Launcher::Sober.command_with("roblox://placeId=1", &ClientOptions::default());
    assert_eq!(cmd.get_program(), "flatpak");
    let args: Vec<_> = cmd
        .get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    assert_eq!(args, ["run", "org.vinegarhq.Sober", "roblox://placeId=1"]);
    assert_eq!(cmd.get_envs().count(), 0);
}
