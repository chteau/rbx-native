use std::collections::VecDeque;
use std::time::Duration;

use rbx_play::keycode::KeyCode;
use rbx_play::protocol::{CommandBody, Located};
use serde_json::{json, Value};

use super::run::{self, window_point, Button, Client};
use super::{KEYBOARD, MOUSE, NAVIGATION};
use crate::mcp::tools::list;
use crate::mcp::tools::tests::{place, run as call_tool};

/// What the steps did, in order.
#[derive(Debug, Clone, PartialEq)]
enum Event {
    Ask(CommandBody),
    Move(i32, i32),
    Button(Button, bool),
    Scroll(bool),
    Key(&'static str, bool),
    TypeKeys(String),
    Sleep(Duration),
}

/// A client whose probe answers from a script, in a 1000x800 window
/// showing a 1000x800 viewport under a 58 px inset.
struct Recorder {
    answers: VecDeque<Result<String, String>>,
    events: Vec<Event>,
}

const SCREEN: &str = r#"{"inset_x":0,"inset_y":58,"width":1000,"height":800}"#;

fn button_at(x: f64, y: f64) -> String {
    format!(r#"{{"x":{x},"y":{y},"inset_x":0,"inset_y":58,"width":1000,"height":800}}"#)
}

impl Recorder {
    fn new(answers: &[Result<&str, &str>]) -> Self {
        Recorder {
            answers: answers
                .iter()
                .map(|a| a.map(str::to_owned).map_err(str::to_owned))
                .collect(),
            events: Vec::new(),
        }
    }
}

impl Client for Recorder {
    fn ask(&mut self, body: CommandBody, _: Duration) -> Result<String, String> {
        self.events.push(Event::Ask(body));
        self.answers.pop_front().expect("a scripted answer")
    }
    fn size(&mut self) -> Result<(u32, u32), String> {
        Ok((1000, 800))
    }
    fn move_to(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.events.push(Event::Move(x, y));
        Ok(())
    }
    fn button(&mut self, button: Button, down: bool) -> Result<(), String> {
        self.events.push(Event::Button(button, down));
        Ok(())
    }
    fn scroll(&mut self, up: bool) -> Result<(), String> {
        self.events.push(Event::Scroll(up));
        Ok(())
    }
    fn key(&mut self, key: &'static KeyCode, down: bool) -> Result<(), String> {
        self.events.push(Event::Key(key.name, down));
        Ok(())
    }
    fn type_keys(&mut self, text: &str) -> Result<(), String> {
        self.events.push(Event::TypeKeys(text.to_owned()));
        Ok(())
    }
    fn sleep(&mut self, duration: Duration) {
        self.events.push(Event::Sleep(duration));
    }
}

fn steps(value: Value) -> Vec<Value> {
    value.as_array().unwrap().clone()
}

fn locate(path: &str) -> Event {
    Event::Ask(CommandBody::Locate {
        path: Some(path.to_owned()),
    })
}

fn presses(events: &[Event]) -> Vec<Event> {
    events
        .iter()
        .filter(|e| !matches!(e, Event::Sleep(_)))
        .cloned()
        .collect()
}

#[test]
fn a_gui_object_is_clicked_at_its_centre_past_the_inset() {
    let button = button_at(200.0, 130.0);
    let mut client = Recorder::new(&[Ok(&button)]);
    let path = "LocalPlayer.PlayerGui.Menu.Play";
    let answer = run::mouse(
        &mut client,
        &steps(json!([
            { "action": "moveTo", "instance_path": path },
            { "action": "wait", "wait_time_ms": 150 },
            { "action": "mouseButtonClick", "mouse_button": "left" },
        ])),
        &mut None,
    );
    assert_eq!(answer, Ok("Success".to_owned()));
    assert_eq!(
        presses(&client.events),
        vec![
            locate(path),
            Event::Move(200, 188),
            Event::Button(Button::Left, true),
            Event::Button(Button::Left, false),
        ]
    );
    assert_eq!(client.events[2], Event::Sleep(Duration::from_millis(150)));
}

#[test]
fn coordinates_are_reused_and_asked_about_once() {
    let mut client = Recorder::new(&[Ok(SCREEN)]);
    let answer = run::mouse(
        &mut client,
        &steps(json!([
            { "action": "mouseButtonDown", "mouse_button": "right", "x": 600, "y": 400 },
            { "action": "mouseButtonUp", "mouse_button": "right" },
            { "action": "scrollDown" },
            { "action": "scrollUp", "x": 10, "y": 20 },
        ])),
        &mut None,
    );
    assert_eq!(answer, Ok("Success".to_owned()));
    assert_eq!(
        presses(&client.events),
        vec![
            Event::Ask(CommandBody::Locate { path: None }),
            Event::Move(600, 458),
            Event::Button(Button::Right, true),
            Event::Button(Button::Right, false),
            Event::Scroll(false),
            Event::Move(10, 78),
            Event::Scroll(true),
        ]
    );
}

#[test]
fn the_pointer_is_kept_from_one_call_to_the_next() {
    let mut last = None;
    let mut first = Recorder::new(&[Ok(SCREEN)]);
    let moved = run::mouse(
        &mut first,
        &steps(json!([{ "action": "moveTo", "x": 300, "y": 200 }])),
        &mut last,
    );
    assert_eq!(moved, Ok("Success".to_owned()));
    assert_eq!(last, Some((300.0, 200.0)));
    // The next call clicks there, putting the pointer back first.
    let mut second = Recorder::new(&[Ok(SCREEN)]);
    let clicked = run::mouse(
        &mut second,
        &steps(json!([
            { "action": "mouseButtonClick", "mouse_button": "left" },
            { "action": "scrollUp" },
        ])),
        &mut last,
    );
    assert_eq!(clicked, Ok("Success".to_owned()));
    assert_eq!(
        presses(&second.events),
        vec![
            Event::Ask(CommandBody::Locate { path: None }),
            Event::Move(300, 258),
            Event::Button(Button::Left, true),
            Event::Button(Button::Left, false),
            Event::Scroll(true),
        ]
    );
}

#[test]
fn mouse_steps_fail_in_studios_words_after_the_steps_before_them() {
    let fail = |value: Value, answers: &[Result<&str, &str>]| {
        let mut client = Recorder::new(answers);
        let answer = run::mouse(&mut client, &steps(value), &mut None);
        (answer.unwrap_err(), presses(&client.events))
    };
    assert_eq!(
        fail(json!([{ "action": "mouseButtonClick" }]), &[]),
        (
            "Either x and y, instance_path, or a prior action that establishes mouse position \
             is required"
                .to_owned(),
            vec![]
        )
    );
    // The move happened; the click had no button.
    let (error, events) = fail(
        json!([
            { "action": "moveTo", "x": 5, "y": 5 },
            { "action": "mouseButtonClick", "mouse_button": "middle" },
        ]),
        &[Ok(SCREEN)],
    );
    assert_eq!(
        error,
        "mouse_button (\"left\" or \"right\") is required for mouseButtonClick"
    );
    assert_eq!(events.last(), Some(&Event::Move(5, 63)));
    assert_eq!(
        fail(json!([{ "action": "wait" }]), &[]).0,
        "wait_time_ms is required for wait action"
    );
    assert_eq!(
        fail(json!([{ "action": "wait", "wait_time_ms": 20000 }]), &[]).0,
        "wait_time_ms must be between 0 and 10000"
    );
    assert_eq!(
        fail(
            json!([{ "action": "moveTo", "instance_path": "Workspace.Door" }]),
            &[Err("Instance is not a GuiObject: Part")]
        )
        .0,
        "Instance is not a GuiObject: Part"
    );
    assert_eq!(
        run::mouse(&mut Recorder::new(&[]), &[], &mut None),
        Ok("Success".to_owned())
    );
}

#[test]
fn keys_are_pressed_and_held_and_instance_paths_ignored() {
    let mut client = Recorder::new(&[]);
    let answer = run::keyboard(
        &mut client,
        &steps(json!([
            { "action": "keyPress", "key_code": "Return", "instance_path": "Workspace.Nope" },
            { "action": "keyDown", "key_code": "W" },
            { "action": "wait", "wait_time_ms": 1000 },
            { "action": "keyUp", "key_code": "W" },
            { "action": "hop" },
        ])),
    );
    assert_eq!(answer, Ok("Success".to_owned()));
    assert_eq!(
        client.events,
        vec![
            Event::Key("Return", true),
            Event::Key("Return", false),
            Event::Key("W", true),
            Event::Sleep(Duration::from_secs(1)),
            Event::Key("W", false),
        ]
    );
}

#[test]
fn key_failures_read_as_studios() {
    let fail =
        |step: Value| run::keyboard(&mut Recorder::new(&[]), &steps(json!([step]))).unwrap_err();
    assert_eq!(
        fail(json!({ "action": "keyPress" })),
        "Argument 2 missing or nil"
    );
    assert_eq!(
        fail(json!({ "action": "keyPress", "key_code": "Banana" })),
        "Unable to cast string to KeyCode"
    );
    assert_eq!(
        fail(json!({ "action": "textInput" })),
        "Argument 1 missing or nil"
    );
    assert!(fail(json!({ "action": "keyPress", "key_code": "ButtonA" }))
        .starts_with("KeyCode.ButtonA has no key on this keyboard"));
}

#[test]
fn text_goes_to_the_focused_box_after_clicking_its_target() {
    let field = button_at(250.0, 225.0);
    let mut client = Recorder::new(&[Ok(&field), Ok("typed"), Ok("chat")]);
    let path = "LocalPlayer.PlayerGui.Menu.Name";
    let answer = run::keyboard(
        &mut client,
        &steps(json!([
            { "action": "textInput", "text_inputs": "Hi", "instance_path": path },
            { "action": "textInput", "text_inputs": "gg" },
        ])),
    );
    assert_eq!(answer, Ok("Success".to_owned()));
    assert_eq!(
        presses(&client.events),
        vec![
            locate(path),
            Event::Move(250, 283),
            Event::Button(Button::Left, true),
            Event::Button(Button::Left, false),
            Event::Ask(CommandBody::TypeText {
                text: "Hi".to_owned(),
                path: Some(path.to_owned()),
            }),
            Event::Ask(CommandBody::TypeText {
                text: "gg".to_owned(),
                path: None,
            }),
            // The chat bar had the focus: typed as keys.
            Event::TypeKeys("gg".to_owned()),
        ]
    );
}

#[test]
fn navigation_asks_the_client_to_walk_at_a_bounded_speed() {
    let mut client = Recorder::new(&[
        Ok("Success"),
        Err("Can not find a route to the destination"),
    ]);
    let walk = |client: &mut Recorder, arguments: Value| run::navigate(client, &arguments);
    assert_eq!(
        walk(
            &mut client,
            json!({ "instance_path": "Workspace.Door", "speed_multiplier": 50 })
        ),
        Ok("Success".to_owned())
    );
    assert_eq!(
        walk(
            &mut client,
            json!({ "x": 5000, "z": 5000, "speed_multiplier": 0.01 })
        ),
        Err("Can not find a route to the destination".to_owned())
    );
    assert_eq!(
        client.events,
        vec![
            Event::Ask(CommandBody::Navigate {
                path: Some("Workspace.Door".to_owned()),
                x: 0.0,
                y: 0.0,
                z: 0.0,
                speed: 10.0,
            }),
            Event::Ask(CommandBody::Navigate {
                path: None,
                x: 5000.0,
                y: 0.0,
                z: 5000.0,
                speed: 0.1,
            }),
        ]
    );
}

#[test]
fn gui_points_scale_into_a_window_drawn_at_another_size() {
    let screen = Located {
        x: None,
        y: None,
        inset_x: 0.0,
        inset_y: 58.0,
        width: 1000.0,
        height: 800.0,
    };
    assert_eq!(
        window_point(&screen, (1000, 800), (200.0, 130.0)),
        (200, 188)
    );
    assert_eq!(
        window_point(&screen, (2000, 1600), (200.0, 130.0)),
        (400, 376)
    );
    assert_eq!(
        window_point(&screen, (1000, 800), (-50.0, 99999.0)),
        (-50, 100057)
    );
}

#[test]
fn outside_play_the_tools_answer_as_studio_does_in_edit() {
    let dom = place();
    for tool in [NAVIGATION, KEYBOARD, MOUSE] {
        assert_eq!(
            call_tool(
                &dom,
                tool,
                json!({ "datamodel_type": "Client", "actions": [] })
            ),
            (
                true,
                "Client datamodel is not available in Edit mode".to_owned()
            )
        );
    }
    assert_eq!(
        call_tool(&dom, MOUSE, json!({ "datamodel_type": "Client" })),
        (true, "Missing required argument: actions".to_owned())
    );
    assert_eq!(
        call_tool(&dom, KEYBOARD, json!({ "actions": [] })),
        (true, "datamodel_type is required".to_owned())
    );
}

#[test]
fn the_tools_are_listed_with_studios_arguments() {
    let tools = list();
    let schema = |name: &str| {
        tools
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == name)
            .unwrap_or_else(|| panic!("{name} is listed"))["inputSchema"]
            .clone()
    };
    let keyboard = schema(KEYBOARD);
    let step = &keyboard["properties"]["actions"]["items"]["properties"];
    let listed: Vec<&str> = step["key_code"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(listed, rbx_play::keycode::names().collect::<Vec<_>>());
}
