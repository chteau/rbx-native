//! **Row C**: the ribbon: grouped commands for whichever category tab Row
//! B has selected (Row B itself lives in `shell::chrome`, since it is a tab
//! strip like Row A's, not ribbon content).
//!
//! Two button shapes, both the frame's:
//!
//! - a **tile**, 42px wide, icon over label, filling the ribbon's height,
//!   for the commands worth aiming at;
//! - a **stack**, 78px wide, three rows of icon-beside-label, for the
//!   ones that belong to the tile beside them (Copy's paste/cut/duplicate)
//!   or that are really a readout (the snap increments).
//!
//! Which tiles each tab shows, and in what order, is `layout`'s: Roblox
//! Studio 0.748's ribbon, with this editor's own commands beside their
//! nearest Studio neighbour. A Studio command this editor doesn't have
//! isn't drawn at all; a tile that is drawn but has nothing to act on yet
//! (Copy with nothing selected, Play mid-test) greys with the reason on
//! hover, the same rule `menu_bar`'s greyed items follow.

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex, Icon};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::roving::Roving;

use crate::script_templates::ScriptTemplates;
use crate::tokens;
use crate::transform::Tool;

use super::menu::{self, MenuId};
use super::play::{session::StopReason, Phase};
use super::Shell;

mod commands;
mod items;
pub(super) mod layout;
mod model_tiles;
mod overflow;
mod toggles;

#[cfg(test)]
pub(super) use commands::PART_INSERTS;
pub(super) use commands::{palette_entries, RibbonCommand, GUI_INSERTS, SUN_BODIES, SUN_MODES};
use commands::{Insert, SCRIPT_INSERTS};
pub(super) use model_tiles::is_locked;
pub(super) use overflow::RibbonScroll;

/// The ribbon's category tabs: Studio's, in Studio's order, plus Test,
/// which holds what Studio 0.748 keeps in its quick-access row and Test
/// menu (Play, Stop, the device and controller emulators).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Tab {
    #[default]
    Home,
    Avatar,
    Ui,
    Script,
    Model,
    Test,
    Plugins,
}

impl Tab {
    pub(super) const ALL: [Tab; 7] = [
        Tab::Home,
        Tab::Avatar,
        Tab::Ui,
        Tab::Script,
        Tab::Model,
        Tab::Test,
        Tab::Plugins,
    ];

    pub(super) fn label(self) -> &'static str {
        match self {
            Tab::Home => "Home",
            Tab::Avatar => "Avatar",
            Tab::Ui => "UI",
            Tab::Script => "Script",
            Tab::Model => "Model",
            Tab::Test => "Test",
            Tab::Plugins => "Plugins",
        }
    }
}

