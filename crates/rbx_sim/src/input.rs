//! A client's input in Play (local): what the editor's keyboard and mouse
//! did, written into `InputObject`s and handed to the game (bound actions,
//! then `UserInputService`'s signals; see `rbx_lua::server`). Like
//! Roblox, one `InputObject` stands for a key or button from its press to
//! its release, and one for all mouse motion.

use std::collections::HashMap;

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

/// One thing the player did.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// A key, by its `Enum.KeyCode` name (`W`, `Space`, `LeftShift`).
    Key { code: String, down: bool },
    /// Mouse button 1 (left), 2 (right) or 3 (middle).
    Button { button: u8, down: bool },
    /// The cursor at `position` (viewport pixels) after moving `delta`; a
    /// locked mouse keeps reporting motion where it stays put.
    Move { position: [f32; 2], delta: [f32; 2] },
    /// The wheel, positive notches away from the user (zoom in).
    Wheel { notches: f32 },
    /// A finger on a touch screen at `position` (viewport pixels); `id`
    /// tells fingers apart while they're down.
    Touch {
        id: u32,
        position: [f32; 2],
        phase: Phase,
    },
    /// A gamepad (`pad` 1 to 8) plugged in or out.
    Gamepad { pad: u8, connected: bool },
    /// A gamepad button by its `Enum.KeyCode` name (`ButtonA`, `DPadUp`).
    GamepadButton { pad: u8, code: String, down: bool },
    /// A stick (`Thumbstick1`/`Thumbstick2`: x and y from -1 to 1, up
    /// positive) or a trigger (`ButtonL2`/`ButtonR2`: z from 0 to 1).
    GamepadAxis {
        pad: u8,
        code: String,
        position: [f32; 3],
    },
    /// A motion sensor's reading: gravity and acceleration in g, rotation
    /// as X, Y, Z Euler angles in radians.
    Motion { sensor: Sensor, vector: [f32; 3] },
}

/// Where a finger is in its touch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Began,
    Moved,
    Ended,
}

/// A device motion sensor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sensor {
    Gravity,
    Acceleration,
    Rotation,
}

impl Sensor {
    /// The `UserInputService` signal it fires besides `InputChanged`.
    fn signal(self) -> &'static str {
        match self {
            Sensor::Gravity => "DeviceGravityChanged",
            Sensor::Acceleration => "DeviceAccelerationChanged",
            Sensor::Rotation => "DeviceRotationChanged",
        }
    }
}

/// An input written into its `InputObject`: the object, the
/// `UserInputService` signal it fires, and the one a touch or a sensor
/// fires after it (`TouchStarted`, `DeviceGravityChanged`).
pub(crate) struct Written {
    pub object: Ref,
    pub signal: &'static str,
    pub also: Option<&'static str>,
}

/// `Enum.MouseBehavior`, as a script left `UserInputService.MouseBehavior`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseBehavior {
    #[default]
    Default,
    LockCenter,
    LockCurrentPosition,
}

#[derive(Default)]
pub(crate) struct Inputs {
    pub(crate) pending: Vec<Input>,
    objects: HashMap<String, Ref>,
    position: [f32; 2],
    /// Where each slot's input last was, for its `Delta`.
    last: HashMap<String, [f32; 3]>,
    pub(crate) touches: crate::touch::Touches,
    /// The device emulated is a touch screen: the left button and the
    /// cursor while it's down are a finger, as in Studio's device
    /// emulator.
    pub(crate) emulate_touch: bool,
    finger: Option<[f32; 2]>,
}

fn item(db: &ReflectionDatabase, enum_name: &str, name: &str) -> Option<Variant> {
    db.enum_value(enum_name, name).map(Variant::Enum)
}

