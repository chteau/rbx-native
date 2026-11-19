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
}

fn item(db: &ReflectionDatabase, enum_name: &str, name: &str) -> Option<Variant> {
    db.enum_value(enum_name, name).map(Variant::Enum)
}

fn v3(x: f32, y: f32, z: f32) -> Variant {
    Variant::Vector3(Vector3Data { x, y, z })
}

impl Inputs {
    /// Writes `input` into its `InputObject` and returns it with the
    /// `UserInputService` signal it fires; `None` for a key Roblox has no
    /// `KeyCode` for.
    pub(crate) fn write(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        input: &Input,
    ) -> Option<(Ref, &'static str)> {
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
        };
        let object = match self.objects.get(&slot) {
            Some(&r) if dom.get(r).is_some() => r,
            _ => {
                let r = dom.new_instance("InputObject", "InputObject", None);
                self.objects.insert(slot.clone(), r);
                r
            }
        };
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
        let signal = match state {
            "Begin" => "InputBegan",
            "End" => "InputEnded",
            _ => "InputChanged",
        };
        Some((object, signal))
    }
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
