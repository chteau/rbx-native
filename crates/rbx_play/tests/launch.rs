use rbx_play::launch::{join_server_uri, join_uri, Launcher};

fn args_of(cmd: &std::process::Command) -> Vec<String> {
    cmd.get_args()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn join_uri_omits_empty_launch_data() {
    assert_eq!(join_uri(123, ""), "roblox://placeId=123");
}

#[test]
fn join_uri_encodes_launch_data() {
    assert_eq!(
        join_uri(123, "a b&c"),
        "roblox://placeId=123&launchData=a%20b%26c"
    );
}

#[test]
fn join_uri_keeps_unreserved_characters() {
    assert_eq!(
        join_uri(1, "a-b_c.d~E9"),
        "roblox://placeId=1&launchData=a-b_c.d~E9"
    );
}

#[test]
fn join_server_uri_encodes_job_id() {
    assert_eq!(
        join_server_uri(42, "a b"),
        "roblox://experiences/start?placeId=42&gameInstanceId=a%20b"
    );
}

#[test]
fn xdg_open_and_mac_open_never_report_a_running_client() {
    assert!(!Launcher::XdgOpen.client_running());
    assert!(!Launcher::MacOpen.client_running());
}

#[test]
fn xdg_open_and_mac_open_close_is_a_no_op() {
    assert!(Launcher::XdgOpen.close_running_client().is_ok());
    assert!(Launcher::MacOpen.close_running_client().is_ok());
}

#[test]
fn sober_command_runs_flatpak() {
    let uri = "roblox://placeId=1";
    let cmd = Launcher::Sober.command(uri);
    assert_eq!(cmd.get_program().to_string_lossy(), "flatpak");
    assert_eq!(args_of(&cmd), ["run", "org.vinegarhq.Sober", uri]);
}

#[test]
fn xdg_open_command_passes_uri() {
    let uri = "roblox://placeId=1";
    let cmd = Launcher::XdgOpen.command(uri);
    assert_eq!(cmd.get_program().to_string_lossy(), "xdg-open");
    assert_eq!(args_of(&cmd), [uri]);
}

#[test]
fn only_sober_tracks_client() {
    assert!(Launcher::Sober.tracks_client());
    assert!(!Launcher::XdgOpen.tracks_client());
    assert!(!Launcher::WindowsShell.tracks_client());
    assert!(!Launcher::MacOpen.tracks_client());
}