impl Shell {
    pub(super) fn ribbon(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        // The ribbon is one Tab stop whose control count depends on the
        // open page, so the group is opened here and closed once the page
        // has actually been built (see `shell::roving`).
        self.ribbon_nav.begin(&self.tab_order, None, cx);
        let mut groups: Vec<Vec<AnyElement>> = Vec::new();
        for group in layout::groups(self.ribbon_tab, layout::Gates::current()) {
            if group == [layout::Item::PluginToolbars] {
                groups.extend(self.plugin_toolbar_groups(cx));
                continue;
            }
            groups.push(
                group
                    .into_iter()
                    .flat_map(|item| self.ribbon_item(item, cx))
                    .collect(),
            );
        }

        // One separator between groups, none at either end, the frame's own
        // rhythm, and the reason the groups are built as a list of lists
        // rather than one flat row.
        let mut children: Vec<AnyElement> = Vec::new();
        for group in groups.into_iter().filter(|group| !group.is_empty()) {
            if !children.is_empty() {
                children.push(separator());
            }
            children.push(
                h_flex()
                    .flex_none()
                    .h_full()
                    .items_stretch()
                    .gap(px(5.))
                    .children(group)
                    .into_any_element(),
            );
        }

        self.ribbon_nav.finish();

        let strip = h_flex()
            .id("ribbon")
            .w_full()
            .h(tokens::ribbon_height())
            .flex_none()
            .items_stretch()
            // Scrolls rather than clips, with a chevron at each end that has
            // more (see `overflow`). A narrow window or a 2x UI scale makes a
            // page wider than the window, and a ribbon that simply loses its
            // last buttons is WCAG 1.4.4's "loss of content".
            .overflow_x_scroll()
            .pt(px(9.))
            .px(px(16.))
            .pb(px(11.))
            .gap(px(14.))
            .bg(tokens::chrome())
            .border_b(px(1.))
            .border_color(tokens::border())
            // Arrow keys move between the ribbon's buttons rather than
            // leaving it; Enter and Space are left alone, because GPUI
            // already turns those into a click on whichever button holds
            // focus.
            .on_key_down(cx.listener(|shell, event: &KeyDownEvent, window, cx| {
                if shell.ribbon_nav.key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                    cx.notify();
                }
            }))
            .children(children);
        self.ribbon_with_overflow(strip)
    }

    /// Studio's first Avatar button, in a group of its own: the orange
    /// figure with a gear that opens the Avatar Settings dock.
    fn avatar_settings_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        let size = px(20.);
        let badge = div()
            .absolute()
            .right(px(-2.))
            .bottom(px(-1.))
            .size(px(10.))
            .rounded_full()
            .bg(tokens::tile())
            .flex()
            .items_center()
            .justify_center()
            .child(
                Icon::new(IconName::Cog)
                    .size(px(9.))
                    .text_color(tokens::text_full()),
            );
        let icon = div()
            .relative()
            .flex_none()
            .size(size)
            .child(
                Icon::new(IconName::PersonStanding)
                    .size_full()
                    .text_color(tokens::warning()),
            )
            .child(badge);
        let tile = tile_frame("ribbon-avatar-settings", icon, "Settings", true);
        self.ribbon_nav
            .claim(tile, cx)
            .tooltip(|window, cx| {
                super::tooltip::text(
                    "Avatar Settings \u{2014} the place's avatar rules",
                    window,
                    cx,
                )
            })
            .on_click(cx.listener(|shell, _, _, cx| shell.toggle_avatar_settings(cx)))
            .into_any_element()
    }

    /// Avatar's Setup tile: Studio's tool turns a character mesh into a
    /// rig, which is what the auto-rigger does.
    fn avatar_setup_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        tile(
            &self.ribbon_nav,
            "ribbon-avatar-setup",
            IconName::Settings,
            "Setup",
            cx,
        )
        .tooltip(|window, cx| {
            super::tooltip::text(
                "Setup \u{2014} auto-rig a character mesh into an R15 rig",
                window,
                cx,
            )
        })
        .on_click(cx.listener(|shell, _, _, cx| shell.choose_autorig(cx)))
        .into_any_element()
    }

    /// The UI tab's Style Editor, as in Studio: the UI Editor on its
    /// Stylesheet sub-tab, lit while that is up.
    fn style_editor_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        let shown = self.stylesheet_shown();
        tile(
            &self.ribbon_nav,
            "ribbon-style-editor",
            IconName::Palette,
            "Style Editor",
            cx,
        )
        .when(shown, |this| {
            this.bg(tokens::accent_soft())
                .text_color(tokens::check_on())
        })
        .tooltip(|window, cx| {
            super::tooltip::text(
                "Style Editor \u{2014} the place's style sheets and rules",
                window,
                cx,
            )
        })
        .on_click(cx.listener(|shell, _, _, cx| shell.reveal_style_editor(cx)))
        .into_any_element()
    }

    fn insert_gui(&self, cx: &mut Context<Self>) -> impl IntoElement + 'static {
        menu::dropdown_in_group(
            self,
            MenuId::InsertGui,
            super::chrome::Trigger::new(tile(
                &self.ribbon_nav,
                "ribbon-insert-gui",
                IconName::AppWindow,
                "GUI",
                cx,
            )),
            vec![
                insert_item(&GUI_INSERTS[0]),
                insert_item(&GUI_INSERTS[1]),
                insert_item(&GUI_INSERTS[2]),
                insert_item(&GUI_INSERTS[3]),
            ],
            cx,
        )
    }
}

/// A primary or destructive tile's 44x44 floor (WCAG 2.5.5), whatever the
/// UI scale or Large Click Targets say.
pub(super) fn primary(id: &'static str, tile: Stateful<Div>) -> Stateful<Div> {
    tile.debug_selector(|| id.into())
        .min_w(tokens::primary_target())
        .min_h(tokens::primary_target())
}

/// `ROADMAP.md`'s Play/Test section still lists the sandbox-place design
/// all of these would need as open.
impl Shell {
    /// Studio's "Current: Client / Server" switch: which game tree the
    /// Explorer and Properties show while a test runs.
    fn view_tile(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        use super::local_run::windows::Role;
        use super::play::live::View;
        // Server & Clients: the Server window can show the first player's
        // client; a player window shows its own.
        match self.local_run_role() {
            Some(Role::Server { client_view }) => {
                let (icon, label) = if client_view {
                    (IconName::Globe, "Current: Client")
                } else {
                    (IconName::Server, "Current: Server")
                };
                return tile(&self.ribbon_nav, "ribbon-view", icon, label, cx)
                    .tooltip(|window, cx| {
                        super::tooltip::text(
                            "Switch this window between the server and Player1's client.",
                            window,
                            cx,
                        )
                    })
                    .on_click(
                        cx.listener(|shell, _, _, cx| shell.local_run_toggle_server_view(cx)),
                    );
            }
            Some(Role::Player) => {
                return unavailable_tile(
                    "ribbon-view",
                    IconName::Globe,
                    "Client",
                    "a player's window shows their client; the Server window shows the server",
                );
            }
            _ => {}
        }
        // Play (local): the server, then each local player's client.
        if let Some((peer, label)) = self.local_run_view() {
            let icon = match peer {
                rbx_sim::Peer::Server => IconName::Server,
                rbx_sim::Peer::Client(_) => IconName::Globe,
            };
            let tile = base_tile("ribbon-view", icon, SharedString::from(label.clone()), true);
            return self
                .ribbon_nav
                .claim(tile, cx)
                .tooltip(move |window, cx| {
                    super::tooltip::text(
                        format!(
                            "Showing {label}. Click for the next of the server and each player."
                        ),
                        window,
                        cx,
                    )
                })
                .on_click(cx.listener(|shell, _, _, cx| shell.local_run_next_view(cx)));
        }
        let view = self.play.live.view;
        let (icon, label) = match view {
            View::Client => (IconName::Globe, "Client"),
            View::Server => (IconName::Server, "Server"),
        };
        if self.play.phase == Phase::Running {
            let next = match view {
                View::Client => View::Server,
                View::Server => View::Client,
            };
            tile(&self.ribbon_nav, "ribbon-view", icon, label, cx)
                .on_click(cx.listener(move |shell, _, _, cx| shell.live_set_view(next, cx)))
        } else {
            unavailable_tile("ribbon-view", icon, label, "no test is running")
        }
    }

