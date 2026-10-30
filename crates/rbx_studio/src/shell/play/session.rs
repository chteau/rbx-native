//! The pure half of a Play session: preparing the place, turning probe
//! messages into Output rows, deciding when a session is over, and the
//! viewport rect the client window is embedded at. No GPUI here.

use rbx_dom::{Ref, WeakDom};
use rbx_play::inject::{Preflight, ProbeConfig};
use rbx_play::instrument::Hook;
use rbx_play::protocol::{LogLevel, Side, Uplink};

use crate::command_bar::Feedback;

/// The Output dock source names for probe messages.
pub(crate) const SERVER_SOURCE: &str = "Server";
pub(crate) const CLIENT_SOURCE: &str = "Client";
pub(crate) const PLAY_SOURCE: &str = "Play";

/// The probed copy of the place, ready to publish.
#[derive(Debug)]
pub(crate) struct Prepared {
    pub(crate) bytes: Vec<u8>,
    /// `preflight_text` of every `Incomplete` finding (publish still goes),
    /// and every script that could not be instrumented for debugging.
    pub(crate) warnings: Vec<String>,
    /// The scripts published with debugger hooks.
    pub(crate) debug: Vec<DebugScript>,
}

/// A script to debug during Play: its breakpoints when Play started.
#[derive(Debug, Clone)]
pub(crate) struct DebugRequest {
    pub(crate) script: Ref,
    pub(crate) hooks: Vec<Hook>,
}

/// A script published with debugger hooks (`rbx_play::instrument`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DebugScript {
    /// The id its hooks report, from 1.
    pub(crate) id: u32,
    /// The script in the edited place (the copy keeps its refs).
    pub(crate) script: Ref,
    /// Where a breakpoint can stop (`Instrumented::lines`).
    pub(crate) lines: Vec<u32>,
}

/// Rewrites each requested script's `Source` in `copy` with debugger hooks.
/// A script that does not parse, or whose hooked text Luau rejects, keeps
/// its source and is reported in the returned warnings.
pub(crate) fn instrument_scripts(
    copy: &mut WeakDom,
    requests: &[DebugRequest],
) -> (Vec<DebugScript>, Vec<String>) {
    let mut scripts = Vec::new();
    let mut warnings = Vec::new();
    for request in requests {
        let Some(source) = crate::script_editor::source::read(copy, request.script) else {
            continue;
        };
        let name = copy
            .get(request.script)
            .map(|i| i.name().to_string())
            .unwrap_or_default();
        let id = scripts.len() as u32 + 1;
        let instrumented =
            rbx_play::instrument::instrument(&source, id, &request.hooks).and_then(|done| {
                rbx_lua::compile_check(&done.source, &name)?;
                Ok(done)
            });
        match instrumented {
            Ok(done) => {
                if crate::script_editor::source::write(copy, request.script, &done.source) {
                    scripts.push(DebugScript {
                        id,
                        script: request.script,
                        lines: done.lines,
                    });
                }
            }
            Err(err) => warnings.push(format!(
                "{name} runs without breakpoints in this test: it could not be prepared for \
                 debugging ({err})."
            )),
        }
    }
    (scripts, warnings)
}

/// Game scripts' MarketplaceService goes through the probe's purchase
/// stand-in, see `rbx_play::market`. Rewrites every script in `copy` that
/// uses it; a script that cannot be rewritten keeps its source and is
/// reported in the returned warnings.
pub(crate) fn mock_purchases(copy: &mut WeakDom) -> Vec<String> {
    let mut warnings = Vec::new();
    for (script, name) in game_scripts(copy) {
        let Some(source) = crate::script_editor::source::read(copy, script) else {
            continue;
        };
        let result = rbx_play::market::rewrite(&source).and_then(|rewritten| match rewritten {
            Some(new) => {
                rbx_lua::compile_check(&new, &name).map_err(|err| err.to_string())?;
                Ok(Some(new))
            }
            None => Ok(None),
        });
        match result {
            Ok(Some(new)) => {
                crate::script_editor::source::write(copy, script, &new);
            }
            Ok(None) => {}
            Err(err) => warnings.push(format!(
                "{name} calls MarketplaceService but could not be prepared for test purchases \
                 ({err}); its purchases are not mocked in this test."
            )),
        }
    }
    warnings
}

