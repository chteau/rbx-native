//! Model › Run (local): the place runs live on `rbx_sim`, off a copy of it.
//! The 3D viewport, Explorer and Properties show the simulation moving, the
//! Output streams what scripts print, Pause holds it and Stop puts the
//! edited place back exactly as it was. Behind its beta switch.
//!
//! Play (local) is the same run with a local player: their character (see
//! `avatars`) walks on WASD, jumps on Space, and the camera follows it,
//! turned by right-drag and zoomed by the wheel.
//!
//! While it runs `Shell::dom` holds a mirror of the simulation's DOM and the
//! edited place sits in the run's stash (see `play::live` for the same swap
//! during Play). Each frame's changes are copied from the simulation's DOM
//! into the mirror; edits made to the mirror are copied the other way into
//! the simulation, logged, so it routes them on its next step.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::*;
use rbx_dom::{Change, Ref, Variant, WeakDom};

use super::output::game::Context as OutputContext;
use super::play::Phase;
use super::Shell;

mod avatars;
pub(crate) mod badges;
pub(crate) mod data;
mod live_prompt;
mod offline;
pub(in crate::shell) mod owners;
mod remote;
mod replay;
mod sound;
pub(crate) mod tabs;
pub(crate) mod windows;
use crate::command_bar::Feedback;
use crate::history::{History, DEFAULT_CAP};
use crate::properties::Properties;
use crate::workspace_view::core_gui::CoreAction;
use rbx_sim::remote::Down;
use rbx_sim::replay::Action;

pub(crate) const BETA_ID: &str = "local-run";
const FRAME_HZ: f64 = 60.0;
/// Frames one tick may step, so a slow frame never spirals.
const MAX_FRAMES_PER_TICK: u64 = 4;
const TICK: Duration = Duration::from_millis(8);

/// The edited place's state while the simulation's copy is shown.
struct Stash {
    dom: WeakDom,
    history: History,
    selection: Vec<Ref>,
}

struct Running {
    session: rbx_sim::Session,
    /// Which peer's DataModel the Explorer, Properties and viewport show;
    /// a client view gets the keys and moves the camera.
    view: rbx_sim::Peer,
    stash: Stash,
    paused: bool,
    /// Step while paused: the next tick runs exactly one frame.
    step_once: bool,
    /// Wall seconds the run has been unpaused for.
    clock: f64,
    last: Instant,
    /// Dropping it stops the frame loop.
    task: Option<Task<()>>,
    /// Play (local): a local player whose character the keys drive.
    playing: bool,
    /// Play (local)'s players are queued to join (`Session::join_later`).
    joining: bool,
    /// What the viewed peer hears.
    sound: sound::Mixer,
    /// Purchase prompts waiting for the test purchase dialog, the shown
    /// one first.
    prompts: std::collections::VecDeque<rbx_lua::server::PurchasePrompt>,
    /// Who Play (local) plays as, for a teleport's restart.
    player: Option<avatars::Player>,
    /// `rbx_dom::editable::generation` as of the last frame shown.
    editables: u64,
    /// The Controller Emulator's pad as the viewed client last heard it.
    pad: Option<rbx_play::gamepad::PadState>,
    /// Playing a recording back instead of taking input.
    replay: Option<replay::ReplayState>,
    /// Server & Clients: the session shows in a test window, a tab per
    /// peer (see `windows`), this window keeps the edited place.
    windowed: bool,
    windows: Vec<windows::PeerWindow>,
    test_window: Option<windows::TestWindowHandle>,
    /// The player whose window last had input: what the run plays aloud
    /// and the Controller Emulator drives.
    audible: Option<Ref>,
    /// Server & Clients: players from collaborators' editors.
    remote: rbx_sim::remote::Host,
    /// Collaborators a teleport brought, to join once the local players
    /// have.
    carried: Vec<rbx_sim::remote::Offer>,
    /// What the Remote inspector captured, shared with every test tab's
    /// dock.
    remotes: crate::shell::remote_inspector::Shared,
}

/// What the next run starts from after a teleport: the place it lands in
/// (`None`: the edited one) and what each player brings.
struct Arriving {
    place: Option<WeakDom>,
    arrivals: std::collections::HashMap<i64, rbx_lua::server::Arrival>,
    guis: Vec<(i64, WeakDom)>,
    /// Collaborators playing from their own editors, coming along.
    carried: Vec<rbx_sim::remote::Offer>,
}

pub(crate) struct LocalRun {
    running: Option<Running>,
    /// Play (local) is fetching the avatar.
    pub(in crate::shell) starting: bool,
    /// Test › Players: how many local players Play (local) starts with.
    pub(in crate::shell) players: usize,
    /// A teleport's landing, taken by the run it restarts.
    arriving: Option<Arriving>,
    /// Test › Offline: the open place's offline pack.
    offline: offline::Offline,
    /// The last run's recording, for Replay and Save recording.
    recording: Option<rbx_sim::replay::Recording>,
    /// The replay transport's scrub rail.
    rail: Option<Entity<gpui_kit::component::slider::SliderState>>,
    /// In a Server & Clients test window: the session it shows.
    pub(in crate::shell) peer: Option<windows::PeerLink>,
    /// The `PlayerGui` this window's viewport draws (see
    /// `WorkspaceView::set_player_gui`).
    shown_gui: Option<Ref>,
    /// Collaborators playing this editor's session, or this editor playing
    /// another's (see `remote`).
    pub(in crate::shell) remote: remote::RemotePlay,
    /// A start waiting on the live DataStores question (see `live_prompt`).
    live_prompt: Option<live_prompt::Pending>,
}

impl Default for LocalRun {
    fn default() -> Self {
        LocalRun {
            running: None,
            starting: false,
            players: 1,
            arriving: None,
            offline: offline::Offline::default(),
            recording: None,
            rail: None,
            peer: None,
            shown_gui: None,
            remote: remote::RemotePlay::default(),
            live_prompt: None,
        }
    }
}

/// What the Controller Emulator's pad going from `old` to `new` is to a
/// client: Gamepad1 plugged in or out, its buttons, sticks and triggers.
fn pad_inputs(
    old: Option<rbx_play::gamepad::PadState>,
    new: Option<rbx_play::gamepad::PadState>,
) -> Vec<rbx_sim::Input> {
    use rbx_play::gamepad::{Button, Side};
    use rbx_sim::Input;
    let mut out = Vec::new();
    if old.is_some() != new.is_some() {
        out.push(Input::Gamepad {
            pad: 1,
            connected: new.is_some(),
        });
    }
    let Some(new) = new else {
        return out;
    };
    let before = old.unwrap_or_default();
    for button in Button::ALL {
        let code = match button {
            Button::South => "ButtonA",
            Button::East => "ButtonB",
            Button::West => "ButtonX",
            Button::North => "ButtonY",
            Button::LeftBumper => "ButtonL1",
            Button::RightBumper => "ButtonR1",
            Button::Select => "ButtonSelect",
            Button::Start => "ButtonStart",
            Button::LeftStick => "ButtonL3",
            Button::RightStick => "ButtonR3",
            Button::DpadUp => "DPadUp",
            Button::DpadDown => "DPadDown",
            Button::DpadLeft => "DPadLeft",
            Button::DpadRight => "DPadRight",
            Button::Home => continue,
        };
        if new.pressed(button) != before.pressed(button) {
            out.push(Input::GamepadButton {
                pad: 1,
                code: code.into(),
                down: new.pressed(button),
            });
        }
    }
    for (side, stick, trigger) in [
        (Side::Left, "Thumbstick1", "ButtonL2"),
        (Side::Right, "Thumbstick2", "ButtonR2"),
    ] {
        if new.stick(side) != before.stick(side) {
            let (x, y) = new.stick(side);
            out.push(Input::GamepadAxis {
                pad: 1,
                code: stick.into(),
                position: [x, y, 0.0],
            });
        }
        if new.trigger(side) != before.trigger(side) {
            out.push(Input::GamepadAxis {
                pad: 1,
                code: trigger.into(),
                position: [0.0, 0.0, new.trigger(side)],
            });
        }
    }
    out
}

/// The most local players a test runs (Studio's Server & Clients count
/// goes to 8).
pub(crate) const MAX_PLAYERS: usize = 8;

/// Animations (the default ones, the catalog's) from Roblox's asset
/// delivery; they land in the asset cache for the next run. An offline
/// pack downloads through it too.
pub(in crate::shell) struct Fetcher(pub(in crate::shell) rbx_cloud::Client);

