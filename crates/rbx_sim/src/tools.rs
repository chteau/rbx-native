//! Tools, our own from the docs and the ToolEquip golden: a tool is held
//! while it's a child of a character. `Humanoid:EquipTool` puts the one
//! held back in the Backpack first, then parents the new one to the
//! character, all before it returns; the reparenting's own events tell:
//! the old tool's `Unequipped` runs just before its `AncestryChanged`, the
//! new one's `Equipped(mouse)` just after (`character-ConfA
//! ConfA.Unequipped ConfA.Ancestry:Backpack character+ConfB
//! ConfB.Ancestry:Model ConfB.Equipped:Instance returned`). A tool that
//! requires a handle and has none still moves, but `Equipped` never fires.
//! The server welds the handle to the right hand (`RightGrip`, `C0` the
//! hand's grip attachment, `C1` the tool's `Grip`).
//!
//! A client moves its own player's tools (number keys, Backspace drops
//! one, `EquipTool` in a LocalScript) and the server follows; `Activate`,
//! `Deactivate` and a click with a tool held fire on the client, then on
//! the server.

use std::collections::HashMap;

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_lua::server::{Arg, CallReply};
use rbx_reflection::ReflectionDatabase;

use crate::humanoid::{cframe, flag, inverse, mul, reference};

/// What a client tells the server about its tools and sounds.
#[derive(Clone, Debug)]
pub(crate) enum ClientOp {
    /// A sound it played (`true`) or stopped.
    Sound(Ref, bool),
    /// A humanoid it simulates finished a `MoveTo` (reached or not): the
    /// server's `MoveToFinished` fires too, for the server script that
    /// asked.
    MoveToFinished(Ref, bool),
    /// The tool's new parent (the player's Backpack or character, or the
    /// Workspace for a drop).
    Move(Ref, Option<Ref>),
    /// Activated (`true`) or deactivated.
    Activate(Ref, bool),
}

#[derive(Default)]
pub struct Tools {
    /// Each player's `Mouse`, what `Equipped` passes.
    mice: HashMap<Ref, Ref>,
    /// A client's moves and activations for the server.
    pub(crate) ops: Vec<ClientOp>,
    /// A client applying the server's frame: those moves are the server's.
    pub(crate) remote: bool,
    /// A client's hotbar: tools in the order they came, each keeping its
    /// slot while it's held.
    slots: Vec<Ref>,
    /// The tool a mouse press activated, for its release.
    pressed: Option<Ref>,
}

const NUMBER_KEYS: [&str; 10] = [
    "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Zero",
];

pub(crate) fn is_method(class: &str, key: &str) -> bool {
    matches!(
        (class, key),
        ("Humanoid", "EquipTool" | "UnequipTools") | ("Tool", "Activate" | "Deactivate")
    )
}

pub(crate) fn is_signal(class: &str, key: &str) -> bool {
    class == "Tool" && matches!(key, "Equipped" | "Unequipped" | "Activated" | "Deactivated")
}

fn class_is(dom: &WeakDom, r: Ref, class: &str) -> bool {
    dom.get(r).is_some_and(|i| i.class() == class)
}

fn child_of_class(dom: &WeakDom, parent: Ref, class: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|c| class_is(dom, *c, class))
}

fn child_named(dom: &WeakDom, parent: Ref, name: &str) -> Option<Ref> {
    dom.get(parent)?
        .children()
        .iter()
        .copied()
        .find(|c| dom.get(*c).is_some_and(|i| i.name() == name))
}

/// The player whose character `model` is.
fn owner_of(dom: &WeakDom, model: Ref) -> Option<Ref> {
    crate::players::players_of(dom)
        .into_iter()
        .find(|p| reference(dom, *p, "Character") == Some(model))
}

/// Whether `model` is a character (a model with a Humanoid).
fn is_character(dom: &WeakDom, model: Option<Ref>) -> bool {
    model.is_some_and(|m| class_is(dom, m, "Model") && child_of_class(dom, m, "Humanoid").is_some())
}

fn handle(dom: &WeakDom, db: &ReflectionDatabase, tool: Ref) -> Option<Ref> {
    child_named(dom, tool, "Handle").filter(|h| {
        dom.get(*h)
            .is_some_and(|i| db.is_subclass_of(i.class(), "BasePart"))
    })
}

fn hand(dom: &WeakDom, character: Ref) -> Option<Ref> {
    child_named(dom, character, "RightHand").or_else(|| child_named(dom, character, "Right Arm"))
}

/// Where a hand holds things: its `RightGripAttachment`, or a classic arm's
/// grip (a stud below its centre, turned to point forward; ToolEquip
/// `client_grip_c0`).
fn grip_c0(dom: &WeakDom, db: &ReflectionDatabase, hand: Ref) -> CFrameData {
    if let Some(attachment) = child_named(dom, hand, "RightGripAttachment") {
        if let Some((_, Variant::CFrame(c))) = dom
            .get(attachment)
            .and_then(|i| db.stored_or_default(i, "CFrame"))
        {
            return *c;
        }
    }
    CFrameData {
        position: Vector3Data {
            x: 0.0,
            y: -1.0,
            z: 0.0,
        },
        rotation: [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0],
    }
}

