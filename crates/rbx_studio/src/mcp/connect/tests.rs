use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::desktop::{self, Change, Dirs};
use super::{probe, quote, Client, Launch, Os};

fn plain() -> Launch {
    Launch::of(Path::new("/opt/RbxNative/rbxstudio"), false, None)
}

/// A path with `\` turned into `/`, so a Windows path built on any host
/// compares as one string.
fn slashed(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[test]
fn the_launch_runs_the_executable_or_flatpak_with_the_bridge_flag() {
    assert_eq!(plain().command, "/opt/RbxNative/rbxstudio");
    assert_eq!(plain().args, ["--mcp"]);
    let flatpak = Launch::of(Path::new("/app/bin/rbxstudio"), true, None);
    assert_eq!(flatpak.command, "flatpak");
    assert_eq!(
        flatpak.args,
        ["run", "io.github.chteau.rbx_native.Editor", "--mcp"]
    );
    assert_eq!(
        flatpak.probe_target(),
        std::env::current_exe().unwrap(),
        "inside the sandbox the test runs the bridge directly"
    );
}

#[test]
fn a_custom_config_folder_rides_along_so_the_bridge_finds_the_editor() {
    let launch = Launch::of(Path::new("/x/rbxstudio"), false, Some("/cfg".to_owned()));
    assert_eq!(
        launch.entry(),
        json!({ "command": "/x/rbxstudio", "args": ["--mcp"], "env": { "XDG_CONFIG_HOME": "/cfg" } })
    );
    assert_eq!(
        plain().entry(),
        json!({ "command": "/opt/RbxNative/rbxstudio", "args": ["--mcp"] }),
        "no env block when nothing needs passing"
    );
    let command = Client::ClaudeCode
        .setup(&launch, Os::Linux, Path::new("/h"))
        .snippet;
    assert_eq!(
        command,
        "claude mcp add --scope user -e XDG_CONFIG_HOME=/cfg rbx-native -- /x/rbxstudio --mcp"
    );
}

#[test]
fn shell_words_are_quoted_for_the_os_they_are_pasted_into() {
    assert_eq!(quote("/usr/bin/rbxstudio", Os::Linux), "/usr/bin/rbxstudio");
    assert_eq!(
        quote("/Users/me/My Apps/x", Os::Mac),
        "'/Users/me/My Apps/x'"
    );
    assert_eq!(quote("/it's/x", Os::Linux), r"'/it'\''s/x'");
    assert_eq!(
        quote(r"C:\Program Files\RbxNative\rbxstudio.exe", Os::Windows),
        r#""C:\Program Files\RbxNative\rbxstudio.exe""#
    );
    assert_eq!(quote("", Os::Linux), "''");
    let windows = Launch::of(Path::new(r"C:\Apps\RbxNative\rbxstudio.exe"), false, None);
    assert_eq!(
        Client::ClaudeCode
            .setup(&windows, Os::Windows, Path::new(r"C:\Users\me"))
            .snippet,
        r#"claude mcp add --scope user rbx-native -- "C:\Apps\RbxNative\rbxstudio.exe" --mcp"#
    );
}

#[test]
fn each_client_gets_its_own_shape() {
    let home = Path::new("/home/me");
    let desktop: Value = serde_json::from_str(
        &Client::ClaudeDesktop
            .setup(&plain(), Os::Linux, home)
            .snippet,
    )
    .unwrap();
    assert_eq!(
        desktop["mcpServers"]["rbx-native"]["args"],
        json!(["--mcp"])
    );
    let cursor = Client::Cursor.setup(&plain(), Os::Linux, home);
    assert_eq!(
        cursor.file,
        Some(PathBuf::from("/home/me/.cursor/mcp.json"))
    );
    let vscode: Value =
        serde_json::from_str(&Client::VsCode.setup(&plain(), Os::Linux, home).snippet).unwrap();
    assert_eq!(vscode["servers"]["rbx-native"]["type"], "stdio");
    assert_eq!(
        vscode["servers"]["rbx-native"]["command"],
        "/opt/RbxNative/rbxstudio"
    );
    let other = Client::Other.setup(&plain(), Os::Linux, home).snippet;
    assert!(other.contains("Transport: stdio") && other.contains("Arguments: --mcp"));
}

#[test]
fn claude_desktop_config_lives_where_each_os_keeps_it() {
    let dirs = |home: &str| Dirs {
        home: PathBuf::from(home),
        ..Dirs::default()
    };
    assert_eq!(
        slashed(&desktop::config_path(Os::Mac, &dirs("/Users/me"))),
        "/Users/me/Library/Application Support/Claude/claude_desktop_config.json"
    );
    assert_eq!(
        slashed(&desktop::config_path(Os::Linux, &dirs("/home/me"))),
        "/home/me/.config/Claude/claude_desktop_config.json"
    );
    let xdg = Dirs {
        config_home: Some(PathBuf::from("/xdg")),
        ..dirs("/home/me")
    };
    assert_eq!(
        slashed(&desktop::config_path(Os::Linux, &xdg)),
        "/xdg/Claude/claude_desktop_config.json"
    );
    let windows = Dirs {
        appdata: Some(PathBuf::from(r"C:\Users\me\AppData\Roaming")),
        ..dirs(r"C:\Users\me")
    };
    assert_eq!(
        slashed(&desktop::config_path(Os::Windows, &windows)),
        "C:/Users/me/AppData/Roaming/Claude/claude_desktop_config.json"
    );
    assert_eq!(
        slashed(&desktop::config_path(Os::Windows, &dirs(r"C:\Users\me"))),
        "C:/Users/me/AppData/Roaming/Claude/claude_desktop_config.json",
        "without %APPDATA% the roaming folder under the profile"
    );
    let store = Dirs {
        store_package: Some(PathBuf::from(
            r"C:\Users\me\AppData\Local\Packages\Claude_pzs8sxrjxfjjc",
        )),
        ..windows.clone()
    };
    assert_eq!(
        slashed(&desktop::config_path(Os::Windows, &store)),
        "C:/Users/me/AppData/Local/Packages/Claude_pzs8sxrjxfjjc/LocalCache/Roaming/Claude/claude_desktop_config.json"
    );
    let overridden = Dirs {
        overridden: Some(PathBuf::from("/tmp/t.json")),
        ..windows
    };
    assert_eq!(
        desktop::config_path(Os::Windows, &overridden),
        PathBuf::from("/tmp/t.json")
    );
}

#[test]
fn merging_keeps_every_other_key_and_server_in_order() {
    let existing = r#"{
  "zeta": 1,
  "mcpServers": { "files": { "command": "npx", "args": ["fs"] } },
  "alpha": { "theme": "dark" }
}"#;
    let entry = plain().entry();
    let (text, change, others) = desktop::merged(Some(existing), &entry).unwrap();
    assert_eq!((change, others), (Change::Added, 1));
    let merged: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(merged["mcpServers"]["files"]["command"], "npx");
    assert_eq!(merged["mcpServers"]["rbx-native"], entry);
    assert_eq!(merged["alpha"]["theme"], "dark");
    let zeta = text.find("\"zeta\"").unwrap();
    let alpha = text.find("\"alpha\"").unwrap();
    assert!(zeta < alpha, "keys stay in the file's order:\n{text}");

    let (again, change, _) = desktop::merged(Some(&text), &entry).unwrap();
    assert_eq!((again, change), (text.clone(), Change::Unchanged));

    let moved = Launch::of(Path::new("/new/rbxstudio"), false, None).entry();
    let (_, change, _) = desktop::merged(Some(&text), &moved).unwrap();
    assert_eq!(change, Change::Replaced(entry));
}