    /// The running game's numbers beside the test tiles: one row per side
    /// (`play::session::stats_line`), every number in the row's tooltip.
    /// Nothing while no test runs.
    fn stats_readout(&self) -> Option<Div> {
        use super::play::session::{stats_details, stats_line};
        use rbx_play::protocol::Side;
        if self.play.phase != Phase::Running {
            return None;
        }
        let row = |id: &'static str, icon: IconName, side: Side, metrics| {
            let (line, details) = match metrics {
                Some(metrics) => (stats_line(side, metrics), stats_details(metrics)),
                None => (
                    "\u{2014}".to_owned(),
                    "Waiting for the first report.".to_owned(),
                ),
            };
            h_flex()
                .id(id)
                .w_full()
                .flex_1()
                .items_center()
                .gap(px(6.))
                .px(px(7.))
                .rounded(tokens::radius())
                .when_some(
                    tokens::on_surface(tokens::tile(), tokens::chrome()),
                    |this, bg| this.bg(bg),
                )
                .text_size(tokens::text_xs())
                .line_height(tokens::line_xs())
                .text_color(tokens::text_label())
                .child(Icon::new(icon).size(tokens::text_xs()))
                .child(div().flex_1().truncate().child(line))
                .tooltip(move |window, cx| super::tooltip::text(details.clone(), window, cx))
        };
        Some(
            v_flex()
                .flex_none()
                .w(px(190.))
                .h_full()
                .gap(px(4.))
                .child(row(
                    "ribbon-stats-client",
                    IconName::Globe,
                    Side::Client,
                    self.play.stats.client.as_ref(),
                ))
                .child(row(
                    "ribbon-stats-server",
                    IconName::Server,
                    Side::Server,
                    self.play.stats.server.as_ref(),
                )),
        )
    }

    fn test_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let nav = &self.ribbon_nav;
        let idle = self.play.phase == Phase::Idle;
        let local_beta = crate::beta::enabled(super::local_run::BETA_ID);
        let local_active = self.local_run_active();
        let play = if local_active {
            unavailable_tile(
                "ribbon-play",
                IconName::Play,
                "Play",
                "Local Run is running",
            )
        } else if idle && rbx_net::is_offline() {
            let why = if local_beta {
                "You\u{2019}re offline, and Play needs Roblox\u{2019}s servers. Play (local) works offline."
            } else {
                "You\u{2019}re offline, and Play needs Roblox\u{2019}s servers."
            };
            base_tile("ribbon-play", IconName::Play, "Play", false)
                .tooltip(move |window, cx| super::tooltip::text(why, window, cx))
        } else if idle {
            tile(nav, "ribbon-play", IconName::Play, "Play", cx)
                .on_click(cx.listener(|shell, _, window, cx| shell.play_start(window, cx)))
        } else {
            unavailable_tile(
                "ribbon-play",
                IconName::Play,
                "Play",
                "a test is already running",
            )
        };
        // Playing someone else's session: only its host ends it.
        let stop = if self.local_run_remote_viewer() {
            tile(nav, "ribbon-stop", IconName::LogOut, "Leave", cx)
                .tooltip(|window, cx| {
                    super::tooltip::text(
                        "Leave this session; only its host can end it.",
                        window,
                        cx,
                    )
                })
                .on_click(cx.listener(|shell, _, _, cx| shell.local_run_stop(cx)))
        } else if self.local_run_role().is_some() {
            tile(nav, "ribbon-stop", IconName::Square, "End Session", cx)
                .tooltip(|window, cx| {
                    super::tooltip::text("Close the server and all client windows.", window, cx)
                })
                .on_click(cx.listener(|shell, _, _, cx| shell.local_run_stop(cx)))
        } else if local_active {
            tile(nav, "ribbon-stop", IconName::Square, "Stop", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.local_run_stop(cx)))
        } else if idle {
            unavailable_tile(
                "ribbon-stop",
                IconName::Square,
                "Stop",
                "nothing is running",
            )
        } else {
            tile(nav, "ribbon-stop", IconName::Square, "Stop", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.play_stop(StopReason::Stopped, cx)))
        };
        let invite = if self.play.job_id.is_some() {
            tile(nav, "ribbon-invite", IconName::Users, "Invite", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.play_copy_join_link(cx)))
        } else {
            unavailable_tile(
                "ribbon-invite",
                IconName::Users,
                "Invite",
                "no test server yet",
            )
        };
        let (mut play, mut stop, mut invite) = (Some(play), Some(stop), Some(invite));
        let mut stacks = local_beta.then(|| self.local_run_stacks(cx).map(Some));
        let mut tiles = Vec::new();
        for slot in test_tile_order(local_beta) {
            let mut stack_at = |i: usize| stacks.as_mut().and_then(|s| s[i].take());
            let element = match slot {
                TestTile::Play => play
                    .take()
                    .map(|t| primary("ribbon-play", t).into_any_element()),
                TestTile::PlayLocal => stack_at(0),
                TestTile::ServerClients => stack_at(1),
                TestTile::Replay => stack_at(2),
                // Stop is a tile of its own rather than a third stack row:
                // three rows in an 80px ribbon are ~25px each, and Stop is
                // the control a person reaches for in a hurry.
                TestTile::Stop => stop
                    .take()
                    .map(|t| primary("ribbon-stop", t).into_any_element()),
                TestTile::Invite => invite.take().map(IntoElement::into_any_element),
                TestTile::View => Some(self.view_tile(cx).into_any_element()),
                TestTile::Players => Some(self.players_tile(cx)),
                TestTile::Data => Some(self.data_tile(cx)),
                TestTile::Offline => Some(self.offline_tile(cx)),
                TestTile::Network => Some(self.network_tile(cx)),
                TestTile::Device => Some(self.device_tile(cx)),
                TestTile::Controller => Some(self.controller_tile(cx).into_any_element()),
            };
            tiles.extend(element);
        }
        tiles.extend(self.stats_readout().map(IntoElement::into_any_element));
        tiles
    }

    /// Local Run's three stacks, only built with its beta on: Play (local)
    /// over Run, Server & Clients over Pause, Replay over Save run.
    fn local_run_stacks(&self, cx: &mut Context<Self>) -> [AnyElement; 3] {
        let nav = &self.ribbon_nav;
        let idle = self.play.phase == Phase::Idle;
        let local_active = self.local_run_active();
        // Paused, the Run row steps the game one frame at a time.
        let run = if local_active && self.local_run_paused() {
            live_stack_row(nav, "ribbon-run", IconName::SkipForward, "Step", cx)
                .tooltip(|window, cx| {
                    super::tooltip::text("Run one frame (1/60 s) of the paused game.", window, cx)
                })
                .on_click(cx.listener(|shell, _, _, cx| shell.local_run_step_frame(cx)))
        } else if local_active {
            unavailable_row(
                "ribbon-run",
                IconName::SquarePlay,
                "Run",
                "Local Run is already running",
            )
        } else if idle {
            live_stack_row(nav, "ribbon-run", IconName::SquarePlay, "Run", cx)
                .on_click(cx.listener(|shell, _, _, cx| shell.run_local(cx)))
        } else {
            unavailable_row(
                "ribbon-run",
                IconName::SquarePlay,
                "Run",
                "a test is already running",
            )
        };
        let play_local = if local_active || self.play.live.local.starting {
            unavailable_row(
                "ribbon-play-local",
                IconName::PersonStanding,
                "Play (local)",
                "Local Run is already running",
            )
        } else if idle {
            live_stack_row(
                nav,
                "ribbon-play-local",
                IconName::PersonStanding,
                "Play (local)",
                cx,
            )
            .on_click(cx.listener(|shell, _, _, cx| shell.play_local(cx)))
        } else {
            unavailable_row(
                "ribbon-play-local",
                IconName::PersonStanding,
                "Play (local)",
                "a test is already running",
            )
        };
        let server_clients = if idle && !local_active {
            live_stack_row(
                nav,
                "ribbon-server-clients",
                IconName::Users,
                "Server & Clients",
                cx,
            )
            .tooltip(|window, cx| {
                super::tooltip::text(
                    "Start a local server and Test \u{203a} Players test players, each in a window of their own, as Studio's Server & Clients does.",
                    window,
                    cx,
                )
            })
            .on_click(cx.listener(|shell, _, _, cx| shell.server_and_clients(cx)))
        } else {
            unavailable_row(
                "ribbon-server-clients",
                IconName::Users,
                "Server & Clients",
                "a test is already running",
            )
        };
        let pause = if local_active {
            let paused = self.local_run_paused();
            live_stack_row(nav, "ribbon-pause", IconName::Pause, "Pause", cx)
                .when(paused, |this| {
                    this.bg(tokens::accent_soft())
                        .text_color(tokens::check_on())
                })
                .on_click(cx.listener(|shell, _, _, cx| shell.local_run_pause(cx)))
        } else {
            unavailable_row(
                "ribbon-pause",
                IconName::Pause,
                "Pause",
                "there is no local run to pause",
            )
        };
        // Every Local Run is recorded: Replay plays it back frame for
        // frame, Save run keeps its recording.
        let recorded = self.local_run_has_recording();
        let replay = if recorded && !self.local_run_replaying() {
            live_stack_row(nav, "ribbon-replay", IconName::Film, "Replay", cx)
                .tooltip(|window, cx| {
                    super::tooltip::text(
                        "Play the last Local Run back exactly, frame for frame, and see where it first differs.",
                        window,
                        cx,
                    )
                })
                .on_click(cx.listener(|shell, _, _, cx| shell.local_run_replay(cx)))
        } else {
            unavailable_row(
                "ribbon-replay",
                IconName::Film,
                "Replay",
                if recorded {
                    "a replay is running"
                } else {
                    "run the place locally first"
                },
            )
        };
        let save_recording = if recorded {
            live_stack_row(nav, "ribbon-save-recording", IconName::Save, "Save run", cx)
                .tooltip(|window, cx| {
                    super::tooltip::text(
                        "Save the recording of the last Local Run, to replay later or in CI.",
                        window,
                        cx,
                    )
                })
                .on_click(cx.listener(|shell, _, _, cx| shell.local_run_save_recording(cx)))
        } else {
            unavailable_row(
                "ribbon-save-recording",
                IconName::Save,
                "Save run",
                "run the place locally first",
            )
        };
        [
            stack(vec![play_local, run]).into_any_element(),
            stack(vec![server_clients, pause]).into_any_element(),
            stack(vec![replay, save_recording]).into_any_element(),
        ]
    }
}