/// Puts `tool`'s handle in `character`'s right hand with a `RightGrip`.
fn weld(dom: &mut WeakDom, db: &ReflectionDatabase, tool: Ref, character: Ref) {
    let (Some(handle), Some(hand)) = (handle(dom, db, tool), hand(dom, character)) else {
        return;
    };
    let c0 = grip_c0(dom, db, hand);
    let c1 = match dom.get(tool).and_then(|i| db.stored_or_default(i, "Grip")) {
        Some((_, Variant::CFrame(c))) => *c,
        _ => crate::humanoid::identity(),
    };
    if let Some(at) = cframe(dom, db, hand) {
        let held = mul(&mul(&at, &c0), &inverse(&c1));
        let _ = dom.set_property(handle, "CFrame", Variant::CFrame(held));
    }
    let weld = dom.new_instance("Weld", "RightGrip", None);
    let _ = dom.set_property(weld, "Part0", Variant::Ref(hand));
    let _ = dom.set_property(weld, "Part1", Variant::Ref(handle));
    let _ = dom.set_property(weld, "C0", Variant::CFrame(c0));
    let _ = dom.set_property(weld, "C1", Variant::CFrame(c1));
    dom.set_parent(weld, Some(hand));
}

/// Takes away the `RightGrip` holding `tool`'s handle.
fn unweld(dom: &mut WeakDom, db: &ReflectionDatabase, tool: Ref, character: Ref) {
    let (Some(handle), Some(hand)) = (handle(dom, db, tool), hand(dom, character)) else {
        return;
    };
    let grips: Vec<Ref> = dom
        .get(hand)
        .map(|i| i.children().to_vec())
        .unwrap_or_default()
        .into_iter()
        .filter(|g| {
            dom.get(*g).is_some_and(|i| i.name() == "RightGrip")
                && reference(dom, *g, "Part1") == Some(handle)
        })
        .collect();
    for grip in grips {
        dom.remove(grip);
    }
}

impl Tools {
    fn mouse(&mut self, dom: &mut WeakDom, player: Ref) -> Ref {
        if let Some(&m) = self.mice.get(&player).filter(|m| dom.get(**m).is_some()) {
            return m;
        }
        let m = dom.new_instance("PlayerMouse", "Instance", None);
        self.mice.insert(player, m);
        m
    }