impl rbx_assets::AssetFetcher for Fetcher {
    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, rbx_assets::FetchError> {
        self.0
            .asset(id)
            .map(|content| content.bytes)
            .map_err(|err| rbx_assets::FetchError::Other {
                id,
                message: err.to_string(),
            })
    }

    fn fetch_thumb(&self, spec: &str) -> Result<Vec<u8>, rbx_assets::FetchError> {
        self.0
            .rbxthumb(spec)
            .map_err(|err| rbx_assets::FetchError::Other {
                id: 0,
                message: err.to_string(),
            })
    }
}

/// The editor's own downloads for `AssetService` in the command bar and
/// plugins, with whatever key is set when the call comes.
pub(in crate::shell) struct EditFetcher;

impl rbx_assets::AssetFetcher for EditFetcher {
    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, rbx_assets::FetchError> {
        Fetcher(rbx_cloud::Client::new(
            rbx_cloud::ApiKey::from_env_or_config(),
        ))
        .fetch_id(id)
    }

    fn fetch_thumb(&self, spec: &str) -> Result<Vec<u8>, rbx_assets::FetchError> {
        Fetcher(rbx_cloud::Client::new(
            rbx_cloud::ApiKey::from_env_or_config(),
        ))
        .fetch_thumb(spec)
    }
}

impl LocalRun {
    /// A session runs here, or this is one of its test windows.
    pub(in crate::shell) fn active(&self) -> bool {
        self.running.is_some() || self.peer.is_some()
    }

    pub(in crate::shell) fn paused(&self) -> bool {
        self.running.as_ref().is_some_and(|run| run.paused)
            || self.peer.as_ref().is_some_and(|peer| peer.paused)
    }

    /// The edited place, while the simulation's copy is shown.
    pub(in crate::shell) fn edited(&self) -> Option<&WeakDom> {
        self.running
            .as_ref()
            .filter(|run| !run.windowed)
            .map(|run| &run.stash.dom)
    }

    /// This window shows a peer of a running session rather than the
    /// edited place.
    pub(in crate::shell) fn shown(&self) -> bool {
        self.running.as_ref().is_some_and(|run| !run.windowed) || self.peer.is_some()
    }
}

/// Play (local)'s players, you then the extra test players (noobs,
/// numbered as Studio numbers its test players), queued to join once the
/// server ran its first moments: the frame loop's ticks join them
/// (`local_run_joined`), the editor never waits for them.
fn queue_players(
    session: &mut rbx_sim::Session,
    name: &str,
    user_id: i64,
    character: Option<WeakDom>,
    players: usize,
) {
    session.join_later(name, user_id, character);
    for n in 2..=players {
        session.join_later(&format!("Player{n}"), -(n as i64), None);
    }
}

/// Output's line for Server & Clients, whose players are Studio's test
/// players, `Player1` to `Player{count}`, all wearing the avatar `note`
/// says was resolved (`name`'s, or the noob and why).
fn test_players_note(count: usize, name: &str, note: &str) -> String {
    let players = match count {
        1 => "Player1".to_owned(),
        n => format!("Player1 to Player{n}"),
    };
    if note == format!("Playing as {name}") {
        format!("Playing as {players}, wearing {name}\u{2019}s avatar")
    } else {
        format!("Playing as {players}. {note}")
    }
}

/// Puts `running` into `dom`'s place and returns the edited place it replaced.
fn stash_edited(dom: &mut WeakDom, running: WeakDom) -> WeakDom {
    std::mem::replace(dom, running)
}

/// Puts the edited place back, with a clean change log.
fn restore_edited(dom: &mut WeakDom, edited: WeakDom) {
    *dom = edited;
    dom.take_changes();
}

/// Copies `changes` from `from` into `into`: a property by value (or its
/// removal), an instance with its subtree and referents, a removal, a move,
/// a class. Writes are logged in `into`.
fn mirror(from: &WeakDom, into: &mut WeakDom, changes: &[Change]) {
    for change in changes {
        match change {
            Change::Property { referent, name } => {
                let (Some(source), true) = (from.get(*referent), into.get(*referent).is_some())
                else {
                    continue;
                };
                if name == "Name" {
                    let _ = into.set_name(*referent, source.name());
                } else if let Some(value) = source.properties().get(name.as_str()) {
                    let _ = into.set_property(*referent, name, value.clone());
                } else {
                    let _ = into.remove_property(*referent, name);
                }
            }
            // An instance outside the game's tree (an AnimationTrack, a script's
            // `Instance.new` not parented yet or under one that isn't, an old
            // character) stays out of the shown one.
            Change::Added(referent) if hidden(from, *referent) => {}
            Change::Added(referent) => copy_subtree(from, into, *referent),
            Change::Parent { referent, .. } if hidden(from, *referent) => {
                into.remove(*referent);
            }
            Change::Parent { referent, .. } => {
                if from.get(*referent).is_none() {
                    continue;
                }
                if into.get(*referent).is_none() {
                    copy_subtree(from, into, *referent);
                } else {
                    place(from, into, *referent);
                }
            }
            Change::Removed(referent) => {
                into.remove(*referent);
            }
            Change::Class(referent) => {
                if let (Some(source), true) = (from.get(*referent), into.get(*referent).is_some()) {
                    let _ = into.set_class(*referent, source.class());
                }
            }
        }
    }
}

/// Whether `referent` is parentless (an engine-made `InputObject`, a
/// script's `Instance.new`) or under something that is.
fn hidden(from: &WeakDom, referent: Ref) -> bool {
    from.parent(referent).is_none() || !from.in_tree(referent)
}

/// Inserts `root` and everything under it that `into` lacks, parents as in
/// `from`.
fn copy_subtree(from: &WeakDom, into: &mut WeakDom, root: Ref) {
    let mut stack = vec![root];
    while let Some(at) = stack.pop() {
        let Some(source) = from.get(at) else {
            continue;
        };
        stack.extend(source.children().iter().rev().copied());
        if into.get(at).is_some() {
            continue;
        }
        let mut copy = rbx_dom::Instance::new(at, source.class(), source.name());
        *copy.properties_mut() = source.properties().clone();
        into.insert(copy);
        place(from, into, at);
    }
}

/// Gives `referent` in `into` the parent it has in `from`.
fn place(from: &WeakDom, into: &mut WeakDom, referent: Ref) {
    match from.parent(referent) {
        Some(parent) if into.get(parent).is_some() => into.set_parent(referent, Some(parent)),
        Some(_) => {}
        None if from.root_refs().contains(&referent) => into.set_parent(referent, None),
        None => into.unparent(referent),
    }
}

/// Every instance under `dom`'s roots, parents before children.
fn all_refs(dom: &WeakDom) -> Vec<Ref> {
    let mut out = Vec::new();
    let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();
    while let Some(at) = stack.pop() {
        if let Some(instance) = dom.get(at) {
            out.push(at);
            stack.extend(instance.children().iter().rev().copied());
        }
    }
    out
}

/// Every property bound to a script's `EditableImage`/`EditableMesh` that
/// changed since `dom`'s last call, as changes: a script painted, so the
/// viewport has to look again, though nothing in the DOM moved. A mesh
/// left alone isn't flattened again (OpenArena rebuilds a few of its many
/// meshes a frame).
pub(in crate::shell) fn editable_bindings(dom: &WeakDom) -> Vec<Change> {
    thread_local! {
        static SHOWN: std::cell::RefCell<HashMap<usize, HashMap<Ref, u64>>> = Default::default();
    }
    SHOWN.with_borrow_mut(|shown| {
        let shown = shown.entry(dom as *const WeakDom as usize).or_default();
        let mut now = HashMap::new();
        let mut out = Vec::new();
        for referent in all_refs(dom) {
            let Some(instance) = dom.get(referent) else {
                continue;
            };
            for (name, value) in instance.properties() {
                let Variant::Content(rbx_dom::Content::Object(object)) = value else {
                    continue;
                };
                let Some(editable) = rbx_dom::editable::get(*object) else {
                    continue;
                };
                let version = editable.version();
                now.insert(*object, version);
                if shown.get(object) != Some(&version) {
                    out.push(Change::Property {
                        referent,
                        name: name.clone(),
                    });
                }
            }
        }
        *shown = now;
        out
    })
}

