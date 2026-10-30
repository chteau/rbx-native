use rbx_reflection::ReflectionDatabase;
use serde_json::{json, Value};

use super::super::call;
use super::super::tests::{place, studio};
use crate::mcp::{DataModel, Job, PlayState, Session, Studio};
use crate::properties::Properties;

fn run(play: PlayState, tool: &str, arguments: Value) -> Result<Job, String> {
    let dom = place();
    let properties = Properties::new(ReflectionDatabase::embedded());
    let studio = Studio { play, ..studio() };
    let mut session = Session::new(&dom, &properties, &studio);
    let result = call(
        &json!({ "name": tool, "arguments": arguments }),
        &mut session,
    )
    .unwrap();
    match session.job {
        Some(job) => Ok(job),
        None => {
            assert_eq!(result["isError"], true);
            Err(result["content"][0]["text"].as_str().unwrap().to_owned())
        }
    }
}

#[test]
fn execute_luau_names_its_data_model() {
    assert_eq!(
        run(
            PlayState::Edit,
            "execute_luau",
            json!({ "code": "1 + 1", "datamodel_type": "Edit" })
        ),
        Ok(Job::Luau {
            code: "1 + 1".to_owned(),
            datamodel: DataModel::Edit
        })
    );
    assert_eq!(
        run(
            PlayState::Playing,
            "execute_luau",
            json!({ "code": "x", "datamodel_type": "Server" })
        ),
        Ok(Job::Luau {
            code: "x".to_owned(),
            datamodel: DataModel::Server
        })
    );
    assert_eq!(
        run(
            PlayState::ShowingLive,
            "execute_luau",
            json!({ "code": "x", "datamodel_type": "Edit" })
        ),
        Err("Edit datamodel is not available in Play mode".to_owned())
    );
    assert_eq!(
        run(PlayState::Edit, "execute_luau", json!({ "code": "x" })),
        Err("datamodel_type is required".to_owned())
    );
    assert_eq!(
        run(
            PlayState::Edit,
            "execute_luau",
            json!({ "code": "x", "datamodel_type": "Studio" })
        ),
        Err("Invalid datamodel_type: Studio".to_owned())
    );
    assert_eq!(
        run(
            PlayState::Starting,
            "execute_luau",
            json!({ "code": "x", "datamodel_type": "Client" })
        ),
        Err("Client datamodel is not available in Starting mode".to_owned())
    );
    assert!(run(
        PlayState::Edit,
        "execute_luau",
        json!({ "datamodel_type": "Edit" })
    )
    .is_err());
}

#[test]
fn start_stop_play_and_console_leave_their_jobs() {
    assert_eq!(
        run(
            PlayState::Edit,
            "start_stop_play",
            json!({ "is_start": true })
        ),
        Ok(Job::Play { start: true })
    );
    assert_eq!(
        run(
            PlayState::Playing,
            "start_stop_play",
            json!({ "is_start": false })
        ),
        Ok(Job::Play { start: false })
    );
    assert!(run(PlayState::Edit, "start_stop_play", json!({})).is_err());
    assert_eq!(
        run(PlayState::Edit, "get_console_output", json!({})),
        Ok(Job::Console)
    );
}

#[test]
fn screen_capture_takes_both_camera_points_or_neither() {
    assert_eq!(
        run(
            PlayState::Edit,
            "screen_capture",
            json!({ "capture_id": "ScreenCapture_1" })
        ),
        Ok(Job::Capture { camera: None })
    );
    assert_eq!(
        run(
            PlayState::Edit,
            "screen_capture",
            json!({ "camera_position": [0, 10, -5.5], "look_at_position": [1, 2, 3] })
        ),
        Ok(Job::Capture {
            camera: Some(([0.0, 10.0, -5.5], [1.0, 2.0, 3.0]))
        })
    );
    assert_eq!(
        run(
            PlayState::Edit,
            "screen_capture",
            json!({ "camera_position": [0, 0, 0] })
        ),
        Err("camera_position and look_at_position must be provided together".to_owned())
    );
    assert_eq!(
        run(
            PlayState::Edit,
            "screen_capture",
            json!({ "camera_position": [0, 0], "look_at_position": [1, 2, 3] })
        ),
        Err("camera_position and look_at_position must be 3-element arrays".to_owned())
    );
    assert_eq!(
        run(
            PlayState::Edit,
            "screen_capture",
            json!({ "camera_position": [0, "a", 0], "look_at_position": [1, 2, 3] })
        ),
        Err("camera_position and look_at_position must be numbers".to_owned())
    );
}

#[test]
fn save_place_saves_or_saves_as() {
    assert_eq!(
        run(PlayState::Edit, "save_place", json!({})),
        Ok(Job::Save { path: None })
    );
    assert_eq!(
        run(
            PlayState::Playing,
            "save_place",
            json!({ "path": " Copy.rbxlx " })
        ),
        Ok(Job::Save {
            path: Some("Copy.rbxlx".into())
        })
    );
    assert_eq!(
        run(PlayState::Edit, "save_place", json!({ "path": 3 })),
        Err("path must be a file path".to_owned())
    );
}
