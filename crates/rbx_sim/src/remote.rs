//! A player who plays a Local Run from another computer: what crosses the
//! wire between the session's host and that player's editor. Bytes only;
//! the transport (the co-edit room's relay, sealed with the place key) is
//! the editor's, and nothing here is Roblox's own protocol.
//!
//! Upstream ([`Up`]) the player asks to join with their name, user id and
//! avatar, then sends the same [`Action`]s a recording keeps (keys, mouse,
//! screen size, chat, reset): small and many. Downstream ([`Down`]) the
//! host sends a [`Frame`] per frame shown: its client's DataModel changes
//! as [`Op`]s carrying the values they ended on (the whole tree first,
//! `reset`), and what the player's window shows besides (camera, core GUI
//! when it changed, chat, notices, printed lines). Referents are the
//! host's, so the player's copy of the tree names every instance as the
//! client does. Each message is LZ4-compressed.
//!
//! [`Host`] is the session's side: offers to accept, the remote players,
//! what each has been sent. [`Viewer`] is the player's: their copy of the
//! client's DataModel, kept up by the frames.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use rbx_dom::{CFrameData, Change, Content, Instance, Ref, Variant, WeakDom};
use rbx_lua::server::ChatLine;

use crate::gui::CoreGui;
use crate::replay::Action;
use crate::{LineKind, MouseBehavior, Peer, Session};

/// Bumped whenever a message's layout changes; a peer on another version
/// is refused, not misread.
pub const VERSION: u8 = 1;

mod codec;
pub use codec::{decode_down, decode_up, encode_down, encode_up};

/// Who sent or gets a message, as the transport names its ends.
pub type PeerId = u64;

/// From a remote player's editor to the session's host.
#[derive(Clone, Debug, PartialEq)]
pub enum Up {
    /// Asks to join as `name`, wearing `template` (a binary model; `None`
    /// for the default character).
    Ask {
        name: String,
        user_id: i64,
        template: Option<Vec<u8>>,
    },
    /// What the player did since the last message, in order.
    Actions(Vec<Action>),
    /// The test purchase dialog the frames showed closed.
    Purchase(bool),
    Leave,
    /// Frames went missing (the connection dropped): the whole tree again.
    Resync,
}

/// From the host to remote players.
#[derive(Clone, Debug, PartialEq)]
pub enum Down {
    /// To everyone, while a session takes players: who hosts it.
    Open {
        host: String,
    },
    /// The ask was taken: `player` is their `Player`, named `name`.
    Welcome {
        player: Ref,
        name: String,
    },
    Frame(Box<Frame>),
    /// The session is over for this player: ended, declined, kicked.
    Ended(String),
}

/// Where an instance goes, as `place` puts it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Parent {
    /// A root of the tree (a service).
    Root,
    Under(Ref),
    /// Out of the tree, kept (an instance parented to nil).
    Nil,
}

/// One change to the player's copy of the tree.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    Add {
        at: Ref,
        parent: Parent,
        class: String,
        name: String,
        props: Vec<(String, Variant)>,
    },
    /// `None`: the property was removed. `Name` is the instance's name.
    Prop {
        at: Ref,
        name: String,
        value: Option<Variant>,
    },
    Move {
        at: Ref,
        parent: Parent,
    },
    /// The instance and everything under it.
    Remove(Ref),
    Class {
        at: Ref,
        class: String,
    },
}

/// One frame of a remote player's client, and of their window.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Frame {
    /// The ops build the tree from nothing.
    pub reset: bool,
    pub ops: Vec<Op>,
    pub camera: Option<(CFrameData, f32)>,
    pub mouse: MouseBehavior,
    pub player_gui: Option<Ref>,
    /// Only when it changed.
    pub core: Option<CoreGui>,
    pub chat: Vec<ChatLine>,
    pub notices: Vec<(String, String)>,
    /// The test purchase dialog: title, line.
    pub dialog: Option<(String, String)>,
    /// `Emit(n)` (`Some`) and `Clear` (`None`) calls.
    pub bursts: Vec<(Ref, Option<u32>)>,
    /// What the client printed.
    pub rows: Vec<(LineKind, String)>,
    pub paused: bool,
}

/// Whether `value` means anything on another computer: a script's
/// `EditableImage` or `EditableMesh` lives in the host's process only.
fn sendable(value: &Variant) -> bool {
    !matches!(value, Variant::Content(Content::Object(_)))
}