#[test]
fn a_missing_or_empty_file_starts_fresh_and_a_broken_one_is_refused() {
    let entry = plain().entry();
    for start in [None, Some(""), Some("  \n")] {
        let (text, change, others) = desktop::merged(start, &entry).unwrap();
        assert_eq!((change, others), (Change::Added, 0));
        assert!(text.contains("\"mcpServers\""));
    }
    assert!(desktop::merged(Some("{ not json"), &entry).is_err());
    assert!(desktop::merged(Some("[1, 2]"), &entry).is_err());
    assert!(desktop::merged(Some(r#"{"mcpServers": []}"#), &entry).is_err());
}

fn scratch(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("rbx-native-connect-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn installing_backs_the_file_up_first_and_never_twice_for_nothing() {
    let dir = scratch("install");
    let path = dir.join("claude_desktop_config.json");
    let before = r#"{"mcpServers":{"files":{"command":"npx"}}}"#;
    std::fs::write(&path, before).unwrap();
    let entry = plain().entry();

    let installed = desktop::install(&path, &entry, 42).unwrap();
    assert_eq!(
        (installed.change.clone(), installed.others),
        (Change::Added, 1)
    );
    let backup = installed.backup.unwrap();
    assert_eq!(backup, dir.join("claude_desktop_config.json.42.bak"));
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), before);
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(after["mcpServers"]["files"]["command"], "npx");
    assert_eq!(after["mcpServers"]["rbx-native"], entry);

    let again = desktop::install(&path, &entry, 43).unwrap();
    assert_eq!((again.change, again.backup), (Change::Unchanged, None));
    assert!(!dir.join("claude_desktop_config.json.43.bak").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn installing_creates_the_file_but_not_claude_s_folder() {
    let dir = scratch("fresh");
    let path = dir.join("claude_desktop_config.json");
    let installed = desktop::install(&path, &plain().entry(), 1).unwrap();
    assert_eq!((installed.change, installed.backup), (Change::Added, None));
    assert!(path.is_file());

    let missing = dir.join("no-claude").join("claude_desktop_config.json");
    let error = desktop::install(&missing, &plain().entry(), 1).unwrap_err();
    assert!(error.contains("Install Claude Desktop"), "{error}");

    let broken = dir.join("broken.json");
    std::fs::write(&broken, "{ oops").unwrap();
    assert!(desktop::install(&broken, &plain().entry(), 1).is_err());
    assert_eq!(std::fs::read_to_string(&broken).unwrap(), "{ oops");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_probe_reads_tools_and_editors_off_the_bridge_answers() {
    let hello =
        json!({ "jsonrpc": "2.0", "id": 1, "result": { "serverInfo": { "name": "rbx-native" } } });
    let tools = json!({ "jsonrpc": "2.0", "id": 2, "result": { "tools": [{}, {}, {}] } });
    let listed = json!({ "studios": [{ "id": "ab12", "name": "Place.rbxl" }] }).to_string();
    let studios = json!({ "jsonrpc": "2.0", "id": 3,
        "result": { "content": [{ "type": "text", "text": listed }] } });
    assert_eq!(
        probe::read(&hello, &tools, &studios).unwrap(),
        probe::Probe {
            tools: 3,
            studios: vec!["ab12".to_owned()]
        }
    );
    let failed = json!({ "jsonrpc": "2.0", "id": 2, "error": { "code": -1, "message": "nope" } });
    assert!(probe::read(&hello, &failed, &studios)
        .unwrap_err()
        .contains("nope"));
    let stranger = json!({ "result": { "serverInfo": { "name": "other" } } });
    assert!(probe::read(&stranger, &tools, &studios).is_err());
}