impl Shell {
    /// Test › Players: how many local players the next Play (local)
    /// starts, picked from a menu as in Studio.
    fn players_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        use super::local_run::windows::Role;
        const LABELS: [&str; super::local_run::MAX_PLAYERS] = [
            "1 Player",
            "2 Players",
            "3 Players",
            "4 Players",
            "5 Players",
            "6 Players",
            "7 Players",
            "8 Players",
        ];
        let count = self.play.live.local.players;
        let label = LABELS[count.clamp(1, LABELS.len()) - 1];
        // Server & Clients: a player window's Exit, Add Clients elsewhere.
        match self.local_run_role() {
            Some(Role::Player) => {
                return tile(
                    &self.ribbon_nav,
                    "ribbon-players",
                    IconName::LogOut,
                    "Exit",
                    cx,
                )
                .tooltip(|window, cx| super::tooltip::text("Exit client window.", window, cx))
                .on_click(cx.listener(|shell, _, _, cx| shell.local_run_exit_client(cx)))
                .into_any_element();
            }
            Some(_) => {
                return tile(
                    &self.ribbon_nav,
                    "ribbon-players",
                    IconName::UserPlus,
                    "Add Client",
                    cx,
                )
                .tooltip(|window, cx| {
                    super::tooltip::text("Add a client to the current session.", window, cx)
                })
                .on_click(cx.listener(|shell, _, _, cx| shell.local_run_add_client(cx)))
                .into_any_element();
            }
            None => {}
        }
        if let Some(count) = self.local_run_player_count().filter(|n| *n > 1) {
            const RUNNING: [&str; super::local_run::MAX_PLAYERS - 1] = [
                "Players: 2",
                "Players: 3",
                "Players: 4",
                "Players: 5",
                "Players: 6",
                "Players: 7",
                "Players: 8",
            ];
            let label = RUNNING[count.clamp(2, RUNNING.len() + 1) - 2];
            return tile(
                &self.ribbon_nav,
                "ribbon-players",
                IconName::Users,
                label,
                cx,
            )
            .tooltip(|window, cx| {
                super::tooltip::text("Click to have the last player leave the game.", window, cx)
            })
            .on_click(cx.listener(|shell, _, _, cx| shell.local_run_remove_player(cx)))
            .into_any_element();
        }
        if self.local_run_active() {
            return unavailable_tile(
                "ribbon-players",
                IconName::Users,
                label,
                "takes effect on the next Play (local)",
            )
            .into_any_element();
        }
        let trigger = tile(
            &self.ribbon_nav,
            "ribbon-players",
            IconName::Users,
            label,
            cx,
        );
        let items = (1..=LABELS.len())
            .map(|players| {
                menu::item(LABELS[players - 1])
                    .checked(players == count)
                    .on_click(move |shell, cx| shell.set_local_run_players(players, cx))
            })
            .collect();
        menu::dropdown_in_group(
            self,
            MenuId::TestPlayers,
            super::chrome::Trigger::new(trigger)
                .tooltip("Local players for Play (local) and Server & Clients"),
            items,
            cx,
        )
        .into_any_element()
    }

    /// Test › Data: whether Local Run keeps this place's DataStores between
    /// runs, starts them empty, uses the experience's real ones, or has API
    /// access off, picked from a menu as in Studio, so Live is never one
    /// stray click away.
    fn data_tile(&self, cx: &mut Context<Self>) -> AnyElement {
        use super::local_run::data;
        let mode = data::mode(&self.path);
        let trigger = tile(
            &self.ribbon_nav,
            "ribbon-data",
            IconName::Database,
            mode.label(),
            cx,
        );
        let items = data::DataMode::ALL
            .into_iter()
            .map(|choice| {
                menu::item(choice.label())
                    .checked(choice == mode)
                    .hint(choice.hint())
                    .on_click(move |shell, cx| {
                        data::set_mode(&shell.path, choice);
                        cx.notify();
                    })
            })
            .collect();
        menu::dropdown_in_group(
            self,
            MenuId::TestData,
            super::chrome::Trigger::new(trigger).tooltip("Which DataStores local runs use"),
            items,
            cx,
        )
        .into_any_element()
    }

    /// Test › Controller: the Controller Emulator window.
    fn controller_tile(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        tile(
            &self.ribbon_nav,
            "ribbon-controller",
            IconName::Gamepad2,
            "Controller",
            cx,
        )
        .on_click(cx.listener(|shell, _, _, cx| shell.open_controller_emulator(cx)))
    }
}

