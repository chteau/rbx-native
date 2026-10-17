//! Play/Test without Roblox Studio (`docs/research/play-sandbox/README.md`,
//! live-tested 2026-10-14): publish a probed copy of the place over the
//! user's private sandbox, open it in the Roblox client (Sober on Linux),
//! embed that window frameless in the viewport, and stream the game's
//! output back through a tunnel.
//!
//! Start (`play_start`), each step shown as `Phase::Starting(step)`:
//! 1. No sandbox → Output error "Choose a sandbox place in Settings ›
//!    Account first." and `open_settings`; stop.
//! 2. `Launcher::detect().client_running()` → set `confirm_close` and show
//!    `play_dialog` ("Roblox is already running" / "Play closes it and
//!    starts your test." / [Cancel] [Close and Play]); Close and Play calls
//!    `close_running_client` on a background task, then continues.
//! 3. The always-open link: the first Play starts a `ChannelServer`
//!    (`DEFAULT_HOLD`) and a cloudflared `Tunnel` (the bundled binary, see
//!    `play::cloudflared`; `$RBX_CLOUDFLARED` overrides it), waits `DNS_SETTLE`, then GETs
//!    `<url>/healthz` with the token until 200 (60 s budget). Kept in
//!    `Play::link` until the app quits; later Plays only `rotate_token`.
//! 4. `session::prepare` on a background task with a fresh `Token`, a
//!    session id `play-<unix ms>`, and the tunnel URL; its warnings go to
//!    Output as `Feedback::Warning`.
//! 5. Publish with `rbx_cloud::Client::publish_place(…, Published)` (key
//!    from `ApiKey::from_env_or_config`), then rename the sandbox place to
//!    `copy_name::render(template, place name)` when that differs from the
//!    name applied earlier this session; a failed rename only warns.
//! 6. Launch `ClientProcess::spawn_with(launcher, &join_uri(place, session),
//!    &play_client_options())`,
//!    after reading `Embedded::existing_clients()` (Linux) to ignore.
//! 7. Linux with an `embed_target`: `Embedded::attach(target.window,
//!    target.rect, &ignore, 90 s)` on a background task; on `NotFound` the
//!    client simply stays in its own window (Output warning).
//! 8. `Phase::Running`; a poll task every `POLL` drains `channel.events()`
//!    into Output via `session::feedback_for`, records `job_id` from
//!    `Hello`, applies `session::stop_for`, checks `client.exited()` and
//!    `embedded.alive()`, and moves the embedded window when the
//!    `embed_target` rect changed.
//!
//! Stop (`play_stop`): drops the session (kills the client when the
//! launcher tracks it, releases the embedded window), keeps the link,
//! writes `reason.message()` to Output, back to `Phase::Idle`. Any start
//! step failing stops with `StopReason::Failed`.

mod client_options;
pub(in crate::shell) mod cloudflared;
pub(crate) mod copy_name;
pub(crate) mod debug;
pub(crate) mod live;
pub(in crate::shell) mod network;
pub(crate) mod session;
#[cfg(target_os = "linux")]
mod slirp4netns;

use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gpui_kit::*;
use rbx_cloud::{ApiKey, Client, PlaceUpdate, PublishMode};
use rbx_play::channel::{ChannelServer, DEFAULT_HOLD};
use rbx_play::inject::ProbeConfig;
use rbx_play::launch::{join_server_uri, join_uri, ClientProcess, Launcher};
use rbx_play::protocol::{Side, Token, Uplink, TOKEN_HEADER};
use rbx_play::tunnel::{Tunnel, TunnelProvider};

use super::Shell;
use crate::command_bar::Feedback;
use crate::launcher::ui::{self, Weight};
use crate::settings::PlaySandbox;
use session::{StopReason, PLAY_SOURCE};