/// Every script of the game's own (not the probe's) with its name.
fn game_scripts(copy: &WeakDom) -> Vec<(Ref, String)> {
    let mut stack: Vec<Ref> = copy.root_refs().to_vec();
    let mut scripts = Vec::new();
    while let Some(r) = stack.pop() {
        let Some(inst) = copy.get(r) else {
            continue;
        };
        stack.extend(inst.children().iter().copied());
        if matches!(inst.class(), "Script" | "LocalScript" | "ModuleScript")
            && !inst.name().starts_with("__rbxnative")
        {
            scripts.push((r, inst.name().to_string()));
        }
    }
    scripts
}

/// Game scripts' `print` goes through the probe's structured stand-in, so a
/// printed table reaches the Output as a tree (see `rbx_play::print`). A
/// script that cannot be rewritten keeps its source and is reported in the
/// returned warnings.
pub(crate) fn structured_prints(copy: &mut WeakDom) -> Vec<String> {
    let mut warnings = Vec::new();
    for (script, name) in game_scripts(copy) {
        let Some(source) = crate::script_editor::source::read(copy, script) else {
            continue;
        };
        let result = rbx_play::print::rewrite(&source).and_then(|rewritten| match rewritten {
            Some(new) => {
                rbx_lua::compile_check(&new, &name).map_err(|err| err.to_string())?;
                Ok(Some(new))
            }
            None => Ok(None),
        });
        match result {
            Ok(Some(new)) => {
                crate::script_editor::source::write(copy, script, &new);
            }
            Ok(None) => {}
            Err(err) => warnings.push(format!(
                "{name} could not be prepared to print tables as trees ({err}); its printed \
                 tables show as plain text in this test."
            )),
        }
    }
    warnings
}

/// Game scripts that say `rbxnative` get the probe's `shared.rbxnative`
/// set before their first line runs (see `rbx_play::record`). A script that
/// cannot be rewritten keeps its source and is reported in the warnings.
pub(crate) fn playtest_marks(copy: &mut WeakDom) -> Vec<String> {
    let mut warnings = Vec::new();
    for (script, name) in game_scripts(copy) {
        let Some(source) = crate::script_editor::source::read(copy, script) else {
            continue;
        };
        let result = rbx_play::record::rewrite(&source).and_then(|rewritten| match rewritten {
            Some(new) => {
                rbx_lua::compile_check(&new, &name).map_err(|err| err.to_string())?;
                Ok(Some(new))
            }
            None => Ok(None),
        });
        match result {
            Ok(Some(new)) => {
                crate::script_editor::source::write(copy, script, &new);
            }
            Ok(None) => {}
            Err(err) => warnings.push(format!(
                "{name} could not be prepared for playtest marks ({err}); its \
                 rbxnative.mark calls are skipped in this test."
            )),
        }
    }
    warnings
}

/// Game scripts' UserInputService goes through the probe's touch stand-in,
/// so a game's own touch code runs on a simulated touch device (see
/// `rbx_play::touch`). Scripts inside a forked PlayerModule are left alone.
/// A script that cannot be rewritten keeps its source and is reported in
/// the returned warnings.
pub(crate) fn simulated_touches(copy: &mut WeakDom) -> Vec<String> {
    let mut warnings = Vec::new();
    for (script, name) in game_scripts(copy) {
        if in_skipped_subtree(copy, script) {
            continue;
        }
        let Some(source) = crate::script_editor::source::read(copy, script) else {
            continue;
        };
        let result = rbx_play::touch::rewrite(&source).and_then(|rewritten| match rewritten {
            Some(new) => {
                rbx_lua::compile_check(&new, &name).map_err(|err| err.to_string())?;
                Ok(Some(new))
            }
            None => Ok(None),
        });
        match result {
            Ok(Some(new)) => {
                crate::script_editor::source::write(copy, script, &new);
            }
            Ok(None) => {}
            Err(err) => warnings.push(format!(
                "{name} uses UserInputService but could not be prepared for simulated touches \
                 ({err}); on a simulated touch device it sees the mouse in this test."
            )),
        }
    }
    warnings
}