/// The Test tab's tiles, in order.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum TestTile {
    Play,
    PlayLocal,
    ServerClients,
    Replay,
    Stop,
    Invite,
    View,
    Players,
    Data,
    Offline,
    Network,
    Device,
    Controller,
}

impl TestTile {
    /// Only there with the Local Run beta on: hidden without it, not greyed.
    pub(super) fn local_run(self) -> bool {
        use TestTile::*;
        matches!(
            self,
            PlayLocal | ServerClients | Replay | Players | Data | Offline
        )
    }
}

pub(super) fn test_tile_order(local_beta: bool) -> Vec<TestTile> {
    use TestTile::*;
    [
        Play,
        PlayLocal,
        ServerClients,
        Replay,
        Stop,
        Invite,
        View,
        Players,
        Data,
        Offline,
        Network,
        Device,
        Controller,
    ]
    .into_iter()
    .filter(|tile| local_beta || !tile.local_run())
    .collect()
}

/// The Play and Stop tiles in isolation, for the 44x44 layout check.
#[cfg(test)]
fn test_tiles() -> Vec<AnyElement> {
    vec![
        primary(
            "ribbon-play",
            unavailable_tile("ribbon-play", IconName::Play, "Play", "test"),
        )
        .into_any_element(),
        primary(
            "ribbon-stop",
            unavailable_tile("ribbon-stop", IconName::Square, "Stop", "test"),
        )
        .into_any_element(),
    ]
}

