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

/// Clones `dom`, instruments `debug`'s scripts, injects the probe, encodes
/// it as a binary place with `crate::save::Format::Binary.encode`, and runs
/// `preflight`. A `TooLarge` finding is an `Err` (its `preflight_text`); the
/// user's DOM is untouched.
pub(crate) fn prepare(
    dom: &WeakDom,
    probe: &ProbeConfig,
    debug: &[DebugRequest],
) -> Result<Prepared, String> {
    let mut copy = dom.clone();
    let (debug, mut warnings) = instrument_scripts(&mut copy, debug);
    rbx_play::inject::inject(&mut copy, probe).map_err(|err| match err {
        rbx_play::inject::InjectError::NoDataModel => {
            "The place has no instances, so there is nothing to play.".to_owned()
        }
    })?;
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

/// One sentence per finding, e.g. "The place is 12.4 MB; Roblox accepts at
/// most 10 MB through Open Cloud." / "3 UnionOperation instances may publish
/// incomplete through Open Cloud."
pub(crate) fn preflight_text(finding: &Preflight) -> String {
    match finding {
        Preflight::TooLarge { bytes } => format!(
            "The place is {:.1} MB; Roblox accepts at most {:.1} MB through Open Cloud.",
            *bytes as f64 / 1_000_000.0,
            rbx_play::inject::MAX_PUBLISH_BYTES as f64 / 1_000_000.0
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

/// Follows the session's players: the first `Joined` after launch is the
/// developer's own (stored in `own_user`); a `Left` of that user →
/// `Some(PlayerLeft)`; `Bye` → `Some(ServerClosed)`; else `None`. A `Left`
/// before any `Joined` stops too (the probe may miss the join).
pub(crate) fn stop_for(event: &Uplink, own_user: &mut Option<u64>) -> Option<StopReason> {
    match event {
        Uplink::Joined { user_id } => {
            own_user.get_or_insert(*user_id);
            None
        }
        Uplink::Left { user_id } => match *own_user {
            Some(own) if own != *user_id => None,
            _ => Some(StopReason::PlayerLeft),
        },
        Uplink::Bye => Some(StopReason::ServerClosed),
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