/// The changes that turn `from` into `to`, matched by referent: what the
/// viewport needs to go back from the run to the edited place.
fn changes_between(from: &WeakDom, to: &WeakDom) -> Vec<Change> {
    let mut changes = Vec::new();
    for referent in all_refs(from) {
        if to.get(referent).is_none() {
            changes.push(Change::Removed(referent));
        }
    }
    for referent in all_refs(to) {
        let Some(after) = to.get(referent) else {
            continue;
        };
        let Some(before) = from.get(referent) else {
            changes.push(Change::Added(referent));
            continue;
        };
        if from.parent(referent) != to.parent(referent) {
            changes.push(Change::Parent {
                referent,
                old: from.parent(referent),
                new: to.parent(referent),
            });
        }
        if before.class() != after.class() {
            changes.push(Change::Class(referent));
        }
        if before.name() != after.name() {
            changes.push(Change::Property {
                referent,
                name: "Name".to_owned(),
            });
        }
        let names: HashSet<&str> = before
            .properties()
            .keys()
            .chain(after.properties().keys())
            .map(|name| name.as_ref())
            .collect();
        for name in names {
            if before.properties().get(name) != after.properties().get(name) {
                changes.push(Change::Property {
                    referent,
                    name: name.to_owned(),
                });
            }
        }
    }
    changes
}

/// `changes` without a repeated write to the same property.
fn dedupe(changes: Vec<Change>) -> Vec<Change> {
    let mut seen: HashSet<(Ref, String)> = HashSet::new();
    changes
        .into_iter()
        .filter(|change| match change {
            Change::Property { referent, name } => seen.insert((*referent, name.clone())),
            _ => true,
        })
        .collect()
}

fn structural(changes: &[Change]) -> bool {
    changes.iter().any(|change| match change {
        Change::Property { name, .. } => name == "Name",
        _ => true,
    })
}

/// What each side of the run printed, as game rows like real Play's:
/// tagged Server or Client, or Local Run for a run with no players.
fn push_rows(
    log: &mut super::output::OutputLog,
    outputs: Vec<(rbx_sim::Peer, rbx_lua::Output)>,
    playing: bool,
) {
    for (peer, output) in outputs {
        let context = match peer {
            _ if !playing => OutputContext::LocalRun,
            rbx_sim::Peer::Server => OutputContext::Server,
            rbx_sim::Peer::Client(_) => OutputContext::Client,
        };
        for row in rows_of(&output) {
            log.push_game(context, row, Vec::new(), None);
        }
    }
}

/// The name a view goes by: Server, or the player's.
pub(crate) fn view_label(session: &rbx_sim::Session, view: rbx_sim::Peer) -> String {
    match view {
        rbx_sim::Peer::Server => "Server".to_owned(),
        rbx_sim::Peer::Client(i) => session
            .clients
            .get(i)
            .map_or_else(|| format!("Player{}", i + 1), |c| c.name.clone()),
    }
}

/// One row of the run's own, tagged "Local Run" like a game row.
/// The test purchase dialog's title and line for `prompt`.
fn purchase_text(
    session: &rbx_sim::Session,
    prompt: &rbx_lua::server::PurchasePrompt,
) -> (String, String) {
    let who = session
        .clients
        .iter()
        .find(|c| c.player == prompt.player)
        .map_or_else(|| "A player".to_owned(), |c| c.name.clone());
    let what = match prompt.kind.as_str() {
        "Product" => format!("developer product {}", prompt.id),
        "GamePass" => format!("game pass {}", prompt.id),
        "Bundle" => format!("bundle {}", prompt.id),
        "Subscription" => format!("subscription {}", prompt.subscription),
        "GroupService.PromptJoinAsync" => {
            return (
                "Join group (test)".to_owned(),
                format!(
                    "{who} is asked to join group {}. This is a test: nobody joins anything.",
                    prompt.id
                ),
            )
        }
        "MarketplaceService.PromptBulkPurchase" => {
            format!("{} avatar items ({})", prompt.id, prompt.subscription)
        }
        "MarketplaceService.PromptRobloxSubscriptionPurchase" => "a Roblox subscription".to_owned(),
        "MarketplaceService.PromptRobuxTransferAsync" => {
            let receiver = prompt.subscription.split(',').nth(1).unwrap_or("?");
            return (
                "Robux transfer (test)".to_owned(),
                format!(
                    "{who} sends {} Robux to user {receiver}. This is a test: no Robux move.",
                    prompt.id
                ),
            );
        }
        "MarketplaceService.PromptCancelSubscription" => {
            return (
                "Cancel subscription (test)".to_owned(),
                format!(
                    "{who} is asked to cancel subscription {}. This is a test: nothing is charged or refunded.",
                    prompt.subscription
                ),
            );
        }
        kind if kind.starts_with("AvatarEditorService.") => {
            let what = match kind.trim_start_matches("AvatarEditorService.Prompt") {
                "SaveAvatar" => "save this avatar",
                "CreateOutfit" => "create an outfit",
                "DeleteOutfit" => "delete an outfit",
                "RenameOutfit" => "rename an outfit",
                "UpdateOutfit" => "update an outfit",
                "SetFavorite" => "change a favorite",
                _ => "let the game read their inventory",
            };
            return (
                "Avatar (test)".to_owned(),
                format!(
                    "The game asks {who} to {what}. This is a test: nothing changes on the account."
                ),
            );
        }
        _ => format!("item {}", prompt.id),
    };
    (
        "Test purchase".to_owned(),
        format!("{who} wants to buy {what}. This is a test: nothing is charged."),
    )
}

pub(in crate::shell) fn push_row(log: &mut super::output::OutputLog, row: Feedback) {
    log.push_game(OutputContext::LocalRun, row, Vec::new(), None);
}

fn rows_of(output: &rbx_lua::Output) -> Vec<Feedback> {
    output
        .lines()
        .iter()
        .zip(output.kinds())
        .map(|(line, kind)| match kind {
            rbx_lua::LineKind::Print => Feedback::Output(line.clone()),
            rbx_lua::LineKind::Warn => Feedback::Warning(line.clone()),
            rbx_lua::LineKind::Error => Feedback::Error(line.clone()),
        })
        .collect()
}

/// The Network Simulator's values as Local Run's link: inbound is what
/// reaches a client, outbound what it sends.
fn local_network(conditions: &rbx_play::netsim::NetConditions) -> rbx_sim::link::Network {
    let way = |w: rbx_play::netsim::OneWay| rbx_sim::link::Way {
        latency: w.latency_ms / 1000.0,
        jitter: w.jitter_ms / 1000.0,
        loss: w.loss_percent / 100.0,
    };
    rbx_sim::link::Network {
        down: way(conditions.inbound),
        up: way(conditions.outbound),
    }
}

impl Shell {
    /// A Local Run or Play (local) is going.
    pub(super) fn local_run_running(&self) -> bool {
        self.play.live.local.running.is_some()
    }