    /// A tool moved from `old` to `new`: its `Unequipped` (`after: false`)
    /// or `Equipped` (`after: true`), the grip with them on a server, and
    /// on a client a move of its own the server must hear of.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reparented(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        server: bool,
        local: Option<Ref>,
        tool: Ref,
        old: Option<Ref>,
        new: Option<Ref>,
        after: bool,
    ) -> Vec<(Ref, String, Vec<Arg>)> {
        if !class_is(dom, tool, "Tool") || old == new {
            return Vec::new();
        }
        if !after {
            let Some(character) = old.filter(|_| is_character(dom, old)) else {
                return Vec::new();
            };
            if server {
                unweld(dom, db, tool, character);
            }
            return vec![(tool, "Unequipped".into(), Vec::new())];
        }
        if !server && !self.remote {
            if let Some(player) = local {
                let mine = |p: Option<Ref>| {
                    p.is_some_and(|p| {
                        Some(p) == reference(dom, player, "Character")
                            || dom.parent(p) == Some(player)
                            || class_is(dom, p, "Workspace")
                    })
                };
                if mine(old) && mine(new) {
                    self.ops.push(ClientOp::Move(tool, new));
                }
            }
        }
        let Some(character) = new.filter(|_| is_character(dom, new)) else {
            return Vec::new();
        };
        let handled = handle(dom, db, tool).is_some();
        if !handled && flag(dom, db, tool, "RequiresHandle") {
            return Vec::new();
        }
        if server && handled {
            weld(dom, db, tool, character);
        }
        let player = owner_of(dom, character).unwrap_or(character);
        let mouse = self.mouse(dom, player);
        vec![(tool, "Equipped".into(), vec![Arg::Instance(mouse)])]
    }

    /// The hotbar as it stands: tools of `player`'s Backpack and the one
    /// held, in slot order.
    pub(crate) fn hotbar(&mut self, dom: &WeakDom, player: Ref) -> Vec<Ref> {
        let character = reference(dom, player, "Character");
        let mut now: Vec<Ref> = Vec::new();
        for parent in [character, child_of_class(dom, player, "Backpack")]
            .into_iter()
            .flatten()
        {
            for child in dom.get(parent).map(|i| i.children()).unwrap_or_default() {
                if class_is(dom, *child, "Tool") {
                    now.push(*child);
                }
            }
        }
        self.slots.retain(|t| now.contains(t));
        for tool in now {
            if !self.slots.contains(&tool) {
                self.slots.push(tool);
            }
        }
        self.slots.clone()
    }

    /// A key or click the game didn't take, on a client with the Backpack
    /// core GUI on: number keys equip a slot (again: put it away),
    /// Backspace drops a droppable tool, mouse button 1 activates the tool
    /// held. The signals to fire, in order.
    pub(crate) fn input(
        &mut self,
        dom: &mut WeakDom,
        db: &ReflectionDatabase,
        player: Ref,
        input: &crate::Input,
        backpack_shown: bool,
    ) -> Vec<(Ref, String, Vec<Arg>)> {
        let Some(character) = reference(dom, player, "Character") else {
            return Vec::new();
        };
        let held = child_of_class(dom, character, "Tool");
        match input {
            crate::Input::Key { code, down: true } if backpack_shown => {
                let backpack = child_of_class(dom, player, "Backpack");
                if let Some(slot) = NUMBER_KEYS.iter().position(|k| k == code) {
                    let slots = self.hotbar(dom, player);
                    let Some(&tool) = slots.get(slot) else {
                        return Vec::new();
                    };
                    if let Some(h) = held {
                        dom.set_parent(h, backpack);
                    }
                    if held != Some(tool) {
                        dom.set_parent(tool, Some(character));
                    }
                } else if code == "Backspace" {
                    if let Some(h) = held.filter(|h| flag(dom, db, *h, "CanBeDropped")) {
                        let workspace = dom
                            .root_refs()
                            .iter()
                            .copied()
                            .find(|r| class_is(dom, *r, "Workspace"));
                        dom.set_parent(h, workspace);
                    }
                }
                Vec::new()
            }
            crate::Input::Button { button: 1, down } => {
                let tool = if *down {
                    held.filter(|h| {
                        flag(dom, db, *h, "Enabled") && !flag(dom, db, *h, "ManualActivationOnly")
                    })
                } else {
                    self.pressed.take()
                };
                let Some(tool) = tool else {
                    return Vec::new();
                };
                if *down {
                    self.pressed = Some(tool);
                }
                self.ops.push(ClientOp::Activate(tool, *down));
                let name = if *down { "Activated" } else { "Deactivated" };
                vec![(tool, name.into(), Vec::new())]
            }
            _ => Vec::new(),
        }
    }

    /// `Humanoid:EquipTool`, `UnequipTools`, `Tool:Activate`, `Deactivate`.
    pub(crate) fn call(
        &mut self,
        dom: &mut WeakDom,
        client: bool,
        referent: Ref,
        method: &str,
        args: &[Arg],
    ) -> Result<CallReply, String> {
        match method {
            "EquipTool" | "UnequipTools" => {
                let character = dom.parent(referent);
                let backpack = character
                    .and_then(|c| owner_of(dom, c))
                    .and_then(|p| child_of_class(dom, p, "Backpack"));
                let tool = match (method, args.first()) {
                    ("UnequipTools", _) => None,
                    (_, Some(Arg::Instance(t))) if class_is(dom, *t, "Tool") => Some(*t),
                    _ => return Err("Unable to cast value to Object".into()),
                };
                let Some(character) = character else {
                    return Ok(CallReply::default());
                };
                let held: Vec<Ref> = dom
                    .get(character)
                    .map(|i| i.children().to_vec())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|c| class_is(dom, *c, "Tool") && Some(*c) != tool)
                    .collect();
                for h in held {
                    dom.set_parent(h, backpack);
                }
                if let Some(tool) = tool.filter(|t| dom.parent(*t) != Some(character)) {
                    dom.set_parent(tool, Some(character));
                }
                Ok(CallReply::default())
            }
            "Activate" | "Deactivate" => {
                let on = method == "Activate";
                if client {
                    self.ops.push(ClientOp::Activate(referent, on));
                }
                let name = if on { "Activated" } else { "Deactivated" };
                Ok(CallReply {
                    fire: vec![(referent, name.into(), Vec::new())],
                    ..CallReply::default()
                })
            }
            _ => Err(format!("{method} is not a valid member")),
        }
    }
}

/// Whether the server takes a client's move of `tool` to `parent`: the
/// player's own tool, between their Backpack and character, or dropped
/// from their hand into the Workspace when it can be.
pub(crate) fn allowed(
    dom: &WeakDom,
    db: &ReflectionDatabase,
    player: Ref,
    tool: Ref,
    parent: Option<Ref>,
) -> bool {
    if !class_is(dom, tool, "Tool") {
        return false;
    }
    let character = reference(dom, player, "Character");
    let backpack = child_of_class(dom, player, "Backpack");
    let from = dom.parent(tool);
    let ours = |p: Option<Ref>| p.is_some() && (p == character || p == backpack);
    if !ours(from) {
        return false;
    }
    ours(parent)
        || (from == character
            && parent.is_some_and(|p| class_is(dom, p, "Workspace"))
            && flag(dom, db, tool, "CanBeDropped"))
}