/// Event/health poll interval while running.
pub(super) const POLL: Duration = Duration::from_millis(100);
/// Wait after cloudflared prints its URL before the first lookup: an
/// earlier one caches "no such host" for about a minute (live-tested).
pub(super) const DNS_SETTLE: Duration = Duration::from_secs(10);

const TUNNEL_TIMEOUT: Duration = Duration::from_secs(60);
const HEALTH_BUDGET: Duration = Duration::from_secs(60);
const HEALTH_RETRY: Duration = Duration::from_secs(1);
#[cfg(any(target_os = "linux", windows))]
const EMBED_TIMEOUT: Duration = Duration::from_secs(90);

/// Where the viewport is, for embedding: the native window it is drawn in
/// (X11 id or `HWND`) and its rect there in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EmbedTarget {
    pub(crate) window: crate::pointer_lock::WindowId,
    pub(crate) rect: session::rbx_play_rect::Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Phase {
    Idle,
    /// A start step in progress, e.g. "Opening the tunnel…",
    /// "Publishing to the sandbox…", "Starting Roblox…".
    Starting(&'static str),
    Running,
}

/// The channel and the tunnel to it, open from the first Play until quit.
struct Link {
    channel: Rc<ChannelServer>,
    _tunnel: Tunnel,
    url: String,
}

/// One running Play.
struct Session {
    _token: Token,
    #[cfg(any(target_os = "linux", windows))]
    embedded: Option<rbx_play::embed::Embedded>,
    /// Gives the embedded client the keyboard when it is clicked.
    #[cfg(any(target_os = "linux", windows))]
    focus: Option<rbx_play::embed::FocusFollow>,
    client: ClientProcess,
    own_user: Option<u64>,
    last_rect: Option<session::rbx_play_rect::Rect>,
    /// The embedded window is hidden because another document (Scripts,
    /// UI Editor, Animation) took the room the 3D view had.
    #[cfg(any(target_os = "linux", windows))]
    hidden: bool,
}

/// The latest `Uplink::Stats` of each side, shown in the Test ribbon.
#[derive(Debug, Default)]
pub(super) struct PlayStats {
    pub(super) server: Option<rbx_play::protocol::Metrics>,
    pub(super) client: Option<rbx_play::protocol::Metrics>,
}

pub(super) struct Play {
    /// The chosen sandbox; persisted in settings.
    pub(super) sandbox: Option<PlaySandbox>,
    /// The name template for the sandbox while it runs a test (see
    /// `copy_name`); persisted in settings.
    pub(super) copy_name: String,
    /// The last test-copy name applied to the sandbox this session.
    named: Option<String>,
    pub(super) phase: Phase,
    /// The "Roblox is already running" question is showing.
    pub(super) confirm_close: bool,
    /// The editor window Play was pressed in (X11 id or `HWND`); the client
    /// is embedded there, over the 3D view.
    main_window: Option<crate::pointer_lock::WindowId>,
    /// The same window as GPUI knows it, for `gpui_base::occlusion`.
    gpui_window: Option<gpui_kit::WindowId>,
    /// The running server's `game.JobId`, once its hello arrived.
    pub(super) job_id: Option<String>,
    /// The Client/Server views of the running game (`play::live`).
    pub(super) live: live::Live,
    /// The scripts debugged in this test (`play::debug`).
    pub(super) debug: debug::PlayDebug,
    /// The Network Simulator (`play::network`).
    pub(super) network: network::NetworkSim,
    /// The window Play was pressed in, for work that needs a `Window` from
    /// the poll (opening a script where the game paused).
    window_handle: Option<AnyWindowHandle>,
    /// Each side's performance, as last reported.
    pub(super) stats: PlayStats,
    /// The window's scale factor when Play was pressed, to turn popups'
    /// logical bounds into the embedded window's device pixels.
    scale: f32,
    /// The holes last cut out of the embedded window (`play_update_holes`).
    #[cfg(any(target_os = "linux", windows))]
    holes: Vec<rbx_play::embed::Rect>,
    link: Option<Link>,
    session: Option<Session>,
    poll: Option<Task<()>>,
    /// Bumped by every start and stop, so a start chain still in flight
    /// notices it was cancelled.
    attempt: u64,
    /// The launcher detected for the start waiting on `confirm_close`.
    pending_launcher: Option<Launcher>,
}

impl Play {
    pub(super) fn new(
        sandbox: Option<PlaySandbox>,
        copy_name: String,
        network_presets: Vec<crate::settings::SavedNetworkPreset>,
    ) -> Self {
        Play {
            network: network::NetworkSim::new(network_presets),
            sandbox,
            copy_name,
            named: None,
            phase: Phase::Idle,
            confirm_close: false,
            main_window: None,
            gpui_window: None,
            live: live::Live::new(),
            debug: debug::PlayDebug::default(),
            window_handle: None,
            stats: PlayStats::default(),
            scale: 1.0,
            #[cfg(any(target_os = "linux", windows))]
            holes: Vec::new(),
            job_id: None,
            link: None,
            session: None,
            poll: None,
            attempt: 0,
            pending_launcher: None,
        }
    }
}

/// Sets the shown step; false when this start was cancelled.
fn step(this: &WeakEntity<Shell>, cx: &mut AsyncApp, attempt: u64, step: &'static str) -> bool {
    this.update(cx, |shell, cx| {
        if shell.play.attempt != attempt {
            return false;
        }
        shell.play.phase = Phase::Starting(step);
        cx.notify();
        true
    })
    .unwrap_or(false)
}

fn fail(this: &WeakEntity<Shell>, cx: &mut AsyncApp, attempt: u64, text: String) {
    let _ = this.update(cx, |shell, cx| {
        if shell.play.attempt == attempt {
            shell.play_stop(StopReason::Failed(text), cx);
        }
    });
}

/// Blocking: GETs `/healthz` until it answers 200 or the budget runs out.
fn wait_healthy(url: &str, token: &str) -> bool {
    let deadline = Instant::now() + HEALTH_BUDGET;
    loop {
        if ureq::get(url).header(TOKEN_HEADER, token).call().is_ok() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(HEALTH_RETRY);
    }
}

fn session_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    format!("play-{millis}")
}

