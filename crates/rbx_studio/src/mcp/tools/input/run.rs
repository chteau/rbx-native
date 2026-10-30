//! Running the input tools' steps against the client, in Studio's order:
//! each step is checked when its turn comes, so the steps before a bad one
//! have already happened, as in Studio.

use std::time::Duration;

use rbx_play::channel::RequestError;
use rbx_play::keycode::{self, KeyCode};
use rbx_play::protocol::{CommandBody, Located};
use serde_json::Value;

use super::MAX_WAIT_MS;
use crate::mcp::PlayInput;

/// What Studio answers every input call that went through.
const SUCCESS: &str = "Success";

/// A probe question (where something is, typing into a box).
const ASK_TIMEOUT: Duration = Duration::from_secs(15);
/// A walk: the probe answers when the character arrives, and each of its
/// waypoints gives up after 16 s, so this is only for a client gone silent.
const WALK_TIMEOUT: Duration = Duration::from_secs(600);

/// Studio's walk speed bounds.
const MIN_SPEED: f64 = 0.1;
const MAX_SPEED: f64 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Button {
    Left,
    Right,
}

/// The client as the steps reach it: its probe, and real input into its
/// window. A trait so the steps can be checked against a recording stand-in.
pub(super) trait Client {
    fn ask(&mut self, body: CommandBody, timeout: Duration) -> Result<String, String>;
    /// The window's size in pixels.
    fn size(&mut self) -> Result<(u32, u32), String>;
    fn move_to(&mut self, x: i32, y: i32) -> Result<(), String>;
    fn button(&mut self, button: Button, down: bool) -> Result<(), String>;
    fn scroll(&mut self, up: bool) -> Result<(), String>;
    fn key(&mut self, key: &'static KeyCode, down: bool) -> Result<(), String>;
    /// Types `text` as key presses (the chat bar, which no script reaches).
    fn type_keys(&mut self, text: &str) -> Result<(), String>;
    fn sleep(&mut self, duration: Duration);
}

pub(super) fn navigate(client: &mut dyn Client, arguments: &Value) -> Result<String, String> {
    let coordinate = |name: &str| arguments.get(name).and_then(Value::as_f64).unwrap_or(0.0);
    let speed = arguments
        .get("speed_multiplier")
        .and_then(Value::as_f64)
        .unwrap_or(1.0)
        .clamp(MIN_SPEED, MAX_SPEED);
    let body = CommandBody::Navigate {
        path: text(arguments, "instance_path").map(str::to_owned),
        x: coordinate("x"),
        y: coordinate("y"),
        z: coordinate("z"),
        speed,
    };
    client.ask(body, WALK_TIMEOUT)?;
    Ok(SUCCESS.to_owned())
}

/// `last` is where the pointer was left, in GUI coordinates: Studio keeps
/// it between calls, so a call may start with a step that has no position.
/// The pointer is put back there first, since the desktop's own mouse may
/// have moved it since.
pub(super) fn mouse(
    client: &mut dyn Client,
    steps: &[Value],
    last: &mut Option<(f64, f64)>,
) -> Result<String, String> {
    let mut screen = Screen::default();
    let mut placed = false;
    for step in steps {
        let action = text(step, "action").unwrap_or_default();
        if action != "wait" {
            let at = match aim(client, &mut screen, step)? {
                Some(at) => Some(at),
                None if !placed => *last,
                None => None,
            };
            if let Some(at) = at {
                let (x, y) = screen.pixel(client, at)?;
                client.move_to(x, y)?;
                *last = Some(at);
                placed = true;
            } else if !placed {
                return Err(
                    "Either x and y, instance_path, or a prior action that establishes \
                            mouse position is required"
                        .to_owned(),
                );
            }
        }
        match action {
            "mouseButtonDown" | "mouseButtonUp" | "mouseButtonClick" => {
                let button = match text(step, "mouse_button") {
                    Some("left") => Button::Left,
                    Some("right") => Button::Right,
                    _ => {
                        return Err(format!(
                            "mouse_button (\"left\" or \"right\") is required for {action}"
                        ))
                    }
                };
                if action == "mouseButtonClick" {
                    click(client, button)?;
                } else {
                    client.button(button, action == "mouseButtonDown")?;
                }
            }
            "scrollUp" | "scrollDown" => client.scroll(action == "scrollUp")?,
            _ => {}
        }
        pause(client, step, action == "wait")?;
    }
    Ok(SUCCESS.to_owned())
}

pub(super) fn keyboard(client: &mut dyn Client, steps: &[Value]) -> Result<String, String> {
    let mut screen = Screen::default();
    for step in steps {
        let action = text(step, "action").unwrap_or_default();
        match action {
            "keyDown" | "keyUp" | "keyPress" => {
                // Studio passes key_code to an engine call, and these are
                // that call's own complaints.
                let name = match step.get("key_code") {
                    None | Some(Value::Null) => return Err("Argument 2 missing or nil".to_owned()),
                    Some(name) => name.as_str().unwrap_or_default(),
                };
                let key = keycode::find(name).ok_or("Unable to cast string to KeyCode")?;
                if !key.typeable() {
                    return Err(format!(
                        "KeyCode.{name} has no key on this keyboard: the Roblox client only \
                         takes it from a gamepad, a mouse or touch"
                    ));
                }
                if action != "keyUp" {
                    client.key(key, true)?;
                }
                if action != "keyDown" {
                    client.key(key, false)?;
                }
            }
            "textInput" => {
                let typed = text(step, "text_inputs").ok_or("Argument 1 missing or nil")?;
                let target = aim(client, &mut screen, step)?;
                if let Some(at) = target {
                    let (x, y) = screen.pixel(client, at)?;
                    client.move_to(x, y)?;
                    click(client, Button::Left)?;
                }
                let landed = client.ask(
                    CommandBody::TypeText {
                        text: typed.to_owned(),
                        path: target.and(text(step, "instance_path")).map(str::to_owned),
                    },
                    ASK_TIMEOUT,
                )?;
                if landed == "chat" {
                    client.type_keys(typed)?;
                }
            }
            _ => {}
        }
        pause(client, step, action == "wait")?;
    }
    Ok(SUCCESS.to_owned())
}

/// A step's own position in GUI coordinates: the centre of the GuiObject at
/// `instance_path`, or `x` and `y` given together; `None` for neither.
fn aim(
    client: &mut dyn Client,
    screen: &mut Screen,
    step: &Value,
) -> Result<Option<(f64, f64)>, String> {
    if let Some(path) = text(step, "instance_path") {
        let located = screen.ask(client, Some(path))?;
        return match (located.x, located.y) {
            (Some(x), Some(y)) => Ok(Some((x, y))),
            _ => Err(format!("Instance not found: {path}")),
        };
    }
    let coordinate = |name: &str| step.get(name).and_then(Value::as_f64);
    Ok(coordinate("x").zip(coordinate("y")))
}

/// `wait_time_ms`: a `wait` step's whole point, and any other step's pause
/// before the next one.
fn pause(client: &mut dyn Client, step: &Value, required: bool) -> Result<(), String> {
    let Some(ms) = step.get("wait_time_ms").and_then(Value::as_f64) else {
        return if required {
            Err("wait_time_ms is required for wait action".to_owned())
        } else {
            Ok(())
        };
    };
    if !(0.0..=MAX_WAIT_MS).contains(&ms) {
        return Err(format!("wait_time_ms must be between 0 and {MAX_WAIT_MS}"));
    }
    client.sleep(Duration::from_secs_f64(ms / 1000.0));
    Ok(())
}

fn click(client: &mut dyn Client, button: Button) -> Result<(), String> {
    client.button(button, true)?;
    client.sleep(rbx_play_click_hold());
    client.button(button, false)
}

/// How long a click holds its button: the embed back end's, where there is
/// one.
fn rbx_play_click_hold() -> Duration {
    #[cfg(any(target_os = "linux", windows))]
    return rbx_play::embed::CLICK_HOLD;
    #[cfg(not(any(target_os = "linux", windows)))]
    Duration::from_millis(30)
}

fn text<'a>(value: &'a Value, name: &str) -> Option<&'a str> {
    value.get(name).and_then(Value::as_str)
}