/// True when `script` is, or sits inside, one of `touch::SKIPPED_SUBTREES`.
fn in_skipped_subtree(copy: &WeakDom, script: Ref) -> bool {
    let mut at = Some(script);
    while let Some(r) = at {
        let Some(inst) = copy.get(r) else {
            return false;
        };
        if rbx_play::touch::SKIPPED_SUBTREES.contains(&inst.name()) {
            return true;
        }
        at = copy.parent(r);
    }
    false
}

/// Clones `dom`, instruments `debug`'s scripts, injects the probe, encodes
/// it as a binary place with `crate::save::Format::Binary.encode`, and runs
/// `preflight`. A `TooLarge` finding is an `Err` (its `preflight_text`); the
/// user's DOM is untouched.
pub(crate) fn prepare(
    dom: &WeakDom,
    probe: &ProbeConfig,
    debug: &[DebugRequest],
    record_hz: f32,
) -> Result<Prepared, String> {
    let mut copy = dom.clone();
    let mut warnings = mock_purchases(&mut copy);
    warnings.extend(structured_prints(&mut copy));
    warnings.extend(playtest_marks(&mut copy));
    warnings.extend(simulated_touches(&mut copy));
    let (debug, instrument_warnings) = instrument_scripts(&mut copy, debug);
    warnings.extend(instrument_warnings);
    rbx_play::inject::inject(&mut copy, probe).map_err(|err| match err {
        rbx_play::inject::InjectError::NoDataModel => {
            "The place has no instances, so there is nothing to play.".to_owned()
        }
    })?;
    rbx_play::record::set_rate(&mut copy, record_hz);
    let bytes = crate::save::Format::Binary.encode(&copy)?;
    for finding in rbx_play::inject::preflight(&copy, bytes.len()) {
        match finding {
            Preflight::TooLarge { .. } => return Err(preflight_text(&finding)),
            Preflight::Incomplete { .. } => warnings.push(preflight_text(&finding)),
        }
    }
    Ok(Prepared {
        bytes,
        warnings,
        debug,
    })
}

/// One sentence per finding, e.g. "The place is 162.4 MB; Roblox accepts at
/// most 150 MB through Open Cloud." / "3 UnionOperation instances may publish
/// incomplete through Open Cloud."
pub(crate) fn preflight_text(finding: &Preflight) -> String {
    // Roblox's MB: its docs write 100 MB for 104,857,600 bytes.
    const MB: usize = 1024 * 1024;
    match finding {
        Preflight::TooLarge { bytes } => format!(
            "The place is {:.1} MB; Roblox accepts at most {} MB through Open Cloud.",
            *bytes as f64 / MB as f64,
            rbx_play::inject::MAX_PUBLISH_BYTES / MB
        ),
        Preflight::Incomplete { class, count } => format!(
            "{count} {class} instance{} may publish incomplete through Open Cloud.",
            if *count == 1 { "" } else { "s" }
        ),
    }
}