    /// The Network Simulator's applied values reach the running Local Run
    /// at once; false when none is running.
    pub(super) fn local_run_set_network(
        &mut self,
        conditions: &rbx_play::netsim::NetConditions,
    ) -> bool {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return false;
        };
        run.session.set_network(local_network(conditions));
        true
    }

    /// This window shows a run: Play (local)'s, or a test window's peer.
    pub(super) fn local_run_shown(&self) -> bool {
        self.play.live.local.shown()
    }

    fn local_run_can_start(&self) -> bool {
        crate::beta::enabled(BETA_ID)
            && !self.local_run_active()
            && !self.play.live.local.starting
            && self.play.phase == Phase::Idle
    }

    pub(crate) fn run_local(&mut self, cx: &mut Context<Self>) {
        if self.local_run_can_start() && !self.live_data_asks(live_prompt::Pending::Run, cx) {
            self.start_local(None, None, cx);
        }
    }

    /// `RBX_STUDIO_LOCAL_RUN=play` (or `run`, `offline-pack`,
    /// `server-clients:N`, `replay:FILE`) starts Play (local) (or Run, Make
    /// available offline, Server & Clients with N players, a replay) at
    /// startup, a screenshot aid for the live checks, as
    /// `RBX_STUDIO_TERRAIN` is for the Terrain Editor. The beta has to be on.
    pub(super) fn apply_debug_local_run(&mut self, cx: &mut Context<Self>) {
        match std::env::var("RBX_STUDIO_LOCAL_RUN").as_deref() {
            Ok("play") => self.play_local(cx),
            Ok("run") => self.run_local(cx),
            Ok("offline-pack") => self.offline_pack(cx),
            Ok(other) => {
                if let Some(count) = other.strip_prefix("server-clients:") {
                    if let Ok(count) = count.parse::<usize>() {
                        self.play.live.local.players = count.clamp(1, MAX_PLAYERS);
                        self.server_and_clients(cx);
                    }
                }
                if let Some(file) = other.strip_prefix("replay:") {
                    if self.local_run_can_start() {
                        self.local_run_replay_file(std::path::Path::new(file), cx);
                    }
                }
            }
            _ => {}
        }
    }

    /// Play (local): works out the player off the UI thread, then starts.
    pub(crate) fn play_local(&mut self, cx: &mut Context<Self>) {
        self.play_local_in(None, cx);
    }

    /// Play (local), or with `windows` Server & Clients: that many test
    /// players, the server and each player in a window of their own.
    fn play_local_in(&mut self, windows: Option<usize>, cx: &mut Context<Self>) {
        if !self.local_run_can_start()
            || self.live_data_asks(live_prompt::Pending::Play(windows), cx)
        {
            return;
        }
        self.play.live.local.starting = true;
        push_row(
            &mut self.output,
            Feedback::Info("Play (local): getting your avatar\u{2026}".to_owned()),
        );
        cx.notify();
        let universe = crate::home::link_of(&self.path).map_or(0, |l| l.universe_id);
        cx.spawn(async move |this, cx| {
            let (player, badges) = cx
                .background_executor()
                .spawn(async move {
                    let player = avatars::resolve();
                    let badges = badges::resolve(universe, player.0.user_id);
                    (player, badges)
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                shell.play.live.local.starting = false;
                if shell.local_run_can_start() {
                    shell
                        .viewport
                        .update(cx, |viewport, _| viewport.core_gui.badges = badges);
                    shell.start_local(Some(player), windows, cx);
                }
            });
        })
        .detach();
    }

    fn start_local(
        &mut self,
        player: Option<(avatars::Player, avatars::StudioAvatars)>,
        windows: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let arriving = self.play.live.local.arriving.take();
        let place = arriving
            .as_ref()
            .and_then(|a| a.place.clone())
            .unwrap_or_else(|| self.dom.clone());
        let mut session = match rbx_sim::Session::new(place) {
            Ok(session) => session,
            Err(why) => {
                push_row(
                    &mut self.output,
                    Feedback::Error(format!("Local Run couldn\u{2019}t start: {why}")),
                );
                cx.notify();
                return;
            }
        };
        // Every run is recorded, for Replay and Save recording.
        session.record(replay::seed());
        session.set_network(local_network(&self.play.network.applied.conditions));
        // A frame scripts overrun stretches the next one's step, as on a
        // real server.
        session.set_wall_clock(true);
        session.set_fetcher(Arc::new(Fetcher(rbx_cloud::Client::new(
            rbx_cloud::ApiKey::from_env_or_config(),
        ))));
        rbx_lua::server::set_text_measure(Arc::new(rbx_viewer::measure_text));
        let mode = data::mode(&self.path);
        let mut services = data::services(&self.path, mode);
        let data_line = data::go_live(&self.path, mode, &mut services);
        services.web_session = self
            .roblox_session()
            .map(|s| rbx_lua::server::WebSession(s.expose_secret().to_owned()));
        push_row(&mut self.output, Feedback::Info(data_line));
        if let Some(device) = self.device_sim.device() {
            let touch = matches!(
                device.platform,
                crate::device_sim::Platform::Phone | crate::device_sim::Platform::Tablet
            );
            services.device = rbx_lua::server::Device {
                touch,
                keyboard: !touch,
                mouse: !touch,
                gamepad: device.platform == crate::device_sim::Platform::Console,
            };
        }
        let mut carried = Vec::new();
        if let Some(arriving) = arriving {
            services.arrivals = arriving.arrivals;
            for (user, gui) in arriving.guis {
                session.set_arriving_gui(user, gui);
            }
            carried = arriving.carried;
        }
        session.set_services(services);
        session.set_join_after(rbx_sim::JOIN_AFTER);
        session.start();
        let playing = player.is_some();
        let kept = player.as_ref().map(|(p, _)| p.clone());
        let mut note = None;
        if let Some((player, avatars)) = player {
            session.set_avatars(std::rc::Rc::new(avatars));
            match windows {
                // Studio's test players, every one wearing your avatar.
                Some(count) => {
                    for n in 1..=count {
                        session.join_later(
                            &format!("Player{n}"),
                            -(n as i64),
                            player.character.clone(),
                        );
                    }
                }
                None => queue_players(
                    &mut session,
                    &player.name,
                    player.user_id,
                    player.character,
                    self.play.live.local.players,
                ),
            }
            note = Some(match windows {
                Some(count) => test_players_note(count, &player.name, &player.note),
                None => player.note,
            });
        }
        let windowed = windows.is_some();
        // Play (local) shows the server under the loading screen until
        // its players joined; Server & Clients opens their windows then.
        let view = rbx_sim::Peer::Server;
        session.server.watch_view_changes();
        let output = session.take_output();
        // Server & Clients leaves this window on the edited place.
        let (dom, history, selection) = if windowed {
            (WeakDom::default(), History::new(DEFAULT_CAP), Vec::new())
        } else {
            let mut shown = session
                .sim(view)
                .map(|s| s.dom().clone())
                .unwrap_or_default();
            shown.take_changes();
            let history = std::mem::replace(&mut self.history, History::new(DEFAULT_CAP));
            let selection = self.selected_all().to_vec();
            self.deselect(cx);
            (stash_edited(&mut self.dom, shown), history, selection)
        };
        let task = cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            if this
                .update(cx, |shell, cx| shell.local_run_tick(cx))
                .is_err()
            {
                break;
            }
        });
        self.play.live.local.running = Some(Running {
            session,
            view,
            stash: Stash {
                dom,
                history,
                selection,
            },
            paused: false,
            step_once: false,
            clock: 0.0,
            last: Instant::now(),
            task: Some(task),
            playing,
            joining: playing,
            sound: sound::Mixer::new(self.viewport.read(cx).core_gui.volume),
            prompts: std::collections::VecDeque::new(),
            editables: rbx_dom::editable::generation(),
            player: kept,
            pad: None,
            replay: None,
            windowed,
            windows: Vec::new(),
            test_window: None,
            audible: None,
            remote: rbx_sim::remote::Host::default(),
            carried,
            remotes: Default::default(),
        });
        if windowed {
            push_row(
                &mut self.output,
                Feedback::Info("Server & Clients started".to_owned()),
            );
            if let Some(note) = note {
                push_row(&mut self.output, Feedback::Info(note));
            }
            self.local_run_open_windows(output, cx);
            cx.notify();
            return;
        }
        if playing {
            self.viewport
                .update(cx, |viewport, _| viewport.core_gui.joining = true);
        }
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
        let started = if playing {
            "Play (local) started"
        } else {
            "Local Run started"
        };
        push_row(&mut self.output, Feedback::Info(started.to_owned()));
        if let Some(note) = note {
            push_row(&mut self.output, Feedback::Info(note));
        }
        push_rows(&mut self.output, output, playing);
        cx.notify();
    }

    /// The view the run shows, while one runs.
    /// The Command Bar while a local run goes on: `source` runs in the
    /// DataModel this window shows, the server's or a player's client (a
    /// Server & Clients tab's own peer), as Studio's bar does during Play.
    /// Whether it went there; a replay and a collaborator's view keep the
    /// edit one.
    pub(super) fn local_run_command(&mut self, source: &str, cx: &mut Context<Self>) -> bool {
        let action = Action::Command(source.to_owned());
        if let Some(link) = &self.play.live.local.peer {
            if link.remote {
                return false;
            }
            let peer = link.view;
            return link
                .host
                .update(cx, |host, _| host.local_run_act(peer, action))
                .unwrap_or(false);
        }
        // Server & Clients leaves the edit place in this window, as Studio
        // does: its bar stays the editor's.
        let running = self.play.live.local.running.as_ref();
        let Some(view) = running.filter(|run| !run.windowed).map(|run| run.view) else {
            return false;
        };
        self.local_run_act(view, action)
    }

    fn local_run_act(&mut self, peer: rbx_sim::Peer, action: Action) -> bool {
        match self.play.live.local.running.as_mut() {
            Some(run) if run.replay.is_none() => {
                run.session.act(peer, action);
                true
            }
            _ => false,
        }
    }

    pub(crate) fn local_run_view(&self) -> Option<(rbx_sim::Peer, String)> {
        let run = self.play.live.local.running.as_ref()?;
        (!run.windowed && (run.playing || run.replay.is_some()))
            .then(|| (run.view, view_label(&run.session, run.view)))
    }

    /// Test › View during Play (local): the next of the server and each
    /// player's client. The shown tree becomes that peer's.
    pub(crate) fn local_run_next_view(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let count = run.session.clients.len();
        let view = match run.view {
            rbx_sim::Peer::Server if count > 0 => rbx_sim::Peer::Client(0),
            rbx_sim::Peer::Client(i) if i + 1 < count => rbx_sim::Peer::Client(i + 1),
            _ => rbx_sim::Peer::Server,
        };
        self.local_run_show(view, cx);
    }

    /// Play (local)'s players joined (on a frame `local_run_tick` ran):
    /// the run shows the first one's client, or stops when a join failed.
    fn local_run_joined(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        if let Some(why) = run.session.take_join_failures().into_iter().next() {
            push_row(
                &mut self.output,
                Feedback::Error(format!("Play (local) couldn\u{2019}t start: {why}")),
            );
            self.local_run_stop(cx);
            return;
        }
        for client in &mut run.session.clients {
            client.sim.watch_view_changes();
        }
        if run.windowed {
            self.local_run_open_player_windows(cx);
            return;
        }
        self.local_run_show(rbx_sim::Peer::Client(0), cx);
    }

    /// The shown tree becomes `view`'s.
    fn local_run_show(&mut self, view: rbx_sim::Peer, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        run.view = view;
        let client = matches!(run.view, rbx_sim::Peer::Client(_));
        let Some(sim) = run.session.sim_mut(run.view) else {
            return;
        };
        sim.take_view_changes();
        let shown = sim.dom().clone();
        if !client {
            self.play.live.local.shown_gui = None;
            self.viewport
                .update(cx, |viewport, _| viewport.set_player_gui(None));
        }
        self.deselect(cx);
        let changes = changes_between(&self.dom, &shown);
        self.dom = shown;
        self.dom.take_changes();
        let snapshots = self.dom.snapshot(&changes);
        self.viewport.update(cx, |viewport, _| {
            viewport.apply_changes(snapshots, changes);
            viewport.set_play_local(client);
        });
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
        cx.notify();
    }

    /// Draws `player_gui`'s screens (a `PlayerGui` in the shown tree)
    /// instead of `StarterGui`'s, or (`None`) the edit view's again; the
    /// viewport re-plans its GUI when that changes.
    fn show_player_gui(&mut self, player_gui: Option<rbx_dom::Ref>, cx: &mut Context<Self>) {
        if self.play.live.local.shown_gui == player_gui {
            return;
        }
        self.play.live.local.shown_gui = player_gui;
        self.viewport
            .update(cx, |viewport, _| viewport.set_player_gui(player_gui));
        // A reparenting nobody can place re-plans every screen.
        let Some(any) = player_gui.or_else(|| self.dom.root_refs().first().copied()) else {
            return;
        };
        let parent = self.dom.parent(any);
        let changes = vec![Change::Parent {
            referent: any,
            old: parent,
            new: parent,
        }];
        let snapshots = self.dom.snapshot(&changes);
        self.viewport
            .update(cx, |viewport, _| viewport.apply_changes(snapshots, changes));
    }

    /// Test › Players during Play (local): the last player leaves (their
    /// `PlayerRemoving` runs), as a player closing the game does.
    pub(crate) fn local_run_remove_player(&mut self, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let count = run.session.clients.len();
        if count < 2 {
            return;
        }
        let name = run.session.clients[count - 1].name.clone();
        if run.view == rbx_sim::Peer::Client(count - 1) {
            self.local_run_next_view(cx);
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        run.session.leave(count - 1);
        push_row(
            &mut self.output,
            Feedback::Info(format!("{name} left the game")),
        );
        cx.notify();
    }

    /// How many local players the running game has.
    pub(crate) fn local_run_player_count(&self) -> Option<usize> {
        let run = self.play.live.local.running.as_ref()?;
        (run.playing && !run.windowed).then_some(run.session.clients.len())
    }

    /// Test › Players: 1 to [`MAX_PLAYERS`], for the next Play (local).
    pub(crate) fn local_run_next_players(&mut self, cx: &mut Context<Self>) {
        let local = &mut self.play.live.local;
        local.players = local.players % MAX_PLAYERS + 1;
        cx.notify();
    }

    pub(crate) fn local_run_active(&self) -> bool {
        self.play.live.local.active()
    }

    pub(crate) fn local_run_paused(&self) -> bool {
        self.play.live.local.paused()
    }

    pub(crate) fn local_run_pause(&mut self, cx: &mut Context<Self>) {
        if self.local_run_forward(cx, |host, cx| host.local_run_pause(cx)) {
            return;
        }
        if let Some(run) = self.play.live.local.running.as_mut() {
            run.paused = !run.paused;
            cx.notify();
        }
    }

    /// Step while paused: one frame of the game, shown as a tick shows it.
    pub(crate) fn local_run_step_frame(&mut self, cx: &mut Context<Self>) {
        if self.local_run_forward(cx, |host, cx| host.local_run_step_frame(cx)) {
            return;
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        if !run.paused {
            return;
        }
        run.step_once = true;
        run.paused = false;
        self.local_run_tick(cx);
        if let Some(run) = self.play.live.local.running.as_mut() {
            run.paused = true;
            run.step_once = false;
        }
        cx.notify();
    }

    pub(crate) fn local_run_stop(&mut self, cx: &mut Context<Self>) {
        if self.local_run_forward(cx, |host, cx| host.local_run_stop(cx)) {
            return;
        }
        // Playing someone else's session: only they end it, this leaves.
        if self.remote_guest_leave(cx) {
            return;
        }
        let Some(run) = self.play.live.local.running.take() else {
            return;
        };
        let Running {
            mut session,
            stash,
            task,
            playing,
            replay,
            windowed,
            test_window,
            mut remote,
            ..
        } = run;
        drop(task);
        let bye = Down::Ended(format!("{} ended the session.", self.presence_name()));
        for to in remote.end() {
            self.play.live.local.remote.send_down(to, &bye);
        }
        if let Some(note) = self.play.live.local.remote.hosting_note() {
            push_row(&mut self.output, Feedback::Info(note));
        }
        if replay.is_none() {
            self.play.live.local.recording = session.recording();
        }
        let seconds = session.time();
        session.shutdown();
        let output = session.take_output();
        drop(session);
        if windowed {
            // End Session: the test window closes with it.
            if let Some(test_window) = test_window {
                test_window.close(cx);
            }
            push_row(
                &mut self.output,
                Feedback::Info(format!(
                    "Server & Clients ended after {seconds:.1} s of game time"
                )),
            );
            cx.notify();
            return;
        }
        self.viewport
            .update(cx, |viewport, _| viewport.set_play_local(false));
        self.show_player_gui(None, cx);
        self.deselect(cx);
        let back = changes_between(&self.dom, &stash.dom);
        restore_edited(&mut self.dom, stash.dom);
        let snapshots = self.dom.snapshot(&back);
        self.viewport
            .update(cx, |viewport, _| viewport.apply_changes(snapshots, back));
        self.history = stash.history;
        if crate::beta::enabled(crate::camera::ORTHOGRAPHIC_BETA_ID) {
            let span = crate::camera::current_ortho(&self.dom);
            self.viewport.read(cx).set_place_ortho(span);
        }
        self.properties = Properties::new(self.database.clone());
        self.rebuild_explorer(cx);
        self.reselect(stash.selection, cx);
        push_rows(&mut self.output, output, playing);
        push_row(
            &mut self.output,
            Feedback::Info(format!(
                "Local Run stopped after {seconds:.1} s of game time"
            )),
        );
        cx.notify();
    }

    /// A script teleported players into a place this run can reload into:
    /// the run stops and starts again there, each player bringing their
    /// TeleportData and teleport GUI.
    fn local_run_teleport(&mut self, teleport: rbx_sim::Teleport, cx: &mut Context<Self>) {
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let place = teleport.request.place;
        let target = match data::teleport_place(&self.path, place) {
            Some(file) => match rbx_sim::load::read_place(&file) {
                Ok(dom) => Some(dom),
                Err(why) => {
                    push_row(
                        &mut self.output,
                        Feedback::Error(format!("Teleport to {} failed: {why}", file.display())),
                    );
                    return;
                }
            },
            None => None,
        };
        let dom = run.session.server.dom();
        let mut arrivals = std::collections::HashMap::new();
        let mut guis = Vec::new();
        for player in &teleport.request.players {
            let Some(Variant::Int64(user)) = dom
                .get(*player)
                .and_then(|p| p.properties().get("UserId"))
                .cloned()
            else {
                continue;
            };
            arrivals.insert(
                user,
                rbx_lua::server::Arrival {
                    data: teleport.request.data.clone(),
                    source_place: 0,
                    gui: None,
                },
            );
            if let Some(gui) = &teleport.gui {
                guis.push((user, gui.clone()));
            }
        }
        drop(dom);
        let player = run.player.clone();
        // Collaborators come along as they are, on their own connections;
        // the test players are started again.
        let carried = run.remote.carry();
        let windows = run
            .windowed
            .then_some(run.session.clients.len().saturating_sub(carried.len()));
        push_row(
            &mut self.output,
            Feedback::Info(format!("Teleporting to place {place}\u{2026}")),
        );
        self.play.live.local.arriving = Some(Arriving {
            place: target,
            arrivals,
            guis,
            carried,
        });
        self.local_run_stop(cx);
        let player = player.map(|p| (p, avatars::lookups()));
        self.start_local(player, windows, cx);
    }

    /// One tick of the frame loop: steps the frames due on the wall clock.
    fn local_run_tick(&mut self, cx: &mut Context<Self>) {
        let showing = self.is_panel_showing(crate::shell::layout::Panel::RemoteInspector);
        self.leak_finder_keep_alive();
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        // Only while a Remote inspector shows does the session log remotes.
        let capture = showing || run.remotes.borrow().watched();
        run.session.capture_remotes(capture);
        let now = Instant::now();
        let elapsed = now.duration_since(run.last).as_secs_f64();
        run.last = now;
        let seeking = run.replay.as_ref().and_then(|r| r.seek);
        if run.paused && seeking.is_none() {
            run.sound.update(Vec::new(), Vec::new(), false);
            return;
        }
        let speed = run.replay.as_ref().map_or(1.0, |r| r.speed);
        run.clock += elapsed * speed;
        if run.step_once {
            run.clock = run.session.time() + 1.0 / FRAME_HZ;
        }
        if let Some(target) = seeking {
            if target < run.session.server.frame() {
                self.local_run_restart_replay(cx);
            }
            let Some(run) = self.play.live.local.running.as_mut() else {
                return;
            };
            run.clock = target as f64 / FRAME_HZ;
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let due =
            ((run.clock * FRAME_HZ).floor() as u64).saturating_sub(run.session.server.frame());
        if due == 0 {
            if let Some(replay) = run.replay.as_mut() {
                replay.seek = None;
            }
            return;
        }
        let most = match (&run.replay, seeking) {
            (Some(_), Some(_)) => replay::SEEK_FRAMES_PER_TICK,
            (Some(_), None) => MAX_FRAMES_PER_TICK * speed.ceil() as u64,
            _ => MAX_FRAMES_PER_TICK,
        };
        let steps = due.min(most);
        let viewing_client = matches!(run.view, rbx_sim::Peer::Client(_));
        let follows = run.playing || run.replay.is_some();
        if run.windowed && run.replay.is_none() {
            let pad = self.controller_pad.get();
            windows::take_inputs(run, pad, cx);
        } else if run.playing && viewing_client && run.replay.is_none() {
            let (inputs, size, safe) = self.viewport.update(cx, |viewport, _| {
                (
                    viewport.play.as_mut().map(|p| p.take()),
                    viewport.play_viewport_size(),
                    viewport.play_safe_insets(),
                )
            });
            if let rbx_sim::Peer::Client(i) = run.view {
                run.session.act(run.view, Action::Viewport(size));
                run.session.act(run.view, Action::SafeInsets(safe));
                for input in inputs.unwrap_or_default() {
                    run.session.input(i, input);
                }
                // The Controller Emulator is the client's gamepad while
                // its window is open.
                let pad = self.controller_pad.get();
                for input in pad_inputs(run.pad, pad) {
                    run.session.input(i, input);
                }
                run.pad = pad;
            }
        }
        let viewed_player = match run.view {
            rbx_sim::Peer::Client(i) => run.session.clients.get(i).map(|c| c.player),
            rbx_sim::Peer::Server => None,
        };
        let kicked_before = run.session.kicked.len();
        let mut changes = Vec::new();
        let mut by_peer = Vec::new();
        let mut diverged = None;
        for _ in 0..steps {
            match run.replay.as_mut() {
                Some(replay) => {
                    if run.session.server.frame() >= replay.playback.frame_count() {
                        run.paused = true;
                        replay.seek = None;
                        break;
                    }
                    // Pauses on the first divergence only; played on from
                    // there, the rest no longer compares.
                    if let Err(divergence) = replay.playback.step(&mut run.session) {
                        if replay.divergence.is_none() {
                            replay.seek = None;
                            run.paused = true;
                            diverged = Some(divergence);
                        }
                    }
                    for client in &mut run.session.clients {
                        client.sim.watch_view_changes();
                    }
                }
                None => run.session.step(),
            }
            for peer in std::iter::once(rbx_sim::Peer::Server)
                .chain((0..run.session.clients.len()).map(rbx_sim::Peer::Client))
            {
                let Some(sim) = run.session.sim_mut(peer) else {
                    continue;
                };
                let seen = sim.take_view_changes();
                if run.windowed {
                    windows::collect(&mut by_peer, peer, seen);
                } else if peer == run.view {
                    changes.extend(seen);
                }
            }
            if diverged.is_some() {
                break;
            }
        }
        if capture {
            let events = run.session.take_remote_events();
            run.remotes.borrow_mut().take(events, run.session.time());
        }
        if due > steps || run.paused {
            run.clock = run.session.time();
        }
        if let Some(divergence) = diverged {
            push_row(
                &mut self.output,
                Feedback::Error(format!("Replay {divergence}")),
            );
            if let Some(replay) = run.replay.as_mut() {
                replay.divergence = Some(divergence);
            }
        }
        if run.joining && !run.session.joining() {
            run.joining = false;
            let windowed = run.windowed;
            self.local_run_joined(cx);
            // The test windows still show this frame (the server's new
            // players are in it).
            if !windowed {
                return;
            }
        }
        if self
            .play
            .live
            .local
            .running
            .as_ref()
            .is_some_and(|run| !run.carried.is_empty() && !run.session.joining())
        {
            self.local_run_arrive_remote(cx);
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        // What showing the frame costs the editor, the profiler's render.
        let render_started = Instant::now();
        run.prompts.extend(run.session.take_prompts());
        let teleports = run.session.take_teleports();
        if let Some(teleport) = teleports.into_iter().next() {
            self.local_run_teleport(teleport, cx);
            return;
        }
        if run.windowed {
            let kicked = run.session.kicked[kicked_before..].to_vec();
            self.local_run_present_windows(by_peer, kicked, cx);
            return;
        }
        let notices: Vec<(String, String)> = run
            .session
            .take_notices()
            .into_iter()
            .filter(|n| n.player.is_none() || n.player == viewed_player)
            .map(|n| (n.title, n.text))
            .collect();
        let dialog = run
            .prompts
            .front()
            .map(|prompt| purchase_text(&run.session, prompt));
        // Someone kicked: the Output says so; the viewed player's own view
        // keeps their last frame under the message, the run goes on.
        let kicked: Vec<(String, String)> = run.session.kicked[kicked_before..].to_vec();
        for (name, message) in &kicked {
            push_row(
                &mut self.output,
                Feedback::Info(format!("{name} was kicked: {message}")),
            );
        }
        if let Some(player) = viewed_player {
            match run.session.clients.iter().position(|c| c.player == player) {
                Some(i) => run.view = rbx_sim::Peer::Client(i),
                None => {
                    let message = kicked.last().map(|(_, m)| m.clone()).unwrap_or_default();
                    run.view = rbx_sim::Peer::Server;
                    self.viewport.update(cx, |viewport, _| {
                        viewport.play = None;
                        viewport.core_gui.state = None;
                        viewport.core_gui.kicked = Some(message);
                    });
                    cx.notify();
                    return;
                }
            }
        }
        // Chat each client showed (only the viewed one's is drawn), and
        // what the player asked of the core GUI.
        let mut chat = Vec::new();
        for i in 0..run.session.clients.len() {
            if let Some(sim) = run.session.sim_mut(rbx_sim::Peer::Client(i)) {
                let lines = sim.take_chat();
                if run.view == rbx_sim::Peer::Client(i) {
                    chat = lines;
                }
            }
        }
        // `Emit`/`Clear` the viewed peer made, and the server's, which
        // every client sees.
        let mut bursts = Vec::new();
        for peer in std::iter::once(rbx_sim::Peer::Server)
            .chain((0..run.session.clients.len()).map(rbx_sim::Peer::Client))
        {
            let shown = peer == run.view || peer == rbx_sim::Peer::Server;
            if let Some(sim) = run.session.sim_mut(peer) {
                let taken = windows::bursts(sim);
                if shown {
                    bursts.extend(taken);
                }
            }
        }
        let (actions, menu_open) = self.viewport.update(cx, |viewport, _| {
            viewport.particle_bursts(bursts);
            viewport.core_gui.push_chat(chat);
            let actions = viewport.core_gui.take_actions();
            // The prompt just answered stays closed; the next shows next tick.
            if !actions.iter().any(|a| matches!(a, CoreAction::Purchase(_))) {
                viewport.core_gui.purchase = dialog;
            }
            (actions, viewport.core_gui.menu_open())
        });
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        // A replay's player acts only as recorded.
        let actions = if run.replay.is_some() {
            Vec::new()
        } else {
            run.session.act(run.view, Action::MenuOpen(menu_open));
            actions
        };
        for action in actions {
            match action {
                CoreAction::Say(text) => run.session.act(run.view, Action::Chat(text)),
                CoreAction::Reset => run.session.act(run.view, Action::Reset),
                CoreAction::Volume(volume) => run.sound.set_volume(volume),
                CoreAction::Purchase(bought) => {
                    if let Some(prompt) = run.prompts.pop_front() {
                        run.session.purchase_finished(&prompt, bought);
                    }
                }
                CoreAction::Leave => {
                    self.local_run_stop(cx);
                    return;
                }
            }
        }
        let Some(run) = self.play.live.local.running.as_mut() else {
            return;
        };
        let generation = rbx_dom::editable::generation();
        if generation != run.editables {
            run.editables = generation;
            changes.extend(editable_bindings(&self.dom));
        }
        let changes = dedupe(changes);
        let Some(viewed) = run.session.sim(run.view) else {
            return;
        };
        run.sound.update(
            viewed.voices(),
            viewed.take_audio(),
            follows && !run.paused && viewing_client,
        );
        mirror(&viewed.dom(), &mut self.dom, &changes);
        let output = run.session.take_output();
        self.dom.take_changes();
        push_rows(&mut self.output, output, run.playing);
        if follows && viewing_client {
            let Some(viewed) = run.session.sim(run.view) else {
                return;
            };
            let mouse = viewed.mouse_behavior();
            let camera = viewed.camera();
            let projection = play_projection(&viewed.dom());
            let player_gui = viewed.local_player().and_then(|(player, _)| {
                let dom = viewed.dom();
                dom.get(player)?
                    .children()
                    .iter()
                    .copied()
                    .find(|c| dom.get(*c).is_some_and(|i| i.class() == "PlayerGui"))
            });
            let core = run
                .session
                .sim_mut(run.view)
                .map(rbx_sim::Simulation::core_gui);
            self.show_player_gui(player_gui, cx);
            self.viewport.update(cx, |viewport, _| {
                if let Some(mut core) = core {
                    core.notifications.extend(notices);
                    viewport.core_gui.update(core);
                }
                if let Some(play) = viewport.play.as_mut() {
                    play.mouse = mouse;
                }
                if let Some((cframe, fov)) = camera {
                    let (eye, target) = eye_and_target(&cframe);
                    viewport.look_at_with(eye, target, fov);
                }
                viewport.set_play_projection(projection);
            });
        } else {
            self.viewport
                .update(cx, |viewport, _| viewport.end_play_projection());
        }
        if !changes.is_empty() {
            let snapshots = self.dom.snapshot(&changes);
            let sent = changes.clone();
            self.viewport
                .update(cx, |viewport, _| viewport.apply_changes(snapshots, sent));
            self.properties.dom_changed(&changes);
            if structural(&changes) {
                self.rebuild_explorer(cx);
            }
        }
        if let Some(run) = self.play.live.local.running.as_mut() {
            run.session.set_render_time(render_started.elapsed());
        }
        cx.notify();
    }

    /// The running Local Run session, for the MicroProfiler dock.
    pub(crate) fn local_run_session(&self) -> Option<&rbx_sim::Session> {
        self.play.live.local.running.as_ref().map(|r| &r.session)
    }

    /// The Remote inspector's captures: this window's run's, or in a
    /// Server & Clients tab, the run of the window that started it.
    pub(in crate::shell) fn remote_store(&self) -> Option<crate::shell::remote_inspector::Shared> {
        match (&self.play.live.local.running, &self.play.live.local.peer) {
            (Some(run), _) => Some(run.remotes.clone()),
            (None, Some(peer)) => Some(peer.remotes.clone()),
            (None, None) => None,
        }
    }

    /// The edited place's script a capture's caller names (a client's copy
    /// of a StarterPlayerScripts script maps back to it).
    pub(in crate::shell) fn remote_caller_script(&self, script: &str) -> Option<Ref> {
        let path: Vec<String> = script.split('.').map(str::to_owned).collect();
        let players: Vec<String> = self
            .local_run_session()?
            .clients
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let place = self.edit_dom();
        crate::shell::play::live::place_paths(&path, &players)
            .iter()
            .find_map(|candidate| crate::shell::play::live::find_path(place, candidate))
    }

    /// Edits made to the shown copy during the run: written into the
    /// simulation's DOM (logged, so it routes them) and shown right away.
    pub(super) fn local_run_reflect(&mut self, changes: &[Change], cx: &mut Context<Self>) {
        let sim_dom = match &self.play.live.local.peer {
            Some(peer) => peer.shown_dom(cx),
            None => self
                .play
                .live
                .local
                .running
                .as_ref()
                .and_then(|run| run.session.sim(run.view))
                .map(rbx_sim::Simulation::dom_rc),
        };
        if let Some(sim_dom) = sim_dom {
            mirror(&self.dom, &mut sim_dom.borrow_mut(), changes);
        }

        let snapshots = self.dom.snapshot(changes);
        let sent = changes.to_vec();
        self.viewport
            .update(cx, |viewport, _| viewport.apply_changes(snapshots, sent));
        self.properties.dom_changed(changes);
        cx.notify();
    }
}

/// The game camera's eye and a point straight ahead of it (its -Z).
/// What a player's view follows of their camera's projection: its
/// orthographic span with the Orthographic Camera beta on. Without it the
/// view stays perspective, as Studio's Play view does.
fn play_projection(dom: &rbx_dom::WeakDom) -> Option<(f32, rbx_viewer::OrthoAxis)> {
    crate::beta::enabled(crate::camera::ORTHOGRAPHIC_BETA_ID)
        .then(|| crate::camera::current_ortho(dom))
        .flatten()
}

fn eye_and_target(camera: &rbx_dom::CFrameData) -> ([f32; 3], [f32; 3]) {
    let p = camera.position;
    let r = camera.rotation;
    let eye = [p.x, p.y, p.z];
    (eye, [p.x - r[2], p.y - r[5], p.z - r[8]])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};

    // Not `super::*`: gpui_kit's glob brings its own `test` attribute.
    use super::{
        changes_between, dedupe, mirror, queue_players, restore_edited, stash_edited,
        test_players_note,
    };

    #[test]
    fn server_and_clients_names_the_test_players() {
        assert_eq!(
            test_players_note(2, "Cheeteau", "Playing as Cheeteau"),
            "Playing as Player1 to Player2, wearing Cheeteau\u{2019}s avatar"
        );
        assert_eq!(
            test_players_note(
                1,
                "Noob",
                "Offline with no cached avatar: playing as the noob"
            ),
            "Playing as Player1. Offline with no cached avatar: playing as the noob"
        );
    }

    /// Every item name in `menus`, submenus included.
    fn menu_names(items: &[gpui_kit::OwnedMenuItem], out: &mut Vec<String>) {
        for item in items {
            match item {
                gpui_kit::OwnedMenuItem::Action { name, .. } => out.push(name.clone()),
                gpui_kit::OwnedMenuItem::Submenu(menu) => menu_names(&menu.items, out),
                _ => {}
            }
        }
    }

    /// Tests never read `beta.json`, so every beta is off: Local Run's
    /// ribbon tiles, menu item, MicroProfiler, Remote Inspector, Leak Finder
    /// and Load test docks, the MicroProfiler's key, and the Collaborators
    /// dock's "Join as player", are gone, not greyed.
    #[test]
    fn with_the_beta_off_no_local_run_entry_is_present() {
        use crate::shell::ribbon::{test_tile_order, TestTile};
        assert!(!crate::beta::enabled(super::BETA_ID));

        let tiles = test_tile_order(false);
        assert!(tiles.iter().all(|tile| !tile.local_run()), "{tiles:?}");
        assert!(tiles.contains(&TestTile::Play) && tiles.contains(&TestTile::Stop));
        let on = test_tile_order(true);
        for tile in [
            TestTile::PlayLocal,
            TestTile::ServerClients,
            TestTile::Replay,
            TestTile::Players,
            TestTile::Data,
            TestTile::Offline,
        ] {
            assert!(on.contains(&tile) && !tiles.contains(&tile), "{tile:?}");
        }

        let mut names = Vec::new();
        for menu in crate::menu_bar::menus(&[]) {
            menu_names(&menu.items, &mut names);
        }
        assert!(!names.iter().any(|n| n.contains("(local)")), "{names:?}");
        // M19b's "Join as player" needs this beta and co-edit's.
        assert!(!super::remote::betas_on());

        let hidden = crate::shell::ui_editor::beta_hidden_panels();
        assert!(hidden.contains(&crate::shell::layout::Panel::MicroProfiler));
        assert!(hidden.contains(&crate::shell::layout::Panel::RemoteInspector));
        assert!(hidden.contains(&crate::shell::layout::Panel::LeakFinder));
        assert!(hidden.contains(&crate::shell::layout::Panel::LoadTest));
        assert!(!crate::shortcuts::Command::MicroProfiler.available());

        // The Tests dock lists no integration spec; the plain ones stay.
        let mut dom = rbx_sim::conformance::bare_place();
        let storage = *dom
            .root_refs()
            .iter()
            .find(|r| dom.get(**r).is_some_and(|i| i.class() == "ServerStorage"))
            .expect("bare_place has ServerStorage");
        for name in ["Unit.spec", "Game.integration.spec"] {
            dom.new_instance("ModuleScript", name, Some(storage));
        }
        let listed = |local_run| {
            crate::shell::tests_dock::discover_specs(&dom, local_run)
                .expect("discovers")
                .into_iter()
                .map(|f| f.name)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            listed(crate::beta::enabled(super::BETA_ID)),
            ["ServerStorage.Unit.spec"]
        );
        assert_eq!(
            listed(true),
            [
                "ServerStorage.Unit.spec",
                "ServerStorage.Game.integration.spec"
            ]
        );
    }

    #[test]
    fn play_local_starts_without_waiting_for_its_players() {
        let mut session =
            rbx_sim::Session::new(rbx_sim::conformance::bare_place()).expect("the place runs");
        session.set_join_after(rbx_sim::JOIN_AFTER);
        session.start();
        queue_players(&mut session, "Player1", 1, None, 2);
        // Not one server frame ran: the editor shows the run at once.
        assert_eq!(session.server.frame(), 0);
        assert!(session.clients.is_empty() && session.joining());
        // The frame loop's ticks join them once the server ran JOIN_AFTER.
        while session.joining() {
            session.step();
        }
        assert!(session.server.time() >= rbx_sim::JOIN_AFTER);
        let names: Vec<_> = session.clients.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Player1", "Player2"]);
    }

    fn cframe(y: f32) -> Variant {
        Variant::CFrame(CFrameData {
            position: Vector3Data { x: 0.0, y, z: 0.0 },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        })
    }

    fn place_with_part() -> (WeakDom, Ref, Ref) {
        let mut dom = WeakDom::new();
        let workspace = dom.new_instance("Workspace", "Workspace", None);
        let part = dom.new_instance("Part", "Part", Some(workspace));
        let _ = dom.set_property(part, "CFrame", cframe(50.0));
        let _ = dom.set_property(
            part,
            "Size",
            Variant::Vector3(Vector3Data {
                x: 2.0,
                y: 2.0,
                z: 2.0,
            }),
        );
        let _ = dom.set_property(part, "Anchored", Variant::Bool(false));
        dom.take_changes();
        (dom, workspace, part)
    }

    /// The tree as text, in tree order with sorted properties.
    fn fingerprint(dom: &WeakDom) -> String {
        let mut out = String::new();
        let mut stack: Vec<Ref> = dom.root_refs().iter().rev().copied().collect();
        while let Some(at) = stack.pop() {
            let Some(instance) = dom.get(at) else {
                continue;
            };
            stack.extend(instance.children().iter().rev().copied());
            let properties: BTreeMap<String, String> = instance
                .properties()
                .iter()
                .map(|(key, value)| (key.to_string(), format!("{value:?}")))
                .collect();
            out.push_str(&format!(
                "{} {} {} {:?}\n",
                at.value(),
                instance.class(),
                instance.name(),
                properties
            ));
        }
        out
    }

    #[test]
    fn local_run_mirrors_a_property_change() {
        let (mut sim, _, part) = place_with_part();
        let mut shown = sim.clone();
        let _ = sim.set_property(part, "CFrame", cframe(7.0));
        let changes = sim.take_changes();
        mirror(&sim, &mut shown, &changes);
        assert_eq!(
            shown.get(part).and_then(|i| i.properties().get("CFrame")),
            Some(&cframe(7.0))
        );
    }

    #[test]
    fn local_run_mirrors_an_added_part() {
        let (mut sim, workspace, _) = place_with_part();
        let mut shown = sim.clone();
        let added = sim.new_instance("Part", "Spawned", Some(workspace));
        let _ = sim.set_property(added, "CFrame", cframe(3.0));
        let changes = sim.take_changes();
        mirror(&sim, &mut shown, &changes);
        let copy = shown.get(added).expect("the part is shown");
        assert_eq!(copy.name(), "Spawned");
        assert_eq!(shown.parent(added), Some(workspace));
        assert_eq!(copy.properties().get("CFrame"), Some(&cframe(3.0)));
    }

    #[test]
    fn local_run_mirrors_a_removed_part() {
        let (mut sim, workspace, part) = place_with_part();
        let mut shown = sim.clone();
        sim.remove(part);
        let changes = sim.take_changes();
        mirror(&sim, &mut shown, &changes);
        assert!(shown.get(part).is_none());
        assert!(shown
            .get(workspace)
            .is_some_and(|w| !w.children().contains(&part)));
    }

    #[test]
    fn local_run_restores_the_place_exactly() {
        let (place, _, part) = place_with_part();
        let before = fingerprint(&place);
        let mut dom = place;
        let mut sim = rbx_sim::Simulation::new(dom.clone()).expect("the simulation starts");
        sim.watch_view_changes();
        sim.start();
        let mut shown = sim.dom().clone();
        shown.take_changes();
        let edited = stash_edited(&mut dom, shown);
        for _ in 0..10 {
            sim.step();
            let changes = dedupe(sim.take_view_changes());
            mirror(&sim.dom(), &mut dom, &changes);
        }
        assert_ne!(
            dom.get(part).and_then(|i| i.properties().get("CFrame")),
            Some(&cframe(50.0)),
            "the part fell while running"
        );
        // The changes back, mirrored into the run's copy, give the place.
        let mut shown = dom.clone();
        let back = changes_between(&shown, &edited);
        assert!(!back.is_empty());
        mirror(&edited, &mut shown, &back);
        assert_eq!(fingerprint(&shown), before);
        restore_edited(&mut dom, edited);
        assert_eq!(fingerprint(&dom), before);
        assert!(dom.changes().is_empty());
    }
}