fn v3(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

impl Inputs {
    /// What happened since the last frame, a touch screen's mouse made a
    /// finger.
    pub(crate) fn take_pending(&mut self) -> Vec<Input> {
        let pending = std::mem::take(&mut self.pending);
        if !self.emulate_touch {
            return pending;
        }
        let mut out = Vec::new();
        for input in pending {
            match input {
                Input::Move { position, .. } => {
                    self.position = position;
                    if self.finger.is_some() {
                        self.finger = Some(position);
                        out.push(Input::Touch {
                            id: 0,
                            position,
                            phase: Phase::Moved,
                        });
                    }
                }
                Input::Button { button: 1, down } => {
                    let phase = if down { Phase::Began } else { Phase::Ended };
                    if down == self.finger.is_none() {
                        self.finger = down.then_some(self.position);
                        out.push(Input::Touch {
                            id: 0,
                            position: self.position,
                            phase,
                        });
                    }
                }
                Input::Button { .. } | Input::Wheel { .. } => {}
                other => out.push(other),
            }
        }
        out
    }
    /// Writes `input` into its `InputObject` and returns it with the
    /// `UserInputService` signals it fires; `None` for a key Roblox has no
    /// `KeyCode` for, and for a gamepad plugged in or out.
    pub(crate) fn write(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        input: &Input,
    ) -> Option<Written> {
        // Like `AbsolutePosition`, a pointer's position counts from below
        // the top bar.
        let pointer = |[x, y]: [f32; 2]| [x, y - crate::gui::TOPBAR_INSET, 0.0];
        let state_of = |down: bool| if down { "Begin" } else { "End" };
        let mut also = None;
        let (slot, kind, code, state, at): (String, String, &str, &str, Option<[f32; 3]>) =
            match input {
                Input::Touch {
                    id,
                    position,
                    phase,
                } => {
                    let (state, signal) = match phase {
                        Phase::Began => ("Begin", "TouchStarted"),
                        Phase::Moved => ("Change", "TouchMoved"),
                        Phase::Ended => ("End", "TouchEnded"),
                    };
                    also = Some(signal);
                    let slot = format!("touch {id}");
                    if *phase == Phase::Began {
                        self.objects.remove(&slot);
                        self.last.remove(&slot);
                    }
                    (
                        slot,
                        "Touch".into(),
                        "Unknown",
                        state,
                        Some(pointer(*position)),
                    )
                }
                Input::GamepadButton { pad, code, down } => {
                    item(db, "KeyCode", code)?;
                    let slot = format!("pad{pad} {code}");
                    (slot, format!("Gamepad{pad}"), code, state_of(*down), None)
                }
                Input::GamepadAxis {
                    pad,
                    code,
                    position,
                } => {
                    item(db, "KeyCode", code)?;
                    let slot = format!("pad{pad} {code}");
                    (
                        slot,
                        format!("Gamepad{pad}"),
                        code,
                        "Change",
                        Some(*position),
                    )
                }
                Input::Motion { sensor, vector } => {
                    also = Some(sensor.signal());
                    let kind = if *sensor == Sensor::Rotation {
                        "Gyro"
                    } else {
                        "Accelerometer"
                    };
                    (
                        format!("{sensor:?}"),
                        kind.into(),
                        "Unknown",
                        "Change",
                        Some(*vector),
                    )
                }
                Input::Gamepad { .. } => return None,
                _ => return self.write_desktop(dom, db, input),
            };
        let object = self.object(dom, &slot);
        let at = at.unwrap_or_else(|| self.last.get(&slot).copied().unwrap_or([0.0; 3]));
        let before = self.last.insert(slot, at).unwrap_or(at);
        let delta = [at[0] - before[0], at[1] - before[1], at[2] - before[2]];
        let values = [
            ("KeyCode", item(db, "KeyCode", code)),
            ("UserInputType", item(db, "UserInputType", &kind)),
            ("UserInputState", item(db, "UserInputState", state)),
            ("Position", Some(v3(at[0], at[1], at[2]))),
            ("Delta", Some(v3(delta[0], delta[1], delta[2]))),
        ];
        for (key, value) in values {
            if let Some(value) = value {
                let _ = dom.set_property(object, key, value);
            }
        }
        Some(Written {
            object,
            signal: signal_of(state),
            also,
        })
    }

    fn object(&mut self, dom: &mut WeakDom, slot: &str) -> Ref {
        match self.objects.get(slot) {
            Some(&r) if dom.get(r).is_some() => r,
            _ => {
                let r = dom.new_instance("InputObject", "InputObject", None);
                self.objects.insert(slot.to_owned(), r);
                r
            }
        }
    }

    fn write_desktop(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        input: &Input,
    ) -> Option<Written> {
        let (slot, kind, code, state, delta, z) = match input {
            Input::Key { code, down } => {
                item(db, "KeyCode", code)?;
                let state = if *down { "Begin" } else { "End" };
                (
                    format!("key {code}"),
                    "Keyboard",
                    code.as_str(),
                    state,
                    [0.0; 2],
                    0.0,
                )
            }
            Input::Button { button, down } => {
                let kind = match button {
                    1 => "MouseButton1",
                    2 => "MouseButton2",
                    3 => "MouseButton3",
                    _ => return None,
                };
                let state = if *down { "Begin" } else { "End" };
                (kind.to_owned(), kind, "Unknown", state, [0.0; 2], 0.0)
            }
            Input::Move { position, delta } => {
                self.position = *position;
                let kind = "MouseMovement";
                (kind.to_owned(), kind, "Unknown", "Change", *delta, 0.0)
            }
            Input::Wheel { notches } => {
                let kind = "MouseWheel";
                (
                    kind.to_owned(),
                    kind,
                    "Unknown",
                    "Change",
                    [0.0; 2],
                    *notches,
                )
            }
            _ => return None,
        };
        let object = self.object(dom, &slot);
        // Like `AbsolutePosition`, an input's position counts from below
        // the top bar.
        let [x, y] = self.position;
        let y = y - crate::gui::TOPBAR_INSET;
        let values = [
            ("KeyCode", item(db, "KeyCode", code)),
            ("UserInputType", item(db, "UserInputType", kind)),
            ("UserInputState", item(db, "UserInputState", state)),
            ("Position", Some(v3(x, y, z))),
            ("Delta", Some(v3(delta[0], delta[1], 0.0))),
        ];
        for (key, value) in values {
            if let Some(value) = value {
                let _ = dom.set_property(object, key, value);
            }
        }
        Some(Written {
            object,
            signal: signal_of(state),
            also: None,
        })
    }
}

fn signal_of(state: &str) -> &'static str {
    match state {
        "Begin" => "InputBegan",
        "End" => "InputEnded",
        _ => "InputChanged",
    }
}

