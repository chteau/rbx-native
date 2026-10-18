//! Live co-editing (Beta Features › Live co-editing): this editor's DOM
//! edits go to the hosted room's op log, and everyone's come back and are
//! applied, through `rbx_presence::coedit` (its module doc has the design:
//! identity, last-writer-wins, script text, catch-up).
//!
//! Local edits are found by comparing the DOM with the copy last synced
//! (`shadow`) whenever the undo history or the DOM's change counter moved,
//! so every way the editor mutates the place is covered without hooking
//! each one. Remote edits go through `WeakDom`'s own mutation methods and
//! the same undo and viewport path as any edit:
//!
//! - **Undo.** A batch of remote edits is an undo entry of its own, so
//!   Ctrl+Z takes back the last thing that happened to the place, whoever
//!   did it. Whatever an undo or redo changes is then sent like any other
//!   edit, so taking back a collaborator's edit takes it back for everyone.
//! - **Transport.** Only the hosted relay keeps an op log; on a Worker or
//!   ntfy room co-editing stays off and says so once.

use std::sync::mpsc::Receiver;

use gpui_kit::Context;
use rbx_dom::WeakDom;
use rbx_presence::client::Route;
use rbx_presence::coedit::channel::{self, Channel, Event};
use rbx_presence::coedit::engine::Engine;
use rbx_presence::coedit::ids::IdMap;
use rbx_presence::coedit::{diff, Batch};
use rbx_presence::relay::RelayKind;
use rbx_presence::schema::Client as Editor;

use super::Shell;

/// The `beta.json` id that switches co-editing on.
pub(crate) const BETA_ID: &str = "coedit";

/// Where the room is and who this editor is in it.
pub(super) struct Seed {
    pub(super) kind: RelayKind,
    pub(super) room: String,
    pub(super) key: String,
    pub(super) enc_key: [u8; 32],
    pub(super) user_id: u64,
}

struct Link {
    url: String,
    channel: Channel,
    events: Receiver<Event>,
}

pub(super) struct Coedit {
    seed: Seed,
    actor: String,
    engine: Engine,
    ids: IdMap,
    /// The DOM as last synced: what local edits are found against.
    shadow: WeakDom,
    /// `(history revision, DOM change count)` when `shadow` was taken.
    trigger: (u64, u64),
    link: Option<Link>,
    /// The warnings said once per session.
    told_gap: bool,
    told_transport: bool,
}

impl Shell {
    /// Starts co-editing for a freshly connected room, when the beta is on.
    pub(super) fn coedit_start(&mut self, seed: Seed) {
        if !crate::beta::enabled(BETA_ID) {
            return;
        }
        // A random word per session: the same user in two editors is two
        // actors, or each would take the other's edits for its own echo.
        let session =
            std::hash::BuildHasher::hash_one(&std::collections::hash_map::RandomState::new(), 0u8)
                as u32;
        let actor = format!("{}:rbx-native:{session:08x}", seed.user_id);
        let coedit = Coedit {
            engine: Engine::new(actor.clone()),
            actor,
            seed,
            ids: IdMap::build(&self.dom),
            shadow: self.dom.clone(),
            trigger: (self.history.revision(), self.dom.mutations()),
            link: None,
            told_gap: false,
            told_transport: false,
        };
        self.team_presence.coedit = Some(coedit);
    }

    /// Follows the hosted room to wherever its relay is now: a new address
    /// is a new log, read from its start.
    pub(super) fn coedit_route(&mut self, route: &Route) {
        let Some(coedit) = self.team_presence.coedit.as_mut() else {
            return;
        };
        let url = match route {
            Route::Hosting(url) | Route::Joined(url) => url.trim_end_matches('/').to_owned(),
            Route::Waiting => {
                coedit.link = None;
                return;
            }
        };
        if coedit.link.as_ref().is_some_and(|link| link.url == url) {
            return;
        }
        let (channel, events) = Channel::start(channel::Config {
            relay_url: url.clone(),
            room: coedit.seed.room.clone(),
            key: coedit.seed.key.clone(),
            enc_key: coedit.seed.enc_key,
            user_id: coedit.seed.user_id,
            client: Editor::RbxNative,
        });
        coedit.link = Some(Link {
            url,
            channel,
            events,
        });
    }

    /// Sends what changed here, then applies what arrived, every presence
    /// tick.
    pub(super) fn coedit_tick(&mut self, cx: &mut Context<Self>) {
        // During Play the DOM is the live game's, which is never synced.
        if self.live_shown() {
            return;
        }
        let Some(mut coedit) = self.team_presence.coedit.take() else {
            return;
        };
        if coedit.seed.kind != RelayKind::Hosted && !coedit.told_transport {
            coedit.told_transport = true;
            self.output.push_warning(
                "Live co-editing needs the hosted relay (the default); this place names another, so edits are not shared.",
            );
        }
        let mut incoming = Vec::new();
        if let Some(link) = &coedit.link {
            while let Ok(event) = link.events.try_recv() {
                match event {
                    Event::Batch { batch, .. } => incoming.push(batch),
                    Event::Gap if !coedit.told_gap => {
                        coedit.told_gap = true;
                        self.output.push_warning(
                            "Co-editing: the relay no longer holds some edits made before you joined. Reopen the place from its latest save to be sure you have them.",
                        );
                    }
                    Event::Gap | Event::Error(_) => {}
                }
            }
        }
        // Typing still on its debounce is a local edit too, and must be
        // sent (and be merge input) before anything remote lands on it.
        if !incoming.is_empty() {
            self.flush_script_edits(cx);
        }
        self.coedit_send_local(&mut coedit);
        for batch in &incoming {
            self.coedit_apply(&mut coedit, batch, cx);
        }
        self.team_presence.coedit = Some(coedit);
    }