/// The Output row for a probe message, if it shows one:
/// - `Log`: source `SERVER_SOURCE`/`CLIENT_SOURCE` by side; level error →
///   `Feedback::Error`, warning → `Warning`, output/info → `Output`.
/// - `Hello`: `PLAY_SOURCE` Output "Connected to the server (place version N)."
///   plus, when `http_enabled` or `loadstring_enabled` is false, an `Error`
///   naming which and that the probe cannot work without it (returned as the
///   row instead of the Output one).
/// - `Ack` with `ok == false`: `PLAY_SOURCE` Error with the result; ok acks show nothing.
/// - `Joined`/`Left`/`Stats`/`Bye`: nothing.
pub(crate) fn feedback_for(event: &Uplink) -> Option<(&'static str, Feedback)> {
    match event {
        Uplink::Log {
            side,
            level,
            message,
            ..
        } => {
            let source = match side {
                Side::Server => SERVER_SOURCE,
                Side::Client => CLIENT_SOURCE,
            };
            let text = message.clone();
            let row = match level {
                LogLevel::Error => Feedback::Error(text),
                LogLevel::Warning => Feedback::Warning(text),
                LogLevel::Info => Feedback::Info(text),
                LogLevel::Output => Feedback::Output(text),
            };
            Some((source, row))
        }
        Uplink::Hello {
            place_version,
            http_enabled,
            loadstring_enabled,
            ..
        } => {
            let missing = match (*http_enabled, *loadstring_enabled) {
                (true, true) => {
                    return Some((
                        PLAY_SOURCE,
                        Feedback::Output(format!(
                            "Connected to the server (place version {place_version})."
                        )),
                    ));
                }
                (false, true) => "HTTP requests are",
                (true, false) => "loadstring is",
                (false, false) => "HTTP requests and loadstring are",
            };
            Some((
                PLAY_SOURCE,
                Feedback::Error(format!(
                    "{missing} disabled in the game, and the probe cannot work without it."
                )),
            ))
        }
        Uplink::Ack {
            ok: false, result, ..
        } => Some((PLAY_SOURCE, Feedback::Error(result.clone()))),
        _ => None,
    }
}

/// Why a session ended; shown in Output as `PLAY_SOURCE` Output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StopReason {
    /// Stop pressed.
    Stopped,
    /// The client process exited (Sober closed).
    ClientExited,
    /// The embedded window disappeared.
    #[cfg_attr(not(any(target_os = "linux", windows)), allow(dead_code))]
    WindowClosed,
    /// The developer's own player left the game.
    PlayerLeft,
    /// The server shut down.
    ServerClosed,
    /// Starting failed at some step; the text says which and why.
    Failed(String),
}

impl StopReason {
    /// e.g. "Play stopped.", "Play ended: Roblox was closed.", "Play ended:
    /// you left the game.", "Play ended: the server shut down.",
    /// "Play failed: <text>".
    pub(crate) fn message(&self) -> String {
        match self {
            StopReason::Stopped => "Play stopped.".to_owned(),
            StopReason::ClientExited => "Play ended: Roblox was closed.".to_owned(),
            StopReason::WindowClosed => "Play ended: the Roblox window was closed.".to_owned(),
            StopReason::PlayerLeft => "Play ended: you left the game.".to_owned(),
            StopReason::ServerClosed => "Play ended: the server shut down.".to_owned(),
            StopReason::Failed(text) => format!("Play failed: {text}"),
        }
    }
}

/// What a player event means for the session (`follow`).
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Follow {
    Stop(StopReason),
    /// The developer's player is teleporting out of its server: the Play
    /// waits for it to turn up in the next one.
    Leaving,
    /// It is in another server now, and the one it left has gone quiet.
    Arrived,
}

/// Where the developer's player is, as the probes of every server it
/// passes through report it.
#[derive(Debug, Default)]
pub(crate) struct Presence {
    /// The first `Joined` after launch.
    own_user: Option<u64>,
    /// Its joins minus its leaves: 2 while a teleport's new server sees it
    /// before the old one reports it gone.
    servers: u32,
    /// A teleport `Left` came and no `Joined` yet.
    leaving: bool,
    /// The newest `Hello`'s job: the server the player is in or heading to.
    job_id: String,
}