/// The tooltip of the Show UI tile for the overlay's current state.
fn show_ui_tooltip(shown: bool) -> &'static str {
    if shown {
        "Show UI \u{2014} hide the ScreenGuis drawn over the 3D view"
    } else {
        "Show UI \u{2014} draw the ScreenGuis over the 3D view again"
    }
}

/// The Fullbright tile's tooltip, with its key when it has one.
fn fullbright_tooltip(on: bool) -> String {
    let what = if on {
        "Fullbright \u{2014} light the 3D view with the place\u{2019}s own Lighting again"
    } else {
        "Fullbright \u{2014} flat light and no shadows, to see in a dark place. Lighting is not changed"
    };
    match crate::shortcuts::hint(crate::shortcuts::Command::Fullbright) {
        Some(key) => format!("{what} ({key})"),
        None => what.to_owned(),
    }
}

/// One insert-menu row, running the same [`RibbonCommand`] the command
/// palette lists for it.
fn insert_item(item: &'static Insert) -> menu::Item {
    menu::item(item.label)
        .icon(item.icon)
        .on_click(move |shell, cx| RibbonCommand::Insert(item).run(shell, cx))
}

/// The three built-in script classes, the way into the templates window,
/// then whatever the user has put in their templates directory, labelled
/// `"<name> (<class>)"` so two templates of different classes can share a
/// name without being ambiguous. Rebuilt every frame, so a template added on
/// disk shows up as soon as `Shell` reloads the list.
fn script_items(templates: &ScriptTemplates) -> Vec<menu::Item> {
    let mut items: Vec<menu::Item> = SCRIPT_INSERTS.iter().map(insert_item).collect();
    items.extend([
        menu::separator(),
        menu::item("Manage templates\u{2026}")
            .icon(IconName::Pencil)
            .on_click(|shell, cx| shell.open_script_templates(cx)),
    ]);
    if !templates.extras().is_empty() {
        items.push(menu::separator());
    }
    items.extend(
        templates
            .extras()
            .iter()
            .enumerate()
            .map(|(index, template)| {
                let icon = if template.class == "ModuleScript" {
                    IconName::Package
                } else {
                    IconName::FileCode
                };
                menu::item(format!("{} ({})", template.name, template.class))
                    .icon(icon)
                    .on_click(move |shell, cx| shell.insert_user_template(index, cx))
            }),
    );
    items
}

fn tool_icon(tool: Tool) -> IconName {
    match tool {
        Tool::Select => IconName::MousePointer2,
        Tool::Move => IconName::Move3d,
        Tool::Scale => IconName::Scale3d,
        Tool::Rotate => IconName::Rotate3d,
        // Roblox's own glyph is a bounding box with a handle at each corner
        // this project ships no Roblox icon assets (see `CONTRIBUTING.md`), so a
        // generic Lucide glyph standing for the same idea, a frame with its
        // own corner handles, is this editor's equivalent rather than a copy.
        Tool::Transform => IconName::SquareDashed,
        Tool::Sun => IconName::Sun,
        Tool::Pivot => IconName::LocateFixed,
        Tool::Terrain => IconName::Mountain,
    }
}

/// Each transform tool's own pastel, spent only on that tool's button while
/// it is the active one.
pub(super) fn tool_accent(tool: Tool) -> Rgba {
    match tool {
        Tool::Select => tokens::tool_select(),
        Tool::Move => tokens::tool_move(),
        Tool::Scale => tokens::tool_scale(),
        Tool::Rotate => tokens::tool_rotate(),
        Tool::Transform => tokens::tool_transform(),
        Tool::Sun => tokens::tool_sun(),
        Tool::Pivot => tokens::tool_pivot(),
        // Scale's green: the closest of the existing tool hues to terrain.
        Tool::Terrain => tokens::tool_scale(),
    }
}

/// Marks a tool button as the active one.
///
/// Three cues, not one: a wash of the tool's pastel, a 1.5px border in the
/// same pastel, and the icon itself tinted. WCAG 1.4.1 forbids colour as
/// the *only* means of conveying state, and the border is the part that
/// survives grayscale and every kind of colour blindness, the tools
/// already differ by icon shape, so between the two an active tool is
/// identifiable with no hue perception at all.
pub(super) fn selected(tile: Stateful<Div>, accent: Rgba) -> Stateful<Div> {
    tile.bg(tokens::tool_wash(accent))
        .border(tokens::tool_border())
        .border_color(accent)
        .text_color(accent)
}