    fn coedit_send_local(&mut self, coedit: &mut Coedit) {
        let trigger = (self.history.revision(), self.dom.mutations());
        // Unlinked, edits wait in the difference from `shadow` until there is
        // a log to send them to.
        if trigger == coedit.trigger || coedit.link.is_none() {
            return;
        }
        let ops = diff::local_ops(
            &coedit.shadow,
            &mut self.dom,
            &mut coedit.ids,
            &mut coedit.engine,
        );
        coedit.send(ops);
        coedit.shadow = self.dom.clone();
        coedit.trigger = (self.history.revision(), self.dom.mutations());
    }

    fn coedit_apply(&mut self, coedit: &mut Coedit, batch: &Batch, cx: &mut Context<Self>) {
        // Already reflected; it belongs to nothing this step undoes (the
        // same reason `push_history` drains it).
        self.dom.take_changes();
        let before = self.dom.clone();
        let report = coedit.engine.apply(&mut self.dom, &mut coedit.ids, batch);
        let changes = self.dom.take_changes();
        if !changes.is_empty() {
            self.push_history_snapshot(before);
            self.record_history_change(changes.clone());
            self.rebuild_explorer(cx);
            self.reflect_changes(&changes, cx);
        }
        for note in &report.notes {
            self.output.push_warning(&format!("Co-editing: {note}"));
        }
        coedit.send(report.resend);
        coedit.shadow = self.dom.clone();
        coedit.trigger = (self.history.revision(), self.dom.mutations());
    }
}

impl Coedit {
    fn send(&self, ops: Vec<rbx_presence::coedit::Op>) {
        if ops.is_empty() {
            return;
        }
        if let Some(link) = &self.link {
            link.channel.send(Batch {
                a: self.actor.clone(),
                ops,
            });
        }
    }
}

// The shell half needs a window; what it does with the undo history is
// tested here on the same pieces it calls: two editors' DOMs, a real
// `History`, the diff and the engine.
#[cfg(test)]
mod tests {
    use rbx_dom::{Variant, WeakDom};
    use rbx_presence::coedit::engine::Engine;
    use rbx_presence::coedit::ids::IdMap;
    use rbx_presence::coedit::{diff, Batch};

    use crate::history::{History, DEFAULT_CAP};

    struct Editor {
        dom: WeakDom,
        shadow: WeakDom,
        ids: IdMap,
        engine: Engine,
        history: History,
    }

    impl Editor {
        fn new(dom: &WeakDom, actor: &str) -> Editor {
            Editor {
                dom: dom.clone(),
                shadow: dom.clone(),
                ids: IdMap::build(dom),
                engine: Engine::new(actor.into()),
                history: History::new(DEFAULT_CAP),
            }
        }

        /// `Shell::coedit_send_local`.
        fn send(&mut self) -> Batch {
            let ops = diff::local_ops(&self.shadow, &mut self.dom, &mut self.ids, &mut self.engine);
            self.shadow = self.dom.clone();
            Batch {
                a: self.engine.actor().to_owned(),
                ops,
            }
        }

        /// `Shell::coedit_apply`: one undo entry per batch that changed something.
        fn apply(&mut self, batch: &Batch) {
            self.dom.take_changes();
            let before = self.dom.clone();
            self.engine.apply(&mut self.dom, &mut self.ids, batch);
            let changes = self.dom.take_changes();
            if !changes.is_empty() {
                self.history.push(before);
                self.history.record_changes(changes);
            }
            self.shadow = self.dom.clone();
        }

        fn undo(&mut self) {
            let (previous, _) = self.history.undo(self.dom.clone()).expect("an entry");
            self.dom = previous;
        }
    }

    fn place() -> (WeakDom, rbx_dom::Ref) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "Part", Some(workspace));
        dom.set_property(part, "Transparency", Variant::Float32(0.0))
            .unwrap();
        (dom, part)
    }

    fn transparency(dom: &WeakDom, part: rbx_dom::Ref) -> Option<Variant> {
        dom.get(part)?.properties().get("Transparency").cloned()
    }

    #[test]
    fn a_remote_edit_is_its_own_undo_step_and_undoing_it_is_shared() {
        let (dom, part) = place();
        let mut ana = Editor::new(&dom, "1:rbx-native");
        let mut ben = Editor::new(&dom, "2:rbx-native");

        ana.dom
            .set_property(part, "Transparency", Variant::Float32(0.5))
            .unwrap();
        let edit = ana.send();
        ana.apply(&edit); // the log echoes it back: no change, no undo entry
        ben.apply(&edit);
        assert_eq!(transparency(&ben.dom, part), Some(Variant::Float32(0.5)));
        assert!(
            ana.history.undo(ana.dom.clone()).is_none(),
            "own echo is not a step"
        );

        ben.undo();
        assert_eq!(transparency(&ben.dom, part), Some(Variant::Float32(0.0)));
        let undo = ben.send();
        assert!(!undo.ops.is_empty(), "an undo is sent like any edit");
        ana.apply(&undo);
        ben.apply(&undo);
        assert_eq!(transparency(&ana.dom, part), Some(Variant::Float32(0.0)));
        assert_eq!(transparency(&ben.dom, part), Some(Variant::Float32(0.0)));
    }
}