/// The client's GUI inset and viewport, asked once per call: GUI
/// coordinates start below the inset, the window holds the whole viewport.
#[derive(Default)]
struct Screen {
    known: Option<Located>,
}

impl Screen {
    fn ask(&mut self, client: &mut dyn Client, path: Option<&str>) -> Result<Located, String> {
        let answer = client.ask(
            CommandBody::Locate {
                path: path.map(str::to_owned),
            },
            ASK_TIMEOUT,
        )?;
        let located: Located = serde_json::from_str(&answer)
            .map_err(|err| format!("the client's answer could not be read: {err}"))?;
        self.known = Some(located);
        Ok(located)
    }

    fn pixel(&mut self, client: &mut dyn Client, (x, y): (f64, f64)) -> Result<(i32, i32), String> {
        let located = match self.known {
            Some(located) => located,
            None => self.ask(client, None)?,
        };
        let (width, height) = client.size()?;
        Ok(window_point(&located, (width, height), (x, y)))
    }
}

/// GUI coordinates to window pixels: past the inset, then scaled by how
/// many window pixels the client draws each viewport pixel with.
pub(super) fn window_point(
    screen: &Located,
    (width, height): (u32, u32),
    (x, y): (f64, f64),
) -> (i32, i32) {
    let scale = |window: u32, viewport: f64| {
        if viewport > 0.0 {
            f64::from(window) / viewport
        } else {
            1.0
        }
    };
    let px = (x + screen.inset_x) * scale(width, screen.width);
    let py = (y + screen.inset_y) * scale(height, screen.height);
    // Saturating float-to-int: an off-screen point stays off-screen.
    (px.round() as i32, py.round() as i32)
}