/// What one remote player's editor holds of their client's tree, as the
/// host keeps track of it, and the ops that bring it up to date.
#[derive(Default)]
pub struct Encoder {
    /// Instances the player has.
    known: HashSet<Ref>,
    /// The core GUI as last sent.
    core: Option<CoreGui>,
}

/// Not in the game's tree: an `AnimationTrack`, a script's
/// `Instance.new` not parented yet, an old character. The test windows
/// leave them out too.
fn hidden(dom: &WeakDom, r: Ref) -> bool {
    dom.parent(r).is_none() || !dom.in_tree(r)
}

fn props(instance: &Instance) -> Vec<(String, Variant)> {
    instance
        .properties()
        .iter()
        .filter(|(_, v)| sendable(v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

impl Encoder {
    /// The whole tree, from nothing.
    pub fn snapshot(&mut self, dom: &WeakDom) -> Vec<Op> {
        self.known.clear();
        self.core = None;
        let mut ops = Vec::new();
        for root in dom.root_refs() {
            self.add_subtree(dom, *root, &mut ops);
        }
        ops
    }

    /// What `changes` (a frame's, read back from `dom`) do to the player's
    /// copy: the same as the test windows' mirror does to theirs.
    pub fn ops(&mut self, dom: &WeakDom, changes: &[Change]) -> Vec<Op> {
        let mut ops = Vec::new();
        // A value written many times in a tick is sent once: as it ended.
        let mut written = HashSet::new();
        for change in changes {
            match change {
                Change::Property { referent, name } => {
                    let Some(instance) = dom.get(*referent) else {
                        continue;
                    };
                    if !self.known.contains(referent) || !written.insert((*referent, name)) {
                        continue;
                    }
                    let value = if name == "Name" {
                        Some(Variant::String(instance.name().to_owned()))
                    } else {
                        instance.properties().get(name.as_str()).cloned()
                    };
                    if value.as_ref().is_some_and(|v| !sendable(v)) {
                        continue;
                    }
                    ops.push(Op::Prop {
                        at: *referent,
                        name: name.clone(),
                        value,
                    });
                }
                Change::Added(r) if hidden(dom, *r) => {}
                Change::Added(r) => self.add_subtree(dom, *r, &mut ops),
                Change::Parent { referent, .. } if hidden(dom, *referent) => {
                    if self.known.contains(referent) {
                        self.forget(dom, *referent);
                        ops.push(Op::Remove(*referent));
                    }
                }
                Change::Parent { referent, .. } => {
                    if dom.get(*referent).is_none() {
                        continue;
                    }
                    if !self.known.contains(referent) {
                        self.add_subtree(dom, *referent, &mut ops);
                    } else if let Some(parent) = self.parent(dom, *referent) {
                        ops.push(Op::Move {
                            at: *referent,
                            parent,
                        });
                    }
                }
                Change::Removed(r) => {
                    if self.known.remove(r) {
                        ops.push(Op::Remove(*r));
                    }
                }
                Change::Class(r) => {
                    if let (Some(instance), true) = (dom.get(*r), self.known.contains(r)) {
                        ops.push(Op::Class {
                            at: *r,
                            class: instance.class().to_owned(),
                        });
                    }
                }
            }
        }
        ops
    }

    /// `core` when it differs from what was sent last.
    pub fn core(&mut self, core: CoreGui) -> Option<CoreGui> {
        if self.core.as_ref() == Some(&core) {
            return None;
        }
        self.core = Some(core.clone());
        Some(core)
    }

    /// Where `r` goes in the player's copy; `None` when its parent isn't
    /// there (it follows once the parent comes).
    fn parent(&self, dom: &WeakDom, r: Ref) -> Option<Parent> {
        match dom.parent(r) {
            Some(p) if self.known.contains(&p) => Some(Parent::Under(p)),
            Some(_) => None,
            None if dom.root_refs().contains(&r) => Some(Parent::Root),
            None => Some(Parent::Nil),
        }
    }

    fn add_subtree(&mut self, dom: &WeakDom, root: Ref, ops: &mut Vec<Op>) {
        let mut stack = vec![root];
        while let Some(at) = stack.pop() {
            let Some(instance) = dom.get(at) else {
                continue;
            };
            stack.extend(instance.children().iter().rev().copied());
            if self.known.contains(&at) {
                continue;
            }
            let Some(parent) = self.parent(dom, at) else {
                continue;
            };
            self.known.insert(at);
            ops.push(Op::Add {
                at,
                parent,
                class: instance.class().to_owned(),
                name: instance.name().to_owned(),
                props: props(instance),
            });
        }
    }

    fn forget(&mut self, dom: &WeakDom, root: Ref) {
        let mut stack = vec![root];
        while let Some(at) = stack.pop() {
            self.known.remove(&at);
            if let Some(instance) = dom.get(at) {
                stack.extend(instance.children().iter().copied());
            }
        }
    }
}

/// A remote player's copy of their client's DataModel.
pub struct Viewer {
    dom: Rc<RefCell<WeakDom>>,
}

impl Default for Viewer {
    fn default() -> Self {
        Viewer {
            dom: Rc::new(RefCell::new(WeakDom::new())),
        }
    }
}

fn place(dom: &mut WeakDom, at: Ref, parent: Parent) {
    match parent {
        Parent::Under(p) if dom.get(p).is_some() => dom.set_parent(at, Some(p)),
        Parent::Under(_) => {}
        Parent::Root => dom.set_parent(at, None),
        Parent::Nil => dom.unparent(at),
    }
}

impl Viewer {
    pub fn dom_rc(&self) -> Rc<RefCell<WeakDom>> {
        self.dom.clone()
    }

    /// Applies `frame`'s ops; the changes they made, for whatever shows
    /// the copy (a reset starts the copy over, and changes nothing).
    pub fn apply(&mut self, frame: &Frame) -> Vec<Change> {
        let mut dom = self.dom.borrow_mut();
        if frame.reset {
            *dom = WeakDom::new();
        }
        let mut changes = Vec::new();
        for op in &frame.ops {
            match op {
                Op::Add {
                    at,
                    parent,
                    class,
                    name,
                    props,
                } => {
                    if dom.get(*at).is_some() {
                        continue;
                    }
                    let mut instance = Instance::new(*at, class.as_str(), name.as_str());
                    instance.properties_mut().extend(props.iter().cloned());
                    dom.insert(instance);
                    place(&mut dom, *at, *parent);
                    changes.push(Change::Added(*at));
                }
                Op::Prop { at, name, value } => {
                    let done = match value {
                        Some(Variant::String(text)) if name == "Name" => {
                            dom.set_name(*at, text).is_ok()
                        }
                        Some(value) => dom.set_property(*at, name, value.clone()).is_ok(),
                        None => dom.remove_property(*at, name).is_ok(),
                    };
                    if done {
                        changes.push(Change::Property {
                            referent: *at,
                            name: name.clone(),
                        });
                    }
                }
                Op::Move { at, parent } => {
                    if dom.get(*at).is_some() {
                        let old = dom.parent(*at);
                        place(&mut dom, *at, *parent);
                        changes.push(Change::Parent {
                            referent: *at,
                            old,
                            new: dom.parent(*at),
                        });
                    }
                }
                Op::Remove(at) => {
                    changes.extend(dom.remove(*at).into_iter().map(Change::Removed));
                }
                Op::Class { at, class } => {
                    if dom.set_class(*at, class).is_ok() {
                        changes.push(Change::Class(*at));
                    }
                }
            }
        }
        dom.take_changes();
        if frame.reset {
            changes.clear();
        }
        changes
    }
}

/// An ask to join, waiting for the host.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub from: PeerId,
    pub name: String,
    pub user_id: i64,
    template: Option<Vec<u8>>,
}

/// A player in the session from another computer.
pub struct Guest {
    pub from: PeerId,
    pub player: Ref,
    encoder: Encoder,
    /// Sent their first frame (the whole tree).
    started: bool,
}

/// What a message from a remote player came to, for the editor.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A new ask to join, by this name.
    Offered(String),
    /// A remote player left.
    Left(Ref),
    /// A remote player closed the purchase dialog.
    Purchase { player: Ref, bought: bool },
}

/// The session's side of its remote players.
#[derive(Default)]
pub struct Host {
    pub offers: Vec<Offer>,
    pub guests: Vec<Guest>,
}

fn index_of(session: &Session, player: Ref) -> Option<usize> {
    session.clients.iter().position(|c| c.player == player)
}

/// What a remote player may do to their client: what their own window
/// would, nothing that runs the session (start, join, network, purchase
/// outcomes the host decides).
fn allowed(action: &Action) -> bool {
    matches!(
        action,
        Action::Input(_)
            | Action::Viewport(_)
            | Action::SafeInsets(_)
            | Action::MenuOpen(_)
            | Action::Chat(_)
            | Action::Reset
    )
}

impl Host {
    pub fn guest(&self, player: Ref) -> Option<&Guest> {
        self.guests.iter().find(|g| g.player == player)
    }

    /// One message from `from`: an ask is kept for the host to take, a
    /// guest's actions go straight to their client.
    pub fn receive(&mut self, session: &mut Session, from: PeerId, up: Up) -> Option<Event> {
        let guest = self
            .guests
            .iter()
            .find(|g| g.from == from)
            .map(|g| g.player);
        match (up, guest) {
            (
                Up::Ask {
                    name,
                    user_id,
                    template,
                },
                None,
            ) => {
                self.offers.retain(|o| o.from != from);
                self.offers.push(Offer {
                    from,
                    name: name.clone(),
                    user_id,
                    template,
                });
                Some(Event::Offered(name))
            }
            (Up::Actions(actions), Some(player)) => {
                let i = index_of(session, player)?;
                for action in actions.into_iter().filter(allowed) {
                    session.act(Peer::Client(i), action);
                }
                None
            }
            (Up::Purchase(bought), Some(player)) => Some(Event::Purchase { player, bought }),
            (Up::Leave, Some(player)) => {
                self.guests.retain(|g| g.player != player);
                if let Some(i) = index_of(session, player) {
                    session.leave(i);
                }
                Some(Event::Left(player))
            }
            (Up::Leave, None) => {
                self.offers.retain(|o| o.from != from);
                None
            }
            (Up::Resync, Some(_)) => {
                self.resync(Some(from));
                None
            }
            _ => None,
        }
    }

    /// Lets the ask from `from` in: they join the session wearing the
    /// avatar they sent. Their client's index and their welcome.
    pub fn accept(&mut self, session: &mut Session, from: PeerId) -> Result<(usize, Down), String> {
        let at = self
            .offers
            .iter()
            .position(|o| o.from == from)
            .ok_or("nobody asks to join from there")?;
        let offer = self.offers.remove(at);
        let template = match &offer.template {
            Some(bytes) => Some(rbx_binary::deserialize(bytes).map_err(|e| {
                format!(
                    "{}\u{2019}s avatar couldn\u{2019}t be read: {e}",
                    offer.name
                )
            })?),
            None => None,
        };
        let i = session.join(&offer.name, offer.user_id, template)?;
        let client = &mut session.clients[i];
        client.sim.watch_view_changes();
        let player = client.player;
        let name = client.name.clone();
        self.guests.push(Guest {
            from,
            player,
            encoder: Encoder::default(),
            started: false,
        });
        Ok((i, Down::Welcome { player, name }))
    }

    /// Turns the ask from `from` down; the answer to send them.
    pub fn decline(&mut self, from: PeerId) -> Down {
        self.offers.retain(|o| o.from != from);
        Down::Ended("The host didn\u{2019}t let you in.".into())
    }

    /// `frame` for `player`'s editor, if they're remote: the ops for
    /// `changes` (the whole tree the first time) filled in, the core GUI
    /// dropped when it didn't change.
    pub fn frame(
        &mut self,
        player: Ref,
        dom: &WeakDom,
        changes: &[Change],
        mut frame: Frame,
    ) -> Option<(PeerId, Down)> {
        let guest = self.guests.iter_mut().find(|g| g.player == player)?;
        frame.reset = !guest.started;
        frame.ops = if guest.started {
            guest.encoder.ops(dom, changes)
        } else {
            guest.encoder.snapshot(dom)
        };
        guest.started = true;
        frame.core = frame.core.and_then(|core| guest.encoder.core(core));
        Some((guest.from, Down::Frame(Box::new(frame))))
    }

    /// The next frame to `from` (`None`: to every guest, after the host's
    /// own connection dropped) is the whole tree again.
    pub fn resync(&mut self, from: Option<PeerId>) {
        for guest in &mut self.guests {
            if from.is_none_or(|f| f == guest.from) {
                guest.started = false;
            }
        }
    }

    /// `player` is gone (left, kicked, their tab closed): whom to tell.
    pub fn drop_player(&mut self, player: Ref) -> Option<PeerId> {
        let at = self.guests.iter().position(|g| g.player == player)?;
        Some(self.guests.remove(at).from)
    }

    /// The session ended: everyone waiting or playing, to tell.
    pub fn end(&mut self) -> Vec<PeerId> {
        let mut out: Vec<PeerId> = self.guests.drain(..).map(|g| g.from).collect();
        out.extend(self.offers.drain(..).map(|o| o.from));
        out
    }
}

#[cfg(test)]
mod tests;