/// Follows the developer's player across servers: a `Left` of it ends the
/// Play (`PlayerLeft`) unless it is still in another server (`Arrived`) or
/// left by teleport (`Leaving`, until its next `Joined` → `Arrived`). A
/// `Left` before any `Joined` counts as its own (the probe may miss the
/// join). `Bye` → `ServerClosed`, except from a server it already left.
pub(crate) fn follow(event: &Uplink, presence: &mut Presence) -> Option<Follow> {
    match event {
        Uplink::Hello { job_id, .. } => {
            presence.job_id.clone_from(job_id);
            None
        }
        Uplink::Joined { user_id } => {
            if *presence.own_user.get_or_insert(*user_id) != *user_id {
                return None;
            }
            presence.servers += 1;
            std::mem::take(&mut presence.leaving).then_some(Follow::Arrived)
        }
        Uplink::Left { user_id, teleport } => {
            if presence.own_user.is_some_and(|own| own != *user_id) {
                return None;
            }
            presence.servers = presence.servers.saturating_sub(1);
            Some(if presence.servers > 0 {
                Follow::Arrived
            } else if *teleport {
                presence.leaving = true;
                Follow::Leaving
            } else {
                Follow::Stop(StopReason::PlayerLeft)
            })
        }
        Uplink::Bye { job_id } => {
            let current = job_id.is_empty() || *job_id == presence.job_id;
            (current && !presence.leaving).then_some(Follow::Stop(StopReason::ServerClosed))
        }
        _ => None,
    }
}

/// `rbx_play::embed::Rect` exists on Linux and Windows only; elsewhere an identical
/// local struct stands in so this module builds everywhere.
pub(crate) mod rbx_play_rect {
    #[cfg(any(target_os = "linux", windows))]
    pub(crate) use rbx_play::embed::Rect;

    #[cfg(not(any(target_os = "linux", windows)))]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct Rect {
        pub x: i32,
        pub y: i32,
        pub width: u32,
        pub height: u32,
    }
}

/// The Test ribbon's one-line summary of a side's `Metrics`, the most
/// telling numbers first; parts the side did not report are left out.
/// Client: "60 fps · 45 ms ping · 812 MB"; server: "60 Hz · 1 player · 412 MB".
pub(crate) fn stats_line(side: Side, metrics: &rbx_play::protocol::Metrics) -> String {
    let mut parts = Vec::new();
    if let Some(fps) = metrics.fps {
        let unit = match side {
            Side::Client => "fps",
            Side::Server => "Hz",
        };
        parts.push(format!("{} {unit}", fps.round()));
    }
    if let Some(ping) = metrics.ping_ms {
        parts.push(format!("{} ms ping", ping.round()));
    }
    if let Some(players) = metrics.players {
        let plural = if players == 1 { "" } else { "s" };
        parts.push(format!("{players} player{plural}"));
    }
    if let Some(memory) = metrics.memory_mb {
        parts.push(format!("{} MB", memory.round()));
    }
    if parts.is_empty() {
        "\u{2014}".to_owned()
    } else {
        parts.join(" \u{b7} ")
    }
}

/// Every reported number of a side, one per line, for the readout's tooltip.
pub(crate) fn stats_details(metrics: &rbx_play::protocol::Metrics) -> String {
    let mut lines = Vec::new();
    let mut push = |label: &str, value: Option<f64>, unit: &str| {
        if let Some(value) = value {
            lines.push(format!("{label}: {value:.1}{unit}"));
        }
    };
    push("Frame rate", metrics.fps, "/s");
    push("Physics rate", metrics.physics_fps, "/s");
    push("Frame time", metrics.frame_ms, " ms");
    push("Ping", metrics.ping_ms, " ms");
    push("Memory", metrics.memory_mb, " MB");
    push("Sent", metrics.send_kbps, " KB/s");
    push("Received", metrics.receive_kbps, " KB/s");
    if let Some(instances) = metrics.instances {
        lines.push(format!("Instances: {instances}"));
    }
    if let Some(players) = metrics.players {
        lines.push(format!("Players: {players}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests;