/// The real client: its probe over the channel, and OS input into the
/// embedded window, opened at the first step that needs it.
pub(super) struct Live {
    play: PlayInput,
    #[cfg(any(target_os = "linux", windows))]
    input: Option<rbx_play::embed::Input>,
}

impl Live {
    pub(super) fn new(play: PlayInput) -> Self {
        Live {
            play,
            #[cfg(any(target_os = "linux", windows))]
            input: None,
        }
    }

    #[cfg(any(target_os = "linux", windows))]
    fn input(&mut self) -> Result<&mut rbx_play::embed::Input, String> {
        let input = match self.input.take() {
            Some(input) => input,
            None => {
                let window = self.play.window.ok_or(
                    "the Roblox client's window is not shown in the editor's 3D view, so mouse \
                     and key events cannot reach it",
                )?;
                rbx_play::embed::Input::open(window).map_err(|err| err.to_string())?
            }
        };
        Ok(self.input.insert(input))
    }

    #[cfg(not(any(target_os = "linux", windows)))]
    fn input(&mut self) -> Result<&mut Unreachable, String> {
        Err(
            "the Roblox client keeps its own window on this platform, so mouse and key events \
             cannot reach it"
                .to_owned(),
        )
    }
}

/// No input back end on this platform: [`Live::input`] always fails first.
#[cfg(not(any(target_os = "linux", windows)))]
enum Unreachable {}

#[cfg(not(any(target_os = "linux", windows)))]
impl Unreachable {
    fn size(&self) -> Result<(u32, u32), String> {
        match *self {}
    }
    fn move_to(&mut self, _: i32, _: i32) -> Result<(), String> {
        match *self {}
    }
    fn button(&mut self, _: Button, _: bool) -> Result<(), String> {
        match *self {}
    }
    fn scroll(&mut self, _: bool) -> Result<(), String> {
        match *self {}
    }
    fn key(&mut self, _: &KeyCode, _: bool) -> Result<(), String> {
        match *self {}
    }
    fn type_text(&mut self, _: &str) -> Result<(), String> {
        match *self {}
    }
}

#[cfg(any(target_os = "linux", windows))]
fn embed_button(button: Button) -> rbx_play::embed::Button {
    match button {
        Button::Left => rbx_play::embed::Button::Left,
        Button::Right => rbx_play::embed::Button::Right,
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
fn embed_button(button: Button) -> Button {
    button
}

impl Client for Live {
    fn ask(&mut self, body: CommandBody, timeout: Duration) -> Result<String, String> {
        self.play
            .probe
            .request(body, timeout)
            .map_err(|err| match err {
                RequestError::Failed(message) => message,
                RequestError::TimedOut => "the Roblox client did not answer in time".to_owned(),
                RequestError::Ended => "the playtest ended".to_owned(),
            })
    }

    fn size(&mut self) -> Result<(u32, u32), String> {
        self.input()?.size().map_err(|err| err.to_string())
    }

    /// Then gives the client a frame to see the pointer there: a wheel notch
    /// sent at once after the pointer enters its window is dropped.
    fn move_to(&mut self, x: i32, y: i32) -> Result<(), String> {
        self.input()?.move_to(x, y).map_err(|err| err.to_string())?;
        std::thread::sleep(rbx_play_click_hold());
        Ok(())
    }

    fn button(&mut self, button: Button, down: bool) -> Result<(), String> {
        self.input()?
            .button(embed_button(button), down)
            .map_err(|err| err.to_string())
    }

    fn scroll(&mut self, up: bool) -> Result<(), String> {
        self.input()?.scroll(up).map_err(|err| err.to_string())
    }

    fn key(&mut self, key: &'static KeyCode, down: bool) -> Result<(), String> {
        self.input()?.key(key, down).map_err(|err| err.to_string())
    }

    fn type_keys(&mut self, text: &str) -> Result<(), String> {
        self.input()?.type_text(text).map_err(|err| err.to_string())
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
