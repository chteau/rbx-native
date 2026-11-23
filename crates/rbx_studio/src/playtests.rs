//! Recorded playtests: what a Play session's players did, kept in the
//! editor's own data folder (never in the place) and replayed over the
//! edit-mode 3D view.
//!
//! - `recorder`: turns a Play session's `Uplink`s into a [`Recording`].
//! - `format`: the compact binary file a recording is saved as.
//! - `store`: one folder per place; list, save, rename, delete.
//! - `heat`: heat maps across recordings, time spent, deaths, stuck spots.
//! - `replay`: a recording at a moment, as lines and dots for the viewport.
//! - `repro`: the `.rbxrepro` file a recording is shared as, with its place.
//! - `report`: the Markdown bug report copied from a recording.

pub(crate) mod format;
pub(crate) mod heat;
pub(crate) mod recorder;
pub(crate) mod replay;
pub(crate) mod report;
pub(crate) mod repro;
pub(crate) mod store;
#[cfg(test)]
mod tests;

use glam::Vec3;

/// `m:ss`.
pub(crate) fn clock(seconds: f32) -> String {
    let whole = seconds.max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// The HumanoidRootPart's height above the floor it stands on, in studs
/// (R15's default `HipHeight` 2 plus half the 2-stud root part): where a
/// heat cell is laid under the samples that fill it.
pub(crate) const ROOT_HEIGHT: f32 = 3.0;

/// One recorded playtest. Times are seconds from the session's start.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Recording {
    /// What the Playtests dock calls it; the user can rename it.
    pub(crate) name: String,
    /// When the session started, in Unix seconds.
    pub(crate) started: u64,
    /// Samples per second the probe was asked for.
    pub(crate) rate_hz: f32,
    /// From session start to session stop.
    pub(crate) duration: f32,
    pub(crate) players: Vec<Player>,
    /// One per player that has samples, each sorted by time.
    pub(crate) tracks: Vec<Track>,
    /// Sorted by time.
    pub(crate) events: Vec<Event>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Player {
    pub(crate) user: u64,
    pub(crate) name: String,
}

/// One player's character over the session.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Track {
    pub(crate) user: u64,
    pub(crate) samples: Vec<Sample>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Sample {
    pub(crate) t: f32,
    /// The root part's position.
    pub(crate) pos: Vec3,
    /// Radians about +Y (see `rbx_play::protocol::RecSample::yaw`).
    pub(crate) yaw: f32,
    /// Camera position and unit look direction, when the client sent one.
    pub(crate) camera: Option<(Vec3, Vec3)>,
}

/// What an [`Event`] is. The `u8` is its tag in the file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub(crate) enum EventKind {
    SessionStart = 0,
    SessionStop = 1,
    Join = 2,
    Leave = 3,
    Death = 4,
    Respawn = 5,
    Damage = 6,
    Chat = 7,
    /// A game script's `shared.rbxnative.mark(name, data)`.
    Mark = 8,
    /// A warning a game script printed.
    Warning = 9,
    /// An error a game script raised.
    Error = 10,
}

impl EventKind {
    pub(crate) const ALL: [EventKind; 11] = [
        EventKind::SessionStart,
        EventKind::SessionStop,
        EventKind::Join,
        EventKind::Leave,
        EventKind::Death,
        EventKind::Respawn,
        EventKind::Damage,
        EventKind::Chat,
        EventKind::Mark,
        EventKind::Warning,
        EventKind::Error,
    ];

    pub(crate) fn from_tag(tag: u8) -> Option<EventKind> {
        EventKind::ALL.into_iter().find(|kind| *kind as u8 == tag)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Event {
    pub(crate) t: f32,
    /// The player it happened to; 0 for nobody's.
    pub(crate) user: u64,
    pub(crate) kind: EventKind,
    /// Where, when known.
    pub(crate) pos: Option<Vec3>,
    /// Damage: health lost; 0 otherwise.
    pub(crate) amount: f32,
    /// Join: the name; Chat: the message; Mark: the mark's name;
    /// Warning/Error: the text.
    pub(crate) label: String,
    /// Mark: its data as JSON text; empty otherwise.
    pub(crate) data: String,
}

/// What the Playtests dock lists for a recording.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Summary {
    pub(crate) name: String,
    pub(crate) started: u64,
    pub(crate) duration: f32,
    pub(crate) players: usize,
    pub(crate) deaths: usize,
}

impl Recording {
    /// Players are counted from `players`, deaths from `events`.
    pub(crate) fn summary(&self) -> Summary {
        Summary {
            name: self.name.clone(),
            started: self.started,
            duration: self.duration,
            players: self.players.len(),
            deaths: self
                .events
                .iter()
                .filter(|event| event.kind == EventKind::Death)
                .count(),
        }
    }
}