/// What scripts' `VirtualInput`s sent, as the player's own input: text
/// typed key by key (US layout, Shift held for capitals).
pub(crate) fn from_virtual(sent: Vec<rbx_lua::server::VirtualInput>) -> Vec<Input> {
    use rbx_lua::server::VirtualInput as V;
    const DIGITS: [&str; 10] = [
        "Zero", "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine",
    ];
    let key = |code: &str, down| Input::Key {
        code: code.to_owned(),
        down,
    };
    let mut out = Vec::new();
    for input in sent {
        match input {
            V::Key { code, down } => out.push(Input::Key { code, down }),
            V::Button { button, down } => out.push(Input::Button { button, down }),
            V::Move { position, delta } => out.push(Input::Move { position, delta }),
            V::Wheel { notches } => out.push(Input::Wheel { notches }),
            V::Text(text) => {
                for c in text.chars() {
                    let code = match c {
                        'a'..='z' | 'A'..='Z' => c.to_ascii_uppercase().to_string(),
                        '0'..='9' => DIGITS[c as usize - '0' as usize].to_owned(),
                        ' ' => "Space".to_owned(),
                        _ => continue,
                    };
                    let shift = c.is_ascii_uppercase();
                    if shift {
                        out.push(key("LeftShift", true));
                    }
                    out.push(key(&code, true));
                    out.push(key(&code, false));
                    if shift {
                        out.push(key("LeftShift", false));
                    }
                }
            }
        }
    }
    out
}

/// A service under `game`, made if the place has none, as `GetService`
/// does.
pub(crate) fn service(dom: &mut WeakDom, class: &str) -> Ref {
    let found = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class));
    found.unwrap_or_else(|| dom.new_instance(class, class, None))
}