/// A bare icon inside the transform-tools cluster: no fill or radius of its
/// own at rest, so the icons read as members of one shared card rather than
/// six tiles glued together, the border and the wash [`selected`] paints
/// are what a hover or an active tool adds on top of this.
/// A cluster icon whose tool can't be used right now: greyed, and a hover
/// or press lights nothing. Its tooltip says why, and a click on it does
/// nothing (`Shell::transform_action` refuses the tool too). Built on
/// [`cluster_frame`] rather than over [`cluster_icon`]: an element takes
/// one hover style, and a second one panics in a debug build.
pub(super) fn refused_icon(
    nav: &Roving,
    id: impl Into<ElementId>,
    icon: IconName,
    cx: &mut App,
) -> Stateful<Div> {
    let el = cluster_frame(id, icon)
        .cursor_not_allowed()
        .text_color(tokens::text_disabled());
    nav.claim(el, cx)
}

pub(super) fn cluster_icon(
    nav: &Roving,
    id: impl Into<ElementId>,
    icon: IconName,
    cx: &mut App,
) -> Stateful<Div> {
    let el = cluster_frame(id, icon)
        .cursor_pointer()
        .text_color(tokens::text_muted())
        .hover(|this| {
            tokens::hover_fx(this)
                .bg(tokens::hover())
                .text_color(tokens::text_full())
        })
        .active(|this| this.bg(tokens::ribbon_tab_active()));
    nav.claim(el, cx)
}

/// What [`cluster_icon`] and [`refused_icon`] share: the tile and its
/// glyph, with no pointer styles yet.
fn cluster_frame(id: impl Into<ElementId>, icon: IconName) -> Stateful<Div> {
    // 38x38, well over the WCAG floor: the icons this size sits next to
    // (Part, Script, UI, Toolbox) are drawn at a similar weight, and a
    // cluster of small glyphs beside them read as an afterthought.
    div()
        .id(id.into())
        .flex_none()
        .size(tokens::scaled_width(38.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(tokens::radius_tile())
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::dock())))
        .child(Icon::new(icon).size(tokens::scaled_width(17.)))
}

/// The frame's tile: icon over label, one hit target, full ribbon height.
pub(super) fn tile(
    nav: &Roving,
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    cx: &mut App,
) -> Stateful<Div> {
    nav.claim(base_tile(id, icon, label, true), cx)
}

/// A disabled control is deliberately **not** claimed into the roving
/// group: the APG's toolbar pattern focuses the first non-disabled control
/// on entry and arrows skip the rest, and a keyboard user landing on a
/// button that cannot do anything is a dead end with no way to tell why.
/// The tooltip still explains it on hover.
///
/// The same greyed, unclaimed tile with a reason on hover: either a command
/// that cannot act right now (nothing selected, an empty clipboard), which
/// the person at the keyboard can fix in a second, or one this editor can't
/// offer at all, where the reason names what is missing.
///
/// Offering the click and doing nothing is the worst of the options, which
/// is what this exists to stop.
/// A tile whose command needs Roblox: greyed out with "You're offline"
/// while offline (and out of the roving group, as [`unavailable_tile`]),
/// an ordinary [`tile`] with `tip` otherwise.
fn cloud_tile(
    nav: &Roving,
    id: &'static str,
    icon: IconName,
    label: &'static str,
    tip: &'static str,
    run: fn(&mut Shell, &mut Context<Shell>),
    cx: &mut Context<Shell>,
) -> AnyElement {
    if crate::offline::blocked() {
        return base_tile(id, icon, label, false)
            .tooltip(|window, cx| super::tooltip::text(crate::offline::TIP, window, cx))
            .into_any_element();
    }
    tile(nav, id, icon, label, cx)
        .tooltip(move |window, cx| super::tooltip::text(tip, window, cx))
        .on_click(cx.listener(move |shell, _, _, cx| run(shell, cx)))
        .into_any_element()
}

pub(super) fn unavailable_tile(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    reason: &'static str,
) -> Stateful<Div> {
    base_tile(id, icon, label, false)
        .tooltip(move |window, cx| super::tooltip::text(format!("{label} — {reason}"), window, cx))
}

fn base_tile(
    id: impl Into<ElementId>,
    icon: IconName,
    label: impl Into<SharedString>,
    enabled: bool,
) -> Stateful<Div> {
    tile_frame(id, icon_zone(icon, px(20.)), label, enabled)
}

/// [`tile`] with a badge beside its icon (the Network tile's connection
/// pill). The tile grows past the usual width to fit it, so the badge never
/// covers the icon whatever its text.
pub(super) fn tile_with_badge(
    nav: &Roving,
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    badge: impl IntoElement,
    cx: &mut App,
) -> Stateful<Div> {
    let row = h_flex()
        .flex_none()
        .gap(px(3.))
        .items_center()
        .child(icon_zone(icon, px(20.)))
        .child(badge);
    let tile = tile_frame(id, row, label, true).px(px(6.));
    nav.claim(tile, cx)
}