impl Shell {
    /// The ribbon's Play tile.
    pub(in crate::shell) fn play_start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.play.phase != Phase::Idle {
            return;
        }
        self.play.main_window = crate::pointer_lock::window_id(window);
        self.play.window_handle = Some(window.window_handle());
        self.play.gpui_window = Some(window.window_handle().window_id());
        self.play.scale = window.scale_factor();
        if self.play.sandbox.is_none() {
            self.output.push(
                PLAY_SOURCE,
                Feedback::Error(
                    "Choose a sandbox place in Settings \u{203a} Account first.".to_owned(),
                ),
            );
            self.open_settings_on("account", cx);
            cx.notify();
            return;
        }
        self.play.attempt += 1;
        let attempt = self.play.attempt;
        self.play.phase = Phase::Starting("Checking for Roblox\u{2026}");
        cx.notify();
        cx.spawn(async move |this, cx| {
            let (launcher, running) = cx
                .background_spawn(async move {
                    let launcher = Launcher::detect();
                    (launcher, launcher.client_running())
                })
                .await;
            let _ = this.update(cx, |shell, cx| {
                if shell.play.attempt != attempt {
                    return;
                }
                if running {
                    shell.play.pending_launcher = Some(launcher);
                    shell.play.confirm_close = true;
                    cx.notify();
                } else {
                    shell.play_continue(launcher, false, cx);
                }
            });
        })
        .detach();
    }

    fn play_cancel_close(&mut self, cx: &mut Context<Self>) {
        self.play.confirm_close = false;
        self.play.pending_launcher = None;
        self.play.attempt += 1;
        self.play.phase = Phase::Idle;
        cx.notify();
    }

    fn play_close_and_continue(&mut self, cx: &mut Context<Self>) {
        self.play.confirm_close = false;
        match self.play.pending_launcher.take() {
            Some(launcher) => self.play_continue(launcher, true, cx),
            None => cx.notify(),
        }
    }

    /// Steps 3 to 8 of the module doc.
    fn play_continue(&mut self, launcher: Launcher, close_first: bool, cx: &mut Context<Self>) {
        let attempt = self.play.attempt;
        let Some(sandbox) = self.play.sandbox else {
            return self.play_stop(
                StopReason::Failed("no sandbox place is chosen.".to_owned()),
                cx,
            );
        };
        let token = Token::random();
        let (fresh, known_url) = match &self.play.link {
            Some(link) => {
                link.channel.rotate_token(token.clone());
                // Whatever an earlier session left unread is not this one's.
                while link.channel.events().try_recv().is_ok() {}
                (None, link.url.clone())
            }
            None => match ChannelServer::start(token.clone(), DEFAULT_HOLD) {
                Ok(channel) => (Some(Rc::new(channel)), String::new()),
                Err(err) => {
                    return self.play_stop(
                        StopReason::Failed(format!("the local channel could not start: {err}.")),
                        cx,
                    );
                }
            },
        };
        let template = self.play.copy_name.clone();
        let named = self.play.named.clone().unwrap_or_default();
        let title = self.title.to_string();
        let place_name = [".rbxlx", ".rbxl", ".rbxm"]
            .iter()
            .find_map(|ext| title.strip_suffix(ext))
            .unwrap_or(&title)
            .to_owned();
        let dom = self.dom.clone();
        let debug_requests = self.play_debug_requests();
        cx.spawn(async move |this, cx| {
            if close_first {
                if !step(&this, cx, attempt, "Closing Roblox\u{2026}") {
                    return;
                }
                let closed = cx
                    .background_spawn(async move { launcher.close_running_client() })
                    .await;
                if let Err(err) = closed {
                    return fail(
                        &this,
                        cx,
                        attempt,
                        format!("Roblox could not be closed: {err}."),
                    );
                }
            }

            let base_url = match fresh {
                Some(channel) => {
                    if !step(&this, cx, attempt, "Opening the tunnel\u{2026}") {
                        return;
                    }
                    let port = channel.port();
                    let started = cx
                        .background_spawn(async move {
                            let binary = cloudflared::program().map_err(|err| {
                                format!("the bundled cloudflared could not be unpacked: {err}")
                            })?;
                            TunnelProvider::Cloudflared { binary }
                                .start(port, TUNNEL_TIMEOUT)
                                .map_err(|err| err.to_string())
                        })
                        .await;
                    let tunnel = match started {
                        Ok(tunnel) => tunnel,
                        Err(err) => return fail(&this, cx, attempt, format!("{err}.")),
                    };
                    let url = tunnel.public_url.trim_end_matches('/').to_owned();
                    if !step(&this, cx, attempt, "Waiting for the tunnel\u{2026}") {
                        return;
                    }
                    cx.background_executor().timer(DNS_SETTLE).await;
                    let health = format!("{url}/healthz");
                    let presented = token.as_str().to_owned();
                    let healthy = cx
                        .background_spawn(async move { wait_healthy(&health, &presented) })
                        .await;
                    if !healthy {
                        return fail(
                            &this,
                            cx,
                            attempt,
                            "the tunnel did not answer within 60 seconds.".to_owned(),
                        );
                    }
                    let link = Link {
                        channel,
                        _tunnel: tunnel,
                        url: url.clone(),
                    };
                    let stored = this.update(cx, |shell, _| {
                        if shell.play.link.is_none() {
                            shell.play.link = Some(link);
                        }
                    });
                    if stored.is_err() {
                        return;
                    }
                    url
                }
                None => known_url,
            };

            if !step(&this, cx, attempt, "Preparing the place\u{2026}") {
                return;
            }
            let session = session_id();
            let probe = ProbeConfig {
                base_url,
                token: token.as_str().to_owned(),
                session: session.clone(),
            };
            let prepared = match cx
                .background_spawn(async move { session::prepare(&dom, &probe, &debug_requests) })
                .await
            {
                Ok(prepared) => prepared,
                Err(text) => return fail(&this, cx, attempt, text),
            };
            let warnings = prepared.warnings.clone();
            let debugged = prepared.debug.clone();
            let _ = this.update(cx, |shell, cx| {
                shell.play.debug.pending = debugged;
                for warning in warnings {
                    shell.output.push(PLAY_SOURCE, Feedback::Warning(warning));
                }
                cx.notify();
            });

            if !step(&this, cx, attempt, "Publishing to the sandbox\u{2026}") {
                return;
            }
            let (universe_id, place_id) = (sandbox.universe_id, sandbox.place_id);
            let wanted = copy_name::render(&template, &place_name).filter(|name| *name != named);
            let published = cx
                .background_spawn(async move {
                    let Some(key) = ApiKey::from_env_or_config() else {
                        return Err("no Open Cloud API key is set up.".to_owned());
                    };
                    let client = Client::new(Some(key));
                    client
                        .publish_place(
                            universe_id,
                            place_id,
                            &prepared.bytes,
                            PublishMode::Published,
                        )
                        .map_err(|err| format!("publishing to the sandbox failed: {err}"))?;
                    // A server from an earlier Play still runs the old probe,
                    // whose token no longer opens the channel; joining it
                    // would be a session that never reports.
                    let restart = client.restart_servers(universe_id, true).err().map(|err| {
                        format!(
                            "Old test servers could not be restarted ({err}); give your API key the \
                             universe:write scope so Play never lands in a stale server."
                        )
                    });
                    let rename = wanted.map(|name| {
                        let update = PlaceUpdate {
                            display_name: Some(name.clone()),
                            ..Default::default()
                        };
                        match client.update_place_settings(universe_id, place_id, &update) {
                            Ok(_) => Ok(name),
                            Err(err) => Err(format!(
                                "The sandbox could not be renamed to \u{201c}{name}\u{201d} ({err}); \
                                 its API key needs the universe.place:write scope."
                            )),
                        }
                    });
                    Ok((restart, rename))
                })
                .await;
            let (restart, rename) = match published {
                Err(text) => return fail(&this, cx, attempt, text),
                Ok(done) => done,
            };
            let _ = this.update(cx, |shell, cx| {
                if let Some(warning) = restart {
                    shell.output.push(PLAY_SOURCE, Feedback::Warning(warning));
                }
                match rename {
                    Some(Ok(name)) => shell.play.named = Some(name),
                    Some(Err(warning)) => {
                        shell.output.push(PLAY_SOURCE, Feedback::Warning(warning));
                    }
                    None => {}
                }
                cx.notify();
            });

            if !step(&this, cx, attempt, "Starting Roblox\u{2026}") {
                return;
            }
            let uri = join_uri(place_id, &session);
            let Ok(options) = this.update(cx, |shell, cx| shell.play_client_options(cx)) else {
                return;
            };
            let launched = cx
                .background_spawn(async move {
                    #[cfg(any(target_os = "linux", windows))]
                    let ignore = rbx_play::embed::Embedded::existing_clients().unwrap_or_default();
                    #[cfg(not(any(target_os = "linux", windows)))]
                    let ignore: Vec<crate::pointer_lock::WindowId> = Vec::new();
                    ClientProcess::spawn_with(launcher, &uri, &options).map(|client| (client, ignore))
                })
                .await;
            let (client, ignore) = match launched {
                Ok(launched) => launched,
                Err(err) => {
                    return fail(
                        &this,
                        cx,
                        attempt,
                        format!("Roblox could not be started: {err}."),
                    );
                }
            };
            let target = this.update(cx, |shell, cx| {
                if shell.play.attempt != attempt {
                    return Err(());
                }
                shell.play.session = Some(Session {
                    _token: token,
                    #[cfg(any(target_os = "linux", windows))]
                    embedded: None,
                    #[cfg(any(target_os = "linux", windows))]
                    focus: None,
                    client,
                    own_user: None,
                    last_rect: None,
                    #[cfg(any(target_os = "linux", windows))]
                    hidden: false,
                });
                Ok(shell.play_embed_target(cx))
            });
            let Ok(Ok(target)) = target else {
                return;
            };

            #[cfg(any(target_os = "linux", windows))]
            if let Some(target) = target {
                if !step(&this, cx, attempt, "Embedding the Roblox window\u{2026}") {
                    return;
                }
                let attached = cx
                    .background_spawn(async move {
                        rbx_play::embed::Embedded::attach(
                            target.window,
                            target.rect,
                            &ignore,
                            EMBED_TIMEOUT,
                        )
                    })
                    .await;
                let _ = this.update(cx, |shell, cx| {
                    if shell.play.attempt != attempt {
                        return;
                    }
                    match attached {
                        Ok(embedded) => {
                            let focus = rbx_play::embed::FocusFollow::start(
                                target.window,
                                embedded.window(),
                            );
                            if let Err(err) = &focus {
                                shell.output.push(
                                    PLAY_SOURCE,
                                    Feedback::Warning(format!(
                                        "{err} Click the game and it may not get the keyboard."
                                    )),
                                );
                            }
                            if let Some(running) = shell.play.session.as_mut() {
                                running.embedded = Some(embedded);
                                running.focus = focus.ok();
                                running.last_rect = Some(target.rect);
                            }
                        }
                        Err(err) => shell.output.push(
                            PLAY_SOURCE,
                            Feedback::Warning(format!("{err} Roblox stays in its own window.")),
                        ),
                    }
                    cx.notify();
                });
            }
            #[cfg(not(any(target_os = "linux", windows)))]
            let _ = (target, ignore);

            let _ = this.update(cx, |shell, cx| {
                if shell.play.attempt != attempt {
                    return;
                }
                shell.play.phase = Phase::Running;
                shell.live_begin(cx);
                shell.play_debug_begin(cx);
                shell.play.poll = Some(shell.play_poll(cx));
                cx.notify();
            });
        })
        .detach();
    }

    fn play_poll(&mut self, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |this, cx| loop {
            cx.background_executor().timer(POLL).await;
            let keep = this
                .update(cx, |shell, cx| shell.play_tick(cx))
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
    }

    /// One poll of a running session; false once it is over.
    /// Where the client goes: the 3D view's panel in the window Play was
    /// pressed in, once that panel has been laid out.
    fn play_embed_target(&self, cx: &App) -> Option<EmbedTarget> {
        let window = self.play.main_window?;
        let (x, y, width, height) = self.viewport.read(cx).panel_rect();
        if width == 0 || height == 0 {
            return None;
        }
        Some(EmbedTarget {
            window,
            rect: session::rbx_play_rect::Rect {
                x: i32::try_from(x).ok()?,
                y: i32::try_from(y).ok()?,
                width,
                height,
            },
        })
    }

    fn play_tick(&mut self, cx: &mut Context<Self>) -> bool {
        #[cfg(any(target_os = "linux", windows))]
        let target = self.play_embed_target(cx);
        let Some(channel) = self.play.link.as_ref().map(|link| Rc::clone(&link.channel)) else {
            return false;
        };
        let mut stop = None;
        let mut changed = false;
        while let Ok(event) = channel.events().try_recv() {
            changed = true;
            if self.live_apply(&event, cx) || self.play_debug_event(&event, cx) {
                continue;
            }
            if let Uplink::Stats { side, metrics } = event {
                let slot = match side {
                    Side::Server => &mut self.play.stats.server,
                    Side::Client => &mut self.play.stats.client,
                };
                *slot = Some(metrics);
                continue;
            }
            if let Some((source, row)) = session::feedback_for(&event) {
                match &event {
                    Uplink::Log {
                        side,
                        parts,
                        origin,
                        ..
                    } => {
                        let context = match side {
                            Side::Server => super::output::game::Context::Server,
                            Side::Client => super::output::game::Context::Client,
                        };
                        self.output.push_game(
                            context,
                            row,
                            super::output::game::print_parts(parts),
                            origin.as_ref().map(|o| super::output::game::Location {
                                path: o.script.clone(),
                                line: o.line,
                            }),
                        );
                    }
                    _ => self.output.push(source, row),
                }
            }
            if let Uplink::Hello { job_id, .. } = &event {
                if !job_id.is_empty() {
                    self.play.job_id = Some(job_id.clone());
                }
            }
            if let Some(running) = self.play.session.as_mut() {
                if stop.is_none() {
                    stop = session::stop_for(&event, &mut running.own_user);
                }
            }
        }
        // The game replaces the 3D view only: another document hides it.
        #[cfg(any(target_os = "linux", windows))]
        let on_viewport = self.document == super::chrome::Document::Viewport;
        let Some(running) = self.play.session.as_mut() else {
            return false;
        };
        if stop.is_none() && running.client.exited() {
            stop = Some(StopReason::ClientExited);
        }
        #[cfg(any(target_os = "linux", windows))]
        if stop.is_none() {
            if let Some(embedded) = &running.embedded {
                if !embedded.alive() {
                    stop = Some(StopReason::WindowClosed);
                } else if !on_viewport {
                    if !running.hidden {
                        embedded.hide();
                        running.hidden = true;
                    }
                } else if let Some(target) = target {
                    if running.last_rect != Some(target.rect) || running.hidden {
                        let _ = embedded.set_rect(target.rect);
                        running.last_rect = Some(target.rect);
                    }
                    if running.hidden {
                        embedded.show();
                        running.hidden = false;
                    }
                }
            }
        }
        #[cfg(not(any(target_os = "linux", windows)))]
        let _ = &running.last_rect;
        if let Some(reason) = stop {
            self.play_stop(reason, cx);
            return false;
        }
        self.live_tick(cx);
        #[cfg(any(target_os = "linux", windows))]
        self.play_update_holes();
        if changed {
            cx.notify();
        }
        true
    }

    /// Cuts the editor's popups out of the embedded game window, so they
    /// show above it (`gpui_base::occlusion` records where they were
    /// painted). Called after every frame while embedded and on each poll.
    #[cfg(any(target_os = "linux", windows))]
    pub(in crate::shell) fn play_update_holes(&mut self) {
        let (Some(window), Some(running)) = (self.play.gpui_window, self.play.session.as_ref())
        else {
            return;
        };
        let (Some(embedded), Some(rect)) = (&running.embedded, running.last_rect) else {
            return;
        };
        if running.hidden {
            return;
        }
        let scale = self.play.scale;
        let holes: Vec<rbx_play::embed::Rect> = gpui_base::occlusion::rects(window)
            .into_iter()
            .filter_map(|bounds| {
                let left = (f32::from(bounds.origin.x) * scale).floor() as i32 - rect.x;
                let top = (f32::from(bounds.origin.y) * scale).floor() as i32 - rect.y;
                let width = (f32::from(bounds.size.width) * scale).ceil() as i32;
                let height = (f32::from(bounds.size.height) * scale).ceil() as i32;
                let right = (left + width).min(rect.width as i32);
                let bottom = (top + height).min(rect.height as i32);
                let (left, top) = (left.max(0), top.max(0));
                (right > left && bottom > top).then(|| rbx_play::embed::Rect {
                    x: left,
                    y: top,
                    width: (right - left) as u32,
                    height: (bottom - top) as u32,
                })
            })
            .collect();
        if holes != self.play.holes && embedded.set_holes(&holes).is_ok() {
            self.play.holes = holes;
        }
    }

    /// The ribbon's Stop tile and every automatic stop.
    pub(in crate::shell) fn play_stop(&mut self, reason: StopReason, cx: &mut Context<Self>) {
        self.play.attempt += 1;
        self.play.poll = None;
        self.live_end(cx);
        self.play_debug_end(cx);
        #[cfg(any(target_os = "linux", windows))]
        self.play.holes.clear();
        if let Some(running) = self.play.session.take() {
            // Tell that session's server to close, so the next Play can't be
            // matched into it; its probe still holds the current token.
            if let Some(link) = &self.play.link {
                link.channel.send(rbx_play::protocol::CommandBody::Shutdown);
            }
            // Hidden before the client is killed and the window handed back
            // to the root, so nothing flashes on the desktop.
            #[cfg(any(target_os = "linux", windows))]
            #[cfg_attr(windows, allow(clippy::drop_non_drop))]
            drop(running.focus);
            #[cfg(any(target_os = "linux", windows))]
            if let Some(embedded) = &running.embedded {
                embedded.hide();
            }
            drop(running.client);
            #[cfg(any(target_os = "linux", windows))]
            drop(running.embedded);
        }
        self.play.job_id = None;
        self.play.stats = PlayStats::default();
        self.play.confirm_close = false;
        self.play.pending_launcher = None;
        self.output
            .push(PLAY_SOURCE, Feedback::Output(reason.message()));
        self.play.phase = Phase::Idle;
        cx.notify();
    }

    /// The ribbon's "Invite" tile: copies `join_server_uri(sandbox place,
    /// job_id)` to the clipboard and says so in Output.
    pub(in crate::shell) fn play_copy_join_link(&mut self, cx: &mut Context<Self>) {
        match (&self.play.sandbox, &self.play.job_id) {
            (Some(sandbox), Some(job_id)) => {
                let uri = join_server_uri(sandbox.place_id, job_id);
                cx.write_to_clipboard(ClipboardItem::new_string(uri.clone()));
                self.output.push(
                    PLAY_SOURCE,
                    Feedback::Output(format!("Copied the join link for this server: {uri}")),
                );
            }
            _ => self.output.push(
                PLAY_SOURCE,
                Feedback::Warning("There is no running Play server to invite to yet.".to_owned()),
            ),
        }
        cx.notify();
    }

    /// Settings › Account › Play sandbox: stores it and saves settings.
    pub(in crate::shell) fn set_play_sandbox(
        &mut self,
        sandbox: Option<PlaySandbox>,
        cx: &mut Context<Self>,
    ) {
        self.play.sandbox = sandbox;
        self.save_settings();
        cx.notify();
    }

    /// Settings › Files › Play › Name for test copies: stores it and saves settings.
    pub(in crate::shell) fn set_play_copy_name(
        &mut self,
        template: String,
        cx: &mut Context<Self>,
    ) {
        self.play.copy_name = template;
        self.save_settings();
        cx.notify();
    }

    /// The "Roblox is already running" dialog, while `confirm_close`.
    pub(in crate::shell) fn play_dialog(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.play.confirm_close {
            return None;
        }
        Some(
            ui::dialog(
                440.,
                ui::dialog_glyph("triangle-alert", ui::accent(), ui::wash()),
                "Roblox is already running",
                "Play closes it and starts your test.".to_owned(),
                None,
                vec![
                    ui::button("play-cancel", "Cancel", Weight::Secondary, false)
                        .on_click(cx.listener(|shell, _, _, cx| shell.play_cancel_close(cx)))
                        .into_any_element(),
                    ui::button("play-close", "Close and Play", Weight::Primary, false)
                        .on_click(cx.listener(|shell, _, _, cx| shell.play_close_and_continue(cx)))
                        .into_any_element(),
                ],
            )
            .into_any_element(),
        )
    }
}