/// What `UserInputService.MouseBehavior` holds.
pub(crate) fn mouse_behavior(dom: &WeakDom) -> MouseBehavior {
    let value = dom
        .root_refs()
        .iter()
        .filter_map(|r| dom.get(*r))
        .find(|i| i.class() == "UserInputService")
        .and_then(|i| match i.properties().get("MouseBehavior") {
            Some(Variant::Enum(v)) => Some(*v),
            _ => None,
        });
    match value {
        Some(1) => MouseBehavior::LockCenter,
        Some(2) => MouseBehavior::LockCurrentPosition,
        _ => MouseBehavior::Default,
    }
}

/// Writes `Workspace.CurrentCamera.ViewportSize`, when it changed.
pub(crate) fn set_viewport(dom: &mut WeakDom, size: [f32; 2]) {
    let camera = dom
        .root_refs()
        .iter()
        .filter_map(|r| dom.get(*r))
        .find(|i| i.class() == "Workspace")
        .and_then(|w| match w.properties().get("CurrentCamera") {
            Some(Variant::Ref(r)) if dom.get(*r).is_some() => Some(*r),
            _ => None,
        });
    let value = Variant::Vector2(rbx_dom::Vector2Data {
        x: size[0],
        y: size[1],
    });
    if let Some(camera) = camera {
        if dom
            .get(camera)
            .and_then(|c| c.properties().get("ViewportSize"))
            != Some(&value)
        {
            let _ = dom.set_property(camera, "ViewportSize", value);
        }
    }
}

/// `Workspace.CurrentCamera`'s `CFrame` and `FieldOfView` (degrees).
pub(crate) fn camera(dom: &WeakDom) -> Option<(CFrameData, f32)> {
    let workspace = dom
        .root_refs()
        .iter()
        .filter_map(|r| dom.get(*r))
        .find(|i| i.class() == "Workspace")?;
    let camera = match workspace.properties().get("CurrentCamera") {
        Some(Variant::Ref(r)) => dom.get(*r)?,
        _ => return None,
    };
    let cframe = match camera.properties().get("CFrame") {
        Some(Variant::CFrame(c)) => *c,
        _ => return None,
    };
    let fov = match camera.properties().get("FieldOfView") {
        Some(Variant::Float32(f)) => *f,
        Some(Variant::Float64(f)) => *f as f32,
        _ => 70.0,
    };
    Some((cframe, fov))
}

/// Gives a server's workspace a camera when the place saved none, as a
/// Roblox server's `workspace.CurrentCamera` always is one.
pub(crate) fn server_camera(dom: &mut WeakDom) {
    let Some(workspace) = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
    else {
        return;
    };
    let saved = match dom
        .get(workspace)
        .and_then(|w| w.properties().get("CurrentCamera"))
    {
        Some(Variant::Ref(saved)) => Some(*saved),
        _ => None,
    };
    if saved.is_some_and(|r| dom.get(r).is_some_and(|i| i.class() == "Camera")) {
        return;
    }
    let camera = dom.new_instance("Camera", "Camera", Some(workspace));
    let _ = dom.set_property(camera, "FieldOfView", Variant::Float32(70.0));
    let _ = dom.set_property(workspace, "CurrentCamera", Variant::Ref(camera));
}

/// Gives the workspace the client's own camera: cameras don't replicate,
/// so a client never sees the place's saved one, and its own starts as
/// `Custom`, for the camera scripts to drive.
pub(crate) fn client_camera(dom: &mut WeakDom, db: &ReflectionDatabase) {
    let Some(workspace) = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == "Workspace"))
    else {
        return;
    };
    if let Some(Variant::Ref(saved)) = dom
        .get(workspace)
        .and_then(|w| w.properties().get("CurrentCamera"))
        .cloned()
    {
        if dom.get(saved).is_some_and(|i| i.class() == "Camera") {
            dom.remove(saved);
        }
    }
    let camera = dom.new_instance("Camera", "Camera", Some(workspace));
    let _ = dom.set_property(camera, "FieldOfView", Variant::Float32(70.0));
    if let Some(custom) = item(db, "CameraType", "Custom") {
        let _ = dom.set_property(camera, "CameraType", custom);
    }
    let _ = dom.set_property(
        camera,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 0.0,
                y: 20.0,
                z: 20.0,
            },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
    let _ = dom.set_property(workspace, "CurrentCamera", Variant::Ref(camera));
}