/// A tile's box, states and label, under `top` (its icon).
fn tile_frame(
    id: impl Into<ElementId>,
    top: impl IntoElement,
    label: impl Into<SharedString>,
    enabled: bool,
) -> Stateful<Div> {
    v_flex()
        .id(id.into())
        .relative()
        .flex_none()
        .min_w(tokens::tile_width())
        .h_full()
        .gap(px(6.))
        .px(px(5.))
        .py(px(5.))
        .items_center()
        .justify_center()
        .rounded(tokens::radius())
        .when_some(
            tokens::on_surface(tokens::tile(), tokens::chrome()),
            |this, bg| this.bg(bg),
        )
        .text_size(tokens::text_xs())
        .line_height(tokens::line_xs())
        .map(|this| {
            if enabled {
                // No `tab_index` here: the ribbon is one Tab stop and
                // `shell::roving` decides which of its controls currently
                // holds it.
                this.focus_visible(|this| this.shadow(tokens::focus_ring(tokens::chrome())))
                    .cursor_pointer()
                    .text_color(tokens::text_label())
                    .hover(|this| {
                        tokens::hover_fx(this)
                            .bg(tokens::hover())
                            .text_color(tokens::text_full())
                    })
                    .active(|this| this.bg(tokens::ribbon_tab_active()))
            } else {
                this.cursor_not_allowed()
                    .text_color(tokens::text_disabled())
            }
        })
        .group("ribbon-tile")
        .child(top)
        // The whole label, the tile as wide as it needs: Studio sizes its
        // buttons to their names rather than cutting them short.
        .child(
            div()
                .w_full()
                .text_center()
                .whitespace_nowrap()
                .child(label.into()),
        )
}

/// A ribbon button's icon, and its hover lift.
///
/// The spec asks for a 120ms 1.06x scale. GPUI has no element transform, so
/// the lift is a size change on the icon's own fixed box rather than a
/// transform, the label below it doesn't move, because the box the icon
/// fills is what grows. The vendored GPUI eases a hover's pixel sizes over
/// `tokens::DURATION_HOVER` (the spec's 120ms), given an id to keep the blend
/// in, and drops the easing under reduced motion; the lift itself is dropped
/// there too, since a jump is still a change and the background hover
/// carries the whole affordance without it.
fn icon_zone(icon: IconName, size: Pixels) -> Stateful<Div> {
    let lifted = px(f32::from(size) * 1.06);

    div()
        .id("icon-zone")
        .flex_none()
        .size(size)
        .flex()
        .items_center()
        .justify_center()
        .when(!tokens::reduced_motion(), |this| {
            this.group_hover("ribbon-tile", move |this| this.size(lifted))
        })
        .child(Icon::new(icon).size_full())
}

/// The column of narrow rows that sits beside a tile.
pub(super) fn stack(rows: Vec<Stateful<Div>>) -> Div {
    v_flex()
        .flex_none()
        .min_w(tokens::stack_width())
        .h_full()
        .gap(px(4.))
        .children(rows)
}

/// [`unavailable_tile`]'s stack row: the same reason, the same reasoning.
pub(super) fn unavailable_row(
    id: &'static str,
    icon: IconName,
    label: &'static str,
    reason: &'static str,
) -> Stateful<Div> {
    base_row(id, icon, label)
        .cursor_not_allowed()
        .text_color(tokens::text_disabled())
        .tooltip(move |window, cx| super::tooltip::text(format!("{label} — {reason}"), window, cx))
}

/// A stack row that does something: the same geometry as [`unavailable_row`] with
/// [`base_tile`]'s enabled styling (hover wash, pressed wash, keyboard focus
/// ring) and a place in the ribbon's roving group.
pub(super) fn live_stack_row(
    nav: &Roving,
    id: &'static str,
    icon: IconName,
    label: &'static str,
    cx: &mut App,
) -> Stateful<Div> {
    let row = base_row(id, icon, label)
        .focus_visible(|this| this.shadow(tokens::focus_ring(tokens::chrome())))
        .cursor_pointer()
        .text_color(tokens::text_label())
        .hover(|this| {
            tokens::hover_fx(this)
                .bg(tokens::hover())
                .text_color(tokens::text_full())
        })
        .active(|this| this.bg(tokens::ribbon_tab_active()));
    nav.claim(row, cx)
}

fn base_row(id: &'static str, icon: IconName, label: &'static str) -> Stateful<Div> {
    h_flex()
        .id(id)
        .w_full()
        .flex_1()
        .items_center()
        .gap(px(6.))
        .px(px(7.))
        .rounded(tokens::radius())
        .when_some(
            tokens::on_surface(tokens::tile(), tokens::chrome()),
            |this, bg| this.bg(bg),
        )
        .text_size(tokens::text_xs())
        .line_height(tokens::line_xs())
        .child(Icon::new(icon).size(tokens::text_xs()))
        .child(div().flex_1().whitespace_nowrap().child(label))
}

/// Between two groups: the frame's hairline, inset from the ribbon's own
/// padding so it stops short of both edges.
fn separator() -> AnyElement {
    div()
        .flex_none()
        .self_center()
        .w(px(1.))
        .h(tokens::separator_height())
        .bg(tokens::border())
        .into_any_element()
}

#[cfg(test)]
#[path = "ribbon_tests.rs"]
mod tests;
