//! Where each frame's time went, the MicroProfiler's view of a run: Luau,
//! physics, replication between the peers, and the editor's own render
//! work, with every `debug.profilebegin` scope the scripts opened.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub use rbx_lua::profiler::ProfileLabel;

use crate::Peer;

/// Frames a session keeps: ten seconds at 60 Hz.
pub const KEPT_FRAMES: usize = 600;

/// One peer's costs for one step.
#[derive(Clone, Debug, Default)]
pub struct Costs {
    pub script: Duration,
    pub physics: Duration,
    /// The whole step, script and physics included.
    pub total: Duration,
    pub labels: Vec<ProfileLabel>,
}

/// One frame of a session, every peer together.
#[derive(Clone, Debug)]
pub struct FrameProfile {
    pub frame: u64,
    pub started: Instant,
    pub script: Duration,
    pub physics: Duration,
    /// Moving changes, remotes and parts between the server and clients.
    pub replication: Duration,
    /// The rest of the steps: tweens, animation, sound, GUI layout.
    pub other: Duration,
    /// The editor's work showing the frame, reported after the step.
    pub render: Duration,
    pub labels: Vec<(Peer, ProfileLabel)>,
}

impl FrameProfile {
    pub fn total(&self) -> Duration {
        self.script + self.physics + self.replication + self.other + self.render
    }
}

/// The last [`KEPT_FRAMES`] frames, oldest first.
#[derive(Default)]
pub struct FrameLog {
    frames: VecDeque<FrameProfile>,
}

impl FrameLog {
    pub fn push(&mut self, frame: FrameProfile) {
        if self.frames.len() == KEPT_FRAMES {
            self.frames.pop_front();
        }
        self.frames.push_back(frame);
    }

    pub fn frames(&self) -> &VecDeque<FrameProfile> {
        &self.frames
    }

    pub fn last_mut(&mut self) -> Option<&mut FrameProfile> {
        self.frames.back_mut()
    }
}

/// Builds a session frame from its peers' costs and the step's wall time.
pub(crate) fn frame(
    number: u64,
    started: Instant,
    wall: Duration,
    peers: Vec<(Peer, Costs)>,
) -> FrameProfile {
    let mut out = FrameProfile {
        frame: number,
        started,
        script: Duration::ZERO,
        physics: Duration::ZERO,
        replication: Duration::ZERO,
        other: Duration::ZERO,
        render: Duration::ZERO,
        labels: Vec::new(),
    };
    let mut stepped = Duration::ZERO;
    for (peer, costs) in peers {
        out.script += costs.script;
        out.physics += costs.physics;
        out.other += costs.total.saturating_sub(costs.script + costs.physics);
        stepped += costs.total;
        out.labels
            .extend(costs.labels.into_iter().map(|label| (peer, label)));
    }
    out.replication = wall.saturating_sub(stepped);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_splits_its_wall_time() {
        let ms = Duration::from_millis;
        let server = Costs {
            script: ms(3),
            physics: ms(2),
            total: ms(6),
            labels: Vec::new(),
        };
        let client = Costs {
            script: ms(1),
            physics: ms(0),
            total: ms(2),
            labels: Vec::new(),
        };
        let f = frame(
            1,
            Instant::now(),
            ms(10),
            vec![(Peer::Server, server), (Peer::Client(0), client)],
        );
        assert_eq!(
            (f.script, f.physics, f.other, f.replication),
            (ms(4), ms(2), ms(2), ms(2))
        );
        assert_eq!(f.total(), ms(10));
    }

    #[test]
    fn the_log_keeps_the_last_frames() {
        let mut log = FrameLog::default();
        for n in 0..(KEPT_FRAMES as u64 + 5) {
            log.push(frame(n, Instant::now(), Duration::ZERO, Vec::new()));
        }
        assert_eq!(log.frames().len(), KEPT_FRAMES);
        assert_eq!(log.frames()[0].frame, 5);
    }
}
