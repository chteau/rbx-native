use rbx_dom::{Variant, WeakDom};
use rbx_reflection::ReflectionDatabase;
use serde_json::{json, Value};

use super::{call, finished, resolve};
use crate::mcp::{PlayState, Session, Studio};
use crate::properties::Properties;

pub(super) fn studio() -> Studio {
    Studio {
        id: "a1".to_owned(),
        name: "Arena.rbxl".to_owned(),
        place_id: None,
        play: PlayState::Edit,
    }
}

pub(super) fn place() -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = dom.new_instance("Workspace", "Workspace", None);
    let model = dom.new_instance("Model", "Tower", Some(workspace));
    let part = dom.new_instance("Part", "Door", Some(model));
    dom.set_property(part, "Anchored", Variant::Bool(true))
        .unwrap();
    dom.new_instance("Folder", "Config.v2", Some(workspace));
    let service = dom.new_instance("ServerScriptService", "ServerScriptService", None);
    let main = dom.new_instance("Script", "GameLoop", Some(service));
    dom.set_property(
        main,
        "Source",
        Variant::String("local rounds = 0\nprint(\"Round start\")\nrounds += 1\n".to_owned()),
    )
    .unwrap();
    let module = dom.new_instance("ModuleScript", "RoundConfig", Some(service));
    dom.set_property(
        module,
        "Source",
        Variant::String("return { length = 60 } -- round length".to_owned()),
    )
    .unwrap();
    dom.new_instance("LocalScript", "Empty", Some(service));
    dom
}

pub(super) fn run(dom: &WeakDom, tool: &str, arguments: Value) -> (bool, String) {
    let properties = Properties::new(ReflectionDatabase::embedded());
    let studio = studio();
    let mut session = Session::new(dom, &properties, &studio);
    let mut result = call(
        &json!({ "name": tool, "arguments": arguments }),
        &mut session,
    )
    .unwrap();
    if let Some(work) = session.deferred.take() {
        result = finished(work());
    }
    (
        result["isError"].as_bool().unwrap(),
        result["content"][0]["text"].as_str().unwrap().to_owned(),
    )
}

pub(super) fn json_of(dom: &WeakDom, tool: &str, arguments: Value) -> Value {
    let (failed, text) = run(dom, tool, arguments);
    assert!(!failed, "{tool}: {text}");
    serde_json::from_str(&text).unwrap()
}

#[test]
fn paths_resolve_with_or_without_game_and_through_dotted_names() {
    let dom = place();
    let door = resolve(&dom, "game.Workspace.Tower.Door").unwrap();
    assert_eq!(resolve(&dom, "Workspace.Tower.Door"), Some(door));
    assert_eq!(resolve(&dom, "Game.Workspace.Tower.Door"), Some(door));
    // Printed as Studio's tools print paths: no `game.` in front.
    assert_eq!(super::path_of(&dom, door), "Workspace.Tower.Door");
    let folder = resolve(&dom, "game.Workspace.Config.v2").unwrap();
    assert_eq!(dom.get(folder).unwrap().name(), "Config.v2");
    assert_eq!(resolve(&dom, "game.Workspace.Tower.Window"), None);
    assert_eq!(resolve(&dom, "game"), None);
}

#[test]
fn get_studio_state_answers_in_studios_three_lines() {
    let dom = place();
    assert_eq!(
        run(&dom, "get_studio_state", json!({})),
        (
            false,
            "- Current Studio Mode: Edit\n- Available DataModels: Edit\n- Focused DataModel in \
             the viewport: Edit"
                .to_owned()
        )
    );
}

#[test]
fn the_list_holds_the_shell_tools_with_their_required_arguments() {
    let tools = super::list();
    let required = |name: &str| -> Vec<String> {
        let tool = tools
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} is not listed"));
        serde_json::from_value(tool["inputSchema"]["required"].clone()).unwrap()
    };
    assert_eq!(
        required("execute_luau"),
        ["code", "datamodel_type", "studio_id"]
    );
    assert_eq!(required("start_stop_play"), ["is_start", "studio_id"]);
    assert_eq!(required("get_console_output"), ["studio_id"]);
    assert_eq!(required("screen_capture"), ["capture_id", "studio_id"]);
    assert_eq!(required("save_place"), ["studio_id"]);
}

#[test]
fn get_studio_state_lists_the_live_data_models_during_play() {
    let studio = Studio {
        play: PlayState::Playing,
        ..studio()
    };
    assert_eq!(
        super::studio::state(&studio),
        "- Current Studio Mode: Play\n- Available DataModels: Edit, Client, Server\n- Focused \
         DataModel in the viewport: Client"
    );
    let showing = Studio {
        play: PlayState::ShowingLive,
        ..studio
    };
    assert_eq!(
        super::studio::state(&showing),
        "- Current Studio Mode: Play\n- Available DataModels: Client, Server\n- Focused \
         DataModel in the viewport: Client"
    );
}

#[test]
fn list_roblox_studios_names_this_editor_with_its_place_id_when_linked() {
    let dom = place();
    assert_eq!(
        json_of(&dom, "list_roblox_studios", json!({})),
        json!({ "studios": [{ "id": "a1", "name": "Arena.rbxl" }] })
    );
    let linked = Studio {
        place_id: Some(42),
        ..studio()
    };
    assert_eq!(
        super::studio::describe(&linked),
        json!({ "id": "a1", "name": "Arena.rbxl (place 42)" })
    );
}

#[test]
fn a_call_for_another_studio_is_refused_by_this_one() {
    let dom = place();
    let (failed, text) = run(
        &dom,
        "script_read",
        json!({ "target_file": "game.ServerScriptService.GameLoop", "studio_id": "b2" }),
    );
    assert!(failed);
    assert!(text.contains("a1") && text.contains("b2"), "{text}");
    let (failed, _) = run(
        &dom,
        "script_read",
        json!({ "target_file": "game.ServerScriptService.GameLoop", "studio_id": "a1" }),
    );
    assert!(!failed);
}
