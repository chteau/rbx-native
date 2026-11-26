//! The conformance suite: Luau tests that print a JSON result, run the
//! same way in real Roblox (goldens, recorded by the `record_goldens`
//! example) and in [`Simulation`] (the `conformance` test and example).
//!
//! The tests live in `conformance/tests/` and the goldens in
//! `conformance/goldens/`, one JSON file per test.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use serde_json::Value;

use crate::{Peer, Session};

pub mod streaming;

/// What a test's printed line starts with, and the runner's last line.
pub const PREFIX: &str = "CONFORMANCE ";
pub const DONE: &str = "CONFORMANCE_DONE";
/// The folder in `ServerScriptService` the tests are modules of.
pub const FOLDER: &str = "Conformance";
pub const RUNNER: &str = "ConformanceRunner";

/// Seconds the runner waits before the first test, so a real server is
/// past its start-up hitches.
const SETTLE: f64 = 3.0;

/// Passes over the suite in one run. Roblox isn't deterministic (the
/// first pass on a fresh server also starts from a different point in the
/// frame), so goldens keep the median and the spread, and rbx_sim runs the
/// same passes to compare like for like.
pub const PASSES: u32 = 5;

pub struct Test {
    pub name: &'static str,
    pub source: &'static str,
    /// Absolute tolerance per result field, keyed by a glob where `*`
    /// matches any run of characters (`"name"`, `"name*"`, `"*name"`,
    /// `"*name*"`, `"*"`). The most specific match wins: an exact name,
    /// then the pattern with the most literal characters, then the first
    /// listed.
    pub tolerance: &'static [(&'static str, f64)],
}

/// Whether `field` matches `pattern`, `*` standing for any run of characters.
fn glob_matches(pattern: &str, field: &str) -> bool {
    let mut segments = pattern.split('*');
    let first = segments.next().unwrap_or_default();
    let Some(mut rest) = field.strip_prefix(first) else {
        return false;
    };
    let mut segments: Vec<&str> = segments.collect();
    let Some(last) = segments.pop() else {
        return rest.is_empty();
    };
    for segment in segments {
        match rest.find(segment) {
            Some(at) => rest = &rest[at + segment.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

impl Test {
    pub fn tolerance_of(&self, field: &str) -> f64 {
        self.tolerance
            .iter()
            .enumerate()
            .filter(|(_, (k, _))| glob_matches(k, field))
            .max_by_key(|(i, (k, _))| {
                let literal = k.chars().filter(|&c| c != '*').count();
                (!k.contains('*'), literal, std::cmp::Reverse(*i))
            })
            .map_or(0.0, |(_, (_, t))| *t)
    }
}

const HARNESS: &str = include_str!("../conformance/tests/Harness.luau");

pub const TESTS: &[Test] = &[
    Test {
        name: "FreeFall",
        source: include_str!("../conformance/tests/FreeFall.luau"),
        tolerance: &[("*", 0.5)],
    },
    Test {
        name: "RestingContact",
        source: include_str!("../conformance/tests/RestingContact.luau"),
        tolerance: &[("*", 0.02)],
    },
    Test {
        name: "FrictionSlide",
        source: include_str!("../conformance/tests/FrictionSlide.luau"),
        tolerance: &[("*", 0.5), ("stopped_beat", 2.0)],
    },
    Test {
        name: "ElasticBounce",
        source: include_str!("../conformance/tests/ElasticBounce.luau"),
        tolerance: &[("*", 0.5), ("first_bounce_beat", 1.0)],
    },
    Test {
        name: "TouchedOrder",
        source: include_str!("../conformance/tests/TouchedOrder.luau"),
        tolerance: &[],
    },
    Test {
        name: "TaskTiming",
        source: include_str!("../conformance/tests/TaskTiming.luau"),
        tolerance: &[
            ("*", 1.0),
            ("wait0", 0.01),
            ("wait01", 0.02),
            ("wait05", 0.02),
            ("legacy_wait", 0.02),
        ],
    },
    Test {
        name: "RunServiceOrder",
        source: include_str!("../conformance/tests/RunServiceOrder.luau"),
        tolerance: &[],
    },
    Test {
        name: "SignalBehavior",
        source: include_str!("../conformance/tests/SignalBehavior.luau"),
        tolerance: &[],
    },
    Test {
        name: "FreeFallTrace",
        source: include_str!("../conformance/tests/FreeFallTrace.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "WaitRounding",
        source: include_str!("../conformance/tests/WaitRounding.luau"),
        // Beat counts are integers, so exact; the elapsed times differ in
        // the 4th decimal: the server's frames sum to 2.01658 s over 121
        // beats (`_dt_task2`), a hair under 1/60 s each.
        tolerance: &[("*", 0.0002)],
    },
    Test {
        name: "Raycast",
        source: include_str!("../conformance/tests/Raycast.luau"),
        tolerance: &[],
    },
    Test {
        name: "Shapecast",
        source: include_str!("../conformance/tests/Shapecast.luau"),
        tolerance: &[],
    },
    Test {
        name: "Overlap",
        source: include_str!("../conformance/tests/Overlap.luau"),
        tolerance: &[],
    },
    Test {
        name: "HingeFree",
        source: include_str!("../conformance/tests/HingeFree.luau"),
        tolerance: &[("*", 0.1), ("angle_30", 2.0)],
    },
    Test {
        name: "HingeMotor",
        source: include_str!("../conformance/tests/HingeMotor.luau"),
        tolerance: &[("*", 0.05), ("angle_10", 2.0), ("angle_30", 2.0)],
    },
    Test {
        name: "HingeServo",
        source: include_str!("../conformance/tests/HingeServo.luau"),
        tolerance: &[("*", 2.0)],
    },
    Test {
        name: "BallSocket",
        source: include_str!("../conformance/tests/BallSocket.luau"),
        tolerance: &[("*", 0.1)],
    },
    Test {
        name: "Rope",
        source: include_str!("../conformance/tests/Rope.luau"),
        tolerance: &[("*", 0.1)],
    },
    Test {
        name: "Spring",
        source: include_str!("../conformance/tests/Spring.luau"),
        tolerance: &[("*", 0.1)],
    },
    Test {
        name: "Prismatic",
        source: include_str!("../conformance/tests/Prismatic.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "AlignPosition",
        source: include_str!("../conformance/tests/AlignPosition.luau"),
        tolerance: &[("*", 0.1)],
    },
    Test {
        name: "AlignOrientation",
        source: include_str!("../conformance/tests/AlignOrientation.luau"),
        tolerance: &[("*", 2.0)],
    },
    Test {
        name: "LinearVelocity",
        source: include_str!("../conformance/tests/LinearVelocity.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "AngularVelocity",
        source: include_str!("../conformance/tests/AngularVelocity.luau"),
        tolerance: &[("*", 0.05), ("dy_30", 0.5)],
    },
    Test {
        name: "VectorForce",
        source: include_str!("../conformance/tests/VectorForce.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "Attachments",
        source: include_str!("../conformance/tests/Attachments.luau"),
        tolerance: &[],
    },
    Test {
        name: "BoxBounce",
        source: include_str!("../conformance/tests/BoxBounce.luau"),
        tolerance: &[("*", 0.3), ("first_bounce_beat", 1.0), ("bounces", 1.0)],
    },
    Test {
        name: "SleepWake",
        source: include_str!("../conformance/tests/SleepWake.luau"),
        tolerance: &[("*", 0.1), ("still_from", 10.0)],
    },
    Test {
        name: "SleepPile",
        source: include_str!("../conformance/tests/SleepPile.luau"),
        tolerance: &[("*", 0.1), ("*still_from*", 20.0)],
    },
    Test {
        name: "BoxStack",
        source: include_str!("../conformance/tests/BoxStack.luau"),
        tolerance: &[("*", 0.02)],
    },
    Test {
        name: "Damping",
        source: include_str!("../conformance/tests/Damping.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "RampSlide",
        source: include_str!("../conformance/tests/RampSlide.luau"),
        tolerance: &[("*", 0.2)],
    },
    Test {
        name: "MeshCollision",
        source: include_str!("../conformance/tests/MeshCollision.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "FreeFallRates",
        source: include_str!("../conformance/tests/FreeFallRates.luau"),
        tolerance: &[("*", 1.0)],
    },
    Test {
        name: "HumanoidDefaults",
        source: include_str!("../conformance/tests/HumanoidDefaults.luau"),
        tolerance: &[("*", 0.01)],
    },
    Test {
        name: "HumanoidStand",
        source: include_str!("../conformance/tests/HumanoidStand.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "HumanoidDrop",
        source: include_str!("../conformance/tests/HumanoidDrop.luau"),
        tolerance: &[("*", 0.2), ("r15_land_beat", 1.0), ("r6_land_beat", 1.0)],
    },
    Test {
        name: "HumanoidWalk",
        source: include_str!("../conformance/tests/HumanoidWalk.luau"),
        tolerance: &[("*", 0.3)],
    },
    Test {
        name: "HumanoidTurn",
        source: include_str!("../conformance/tests/HumanoidTurn.luau"),
        tolerance: &[("*", 3.0), ("turned_beat", 1.0), ("back_beat", 1.0)],
    },
    Test {
        name: "HumanoidMoveTo",
        source: include_str!("../conformance/tests/HumanoidMoveTo.luau"),
        tolerance: &[("*", 0.3), ("arrive", 2.0), ("timeout", 2.0)],
    },
    Test {
        name: "HumanoidJump",
        source: include_str!("../conformance/tests/HumanoidJump.luau"),
        tolerance: &[
            ("*", 0.2),
            ("power_apex_beat", 1.0),
            ("height_apex_beat", 1.0),
            ("high_apex_beat", 1.0),
            ("r6_apex_beat", 1.0),
            ("walk_apex_beat", 1.0),
            ("power_land", 1.0),
            ("height_land", 1.0),
            ("high_land", 1.0),
            ("r6_land", 1.0),
            ("walk_land", 1.0),
        ],
    },
    Test {
        name: "HumanoidStep",
        source: include_str!("../conformance/tests/HumanoidStep.luau"),
        tolerance: &[("*", 0.3)],
    },
    Test {
        name: "HumanoidSlope",
        source: include_str!("../conformance/tests/HumanoidSlope.luau"),
        tolerance: &[("*", 0.5)],
    },
    Test {
        name: "HumanoidClimb",
        source: include_str!("../conformance/tests/HumanoidClimb.luau"),
        tolerance: &[("*", 0.3)],
    },
    Test {
        name: "HumanoidSeat",
        source: include_str!("../conformance/tests/HumanoidSeat.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "HumanoidHealth",
        source: include_str!("../conformance/tests/HumanoidHealth.luau"),
        tolerance: &[("*", 0.01)],
    },
    Test {
        name: "HumanoidStates",
        source: include_str!("../conformance/tests/HumanoidStates.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "PlayersCharacter",
        source: include_str!("../conformance/tests/PlayersCharacter.luau"),
        tolerance: &[("*", 0.2)],
    },
    Test {
        name: "AnimationTrack",
        source: include_str!("../conformance/tests/AnimationTrack.luau"),
        tolerance: &[
            ("*", 0.02),
            ("*hip*", 0.3),
            ("*knee*", 0.3),
            ("*waist*", 0.3),
        ],
    },
    Test {
        name: "HumanoidSensor",
        source: include_str!("../conformance/tests/HumanoidSensor.luau"),
        tolerance: &[("*", 0.2)],
    },
    Test {
        name: "HumanoidJumpScale",
        source: include_str!("../conformance/tests/HumanoidJumpScale.luau"),
        tolerance: &[("*", 0.2)],
    },
    Test {
        name: "HumanoidWalkScale",
        source: include_str!("../conformance/tests/HumanoidWalkScale.luau"),
        tolerance: &[("*", 0.3), ("*yaw*", 3.0)],
    },
    Test {
        name: "RemoteArgs",
        source: include_str!("../conformance/tests/RemoteArgs.luau"),
        tolerance: &[],
    },
    Test {
        name: "RemoteOrder",
        source: include_str!("../conformance/tests/RemoteOrder.luau"),
        tolerance: &[],
    },
    Test {
        name: "RemotePayload",
        source: include_str!("../conformance/tests/RemotePayload.luau"),
        tolerance: &[],
    },
    Test {
        name: "ReplicationOrder",
        source: include_str!("../conformance/tests/ReplicationOrder.luau"),
        tolerance: &[],
    },
    Test {
        name: "Ownership",
        source: include_str!("../conformance/tests/Ownership.luau"),
        tolerance: &[],
    },
    Test {
        name: "CameraInput",
        source: include_str!("../conformance/tests/CameraInput.luau"),
        tolerance: &[("*", 0.15), ("start_pitch", 0.5), ("turn_per_100px", 1.0)],
    },
    Test {
        name: "GuiRuntime",
        source: include_str!("../conformance/tests/GuiRuntime.luau"),
        tolerance: &[],
    },
    Test {
        name: "GuiLayout",
        source: include_str!("../conformance/tests/GuiLayout.luau"),
        tolerance: &[("*", 0.6)],
    },
    Test {
        name: "CoreGuiApi",
        source: include_str!("../conformance/tests/CoreGuiApi.luau"),
        tolerance: &[],
    },
    Test {
        name: "ToolEquip",
        source: include_str!("../conformance/tests/ToolEquip.luau"),
        tolerance: &[],
    },
    Test {
        name: "Teams",
        source: include_str!("../conformance/tests/Teams.luau"),
        tolerance: &[],
    },
    Test {
        name: "ClientReplication",
        source: include_str!("../conformance/tests/ClientReplication.luau"),
        tolerance: &[("*speed*", 1.0)],
    },
    Test {
        name: "ReparentOrder",
        source: include_str!("../conformance/tests/ReparentOrder.luau"),
        tolerance: &[],
    },
    Test {
        name: "TweenCurves",
        source: include_str!("../conformance/tests/TweenCurves.luau"),
        tolerance: &[("*_*", 0.00015)],
    },
    Test {
        name: "TweenRuntime",
        source: include_str!("../conformance/tests/TweenRuntime.luau"),
        tolerance: &[
            ("*type_*", 0.02),
            ("*_frames*", 2.0),
            ("*_at*", 0.03),
            ("halfway", 0.03),
            ("after_3", 0.5),
            ("replay_after_cancel", 0.03),
            ("newer_progress", 0.03),
        ],
    },
    Test {
        name: "SoundPlayback",
        source: include_str!("../conformance/tests/SoundPlayback.luau"),
        tolerance: &[
            ("*_frames*", 3.0),
            ("*position*", 0.05),
            ("*speed2*", 0.05),
            ("client_ended_position", 0.02),
        ],
    },
    Test {
        name: "TextChat",
        source: include_str!("../conformance/tests/TextChat.luau"),
        tolerance: &[],
    },
    Test {
        name: "ToolPose",
        source: include_str!("../conformance/tests/ToolPose.luau"),
        tolerance: &[("*shoulder*", 3.0)],
    },
    Test {
        name: "SurfaceMix",
        source: include_str!("../conformance/tests/SurfaceMix.luau"),
        tolerance: &[("*", 0.3), ("*_apex", 0.2)],
    },
    Test {
        name: "DensityMass",
        source: include_str!("../conformance/tests/DensityMass.luau"),
        tolerance: &[("*", 0.3), ("*mass*", 0.001)],
    },
    Test {
        name: "Conveyor",
        source: include_str!("../conformance/tests/Conveyor.luau"),
        tolerance: &[("*", 0.3)],
    },
    Test {
        name: "HumanoidPlatform",
        source: include_str!("../conformance/tests/HumanoidPlatform.luau"),
        tolerance: &[("*", 0.3)],
    },
    Test {
        name: "HumanoidPosture",
        source: include_str!("../conformance/tests/HumanoidPosture.luau"),
        tolerance: &[("*", 0.05), ("*_up", 0.15)],
    },
    Test {
        name: "ContactSink",
        source: include_str!("../conformance/tests/ContactSink.luau"),
        tolerance: &[("*", 0.01)],
    },
    Test {
        name: "HumanoidTurnRates",
        source: include_str!("../conformance/tests/HumanoidTurnRates.luau"),
        tolerance: &[("*", 2.0)],
    },
    Test {
        name: "TerrainFill",
        source: include_str!("../conformance/tests/TerrainFill.luau"),
        tolerance: &[("*", 0.02)],
    },
    Test {
        name: "TerrainRaycast",
        source: include_str!("../conformance/tests/TerrainRaycast.luau"),
        tolerance: &[],
    },
    Test {
        name: "TerrainPhysics",
        source: include_str!("../conformance/tests/TerrainPhysics.luau"),
        tolerance: &[("*", 0.3), ("*_stopped", 3.0), ("*bounce", 0.5)],
    },
    Test {
        name: "TerrainHumanoid",
        source: include_str!("../conformance/tests/TerrainHumanoid.luau"),
        tolerance: &[("*", 0.5)],
    },
    Test {
        name: "Buoyancy",
        source: include_str!("../conformance/tests/Buoyancy.luau"),
        tolerance: &[("*", 0.3), ("*_settled", 20.0)],
    },
    Test {
        name: "TerrainProfile",
        source: include_str!("../conformance/tests/TerrainProfile.luau"),
        tolerance: &[],
    },
    Test {
        name: "Swim",
        source: include_str!("../conformance/tests/Swim.luau"),
        tolerance: &[("*", 0.5)],
    },
    Test {
        name: "DataStoreLimits",
        source: include_str!("../conformance/tests/DataStoreLimits.luau"),
        tolerance: &[],
    },
    Test {
        name: "DataStoreValues",
        source: include_str!("../conformance/tests/DataStoreValues.luau"),
        tolerance: &[],
    },
    Test {
        name: "MemoryStore",
        source: include_str!("../conformance/tests/MemoryStore.luau"),
        tolerance: &[],
    },
    Test {
        name: "HttpJson",
        source: include_str!("../conformance/tests/HttpJson.luau"),
        tolerance: &[],
    },
    Test {
        name: "Messaging",
        source: include_str!("../conformance/tests/Messaging.luau"),
        tolerance: &[],
    },
    Test {
        name: "CollectionTags",
        source: include_str!("../conformance/tests/CollectionTags.luau"),
        tolerance: &[],
    },
    Test {
        name: "Marketplace",
        source: include_str!("../conformance/tests/Marketplace.luau"),
        tolerance: &[],
    },
    Test {
        name: "Pathfinding",
        source: include_str!("../conformance/tests/Pathfinding.luau"),
        tolerance: &[("*", 2.0), ("*_length*", 6.0), ("*_count*", 3.0)],
    },
    Test {
        name: "TeleportApi",
        source: include_str!("../conformance/tests/TeleportApi.luau"),
        tolerance: &[],
    },
    Test {
        name: "BadgeApi",
        source: include_str!("../conformance/tests/BadgeApi.luau"),
        tolerance: &[],
    },
    Test {
        name: "TextFilter",
        source: include_str!("../conformance/tests/TextFilter.luau"),
        tolerance: &[],
    },
    Test {
        name: "Localization",
        source: include_str!("../conformance/tests/Localization.luau"),
        tolerance: &[],
    },
    Test {
        name: "SocialPolicy",
        source: include_str!("../conformance/tests/SocialPolicy.luau"),
        tolerance: &[],
    },
    Test {
        name: "WebMarket",
        source: include_str!("../conformance/tests/WebMarket.luau"),
        tolerance: &[],
    },
    Test {
        name: "WebServices",
        source: include_str!("../conformance/tests/WebServices.luau"),
        tolerance: &[],
    },
    Test {
        name: "ClientApis",
        source: include_str!("../conformance/tests/ClientApis.luau"),
        tolerance: &[],
    },
    Test {
        name: "AudioApi",
        source: include_str!("../conformance/tests/AudioApi.luau"),
        tolerance: &[
            ("*gain_*", 0.05),
            ("audibility", 0.01),
            ("listener_audibility", 0.01),
        ],
    },
    Test {
        name: "EditableApi",
        source: include_str!("../conformance/tests/EditableApi.luau"),
        tolerance: &[],
    },
    Test {
        name: "EditableMeshTopology",
        source: include_str!("../conformance/tests/EditableMeshTopology.luau"),
        tolerance: &[],
    },
    Test {
        name: "EditableMeshQuery",
        source: include_str!("../conformance/tests/EditableMeshQuery.luau"),
        tolerance: &[],
    },
    Test {
        name: "EditableMeshBatch",
        source: include_str!("../conformance/tests/EditableMeshBatch.luau"),
        tolerance: &[],
    },
    Test {
        name: "EditableMeshBones",
        source: include_str!("../conformance/tests/EditableMeshBones.luau"),
        tolerance: &[],
    },
    Test {
        name: "EditableImageDraw",
        source: include_str!("../conformance/tests/EditableImageDraw.luau"),
        tolerance: &[],
    },
    Test {
        name: "EditableAssets",
        source: include_str!("../conformance/tests/EditableAssets.luau"),
        tolerance: &[],
    },
    Test {
        name: "EditableBudget",
        source: include_str!("../conformance/tests/EditableBudget.luau"),
        tolerance: &[],
    },
    Test {
        name: "DragDetectorApi",
        source: include_str!("../conformance/tests/DragDetectorApi.luau"),
        tolerance: &[],
    },
    Test {
        name: "InputActions",
        source: include_str!("../conformance/tests/InputActions.luau"),
        tolerance: &[],
    },
    Test {
        name: "JoinTimeline",
        source: include_str!("../conformance/tests/JoinTimeline.luau"),
        // A live join's timing scatters with the network; what must match
        // is that the server ran first and the client saw what it did.
        tolerance: &[
            ("player_added_s", f64::INFINITY),
            ("server_beats_before_join", f64::INFINITY),
            ("character_after_join_s", f64::INFINITY),
        ],
    },
    Test {
        name: "StyleRuntime",
        source: include_str!("../conformance/tests/StyleRuntime.luau"),
        tolerance: &[],
    },
    Test {
        name: "HumanoidDeath",
        source: include_str!("../conformance/tests/HumanoidDeath.luau"),
        tolerance: &[("*", 0.5), ("*_beat", 1.0), ("*hand_moved*", 1.0)],
    },
    Test {
        name: "VehicleSeat",
        source: include_str!("../conformance/tests/VehicleSeat.luau"),
        tolerance: &[("*", 0.1)],
    },
    Test {
        name: "SwordTouch",
        source: include_str!("../conformance/tests/SwordTouch.luau"),
        tolerance: &[("*", 0.5), ("touch_beats", 1.0)],
    },
    Test {
        name: "CameraLimits",
        source: include_str!("../conformance/tests/CameraLimits.luau"),
        tolerance: &[("*", 0.15)],
    },
    Test {
        name: "WorldApi",
        source: include_str!("../conformance/tests/WorldApi.luau"),
        tolerance: &[],
    },
    Test {
        name: "PartPhysics",
        source: include_str!("../conformance/tests/PartPhysics.luau"),
        tolerance: &[("*", 0.05), ("*group*", 0.3)],
    },
    Test {
        name: "Explosion",
        source: include_str!("../conformance/tests/Explosion.luau"),
        tolerance: &[("*", 0.5)],
    },
    Test {
        name: "Path2DCurve",
        source: include_str!("../conformance/tests/Path2DCurve.luau"),
        // Positions are single-precision UDim2s, printed to nine digits.
        tolerance: &[("*pos_*", 1e-5), ("*arc_*", 1e-5), ("scale_pos", 1e-5)],
    },
    Test {
        name: "NewApis",
        source: include_str!("../conformance/tests/NewApis.luau"),
        tolerance: &[],
    },
    Test {
        name: "ParallelLuau",
        source: include_str!("../conformance/tests/ParallelLuau.luau"),
        tolerance: &[("*fired*", 3.0)],
    },
    Test {
        name: "ModelMoveTo",
        source: include_str!("../conformance/tests/ModelMoveTo.luau"),
        tolerance: &[("*", 0.05)],
    },
    Test {
        name: "AnimationApi",
        source: include_str!("../conformance/tests/AnimationApi.luau"),
        tolerance: &[],
    },
    Test {
        name: "AvatarApi",
        source: include_str!("../conformance/tests/AvatarApi.luau"),
        tolerance: &[],
    },
    Test {
        name: "CsgApi",
        source: include_str!("../conformance/tests/CsgApi.luau"),
        tolerance: &[("*", 0.02)],
    },
    Test {
        name: "ScriptLimits",
        source: include_str!("../conformance/tests/ScriptLimits.luau"),
        tolerance: &[("stack_depth", 4.0), ("index_stack_depth", 4.0)],
    },
    Test {
        name: "ProfilerApi",
        source: include_str!("../conformance/tests/ProfilerApi.luau"),
        tolerance: &[],
    },
    Test {
        name: "DestroyConnections",
        source: include_str!("../conformance/tests/DestroyConnections.luau"),
        tolerance: &[],
    },
    Test {
        name: "FrameOverrun",
        source: include_str!("../conformance/tests/FrameOverrun.luau"),
        tolerance: &[("after_stall_dts", 0.003), ("overrun_dts", 0.01)],
    },
];

/// The folder in `ReplicatedStorage` holding the client halves of the
/// client/server tests, `Shared`, and the `Call`/`Ready` remotes the
/// harness reaches the client through.
pub const CLIENT_FOLDER: &str = "ConformanceClient";

/// Client modules, by name: `Shared` and each test's client half.
const CLIENT_MODULES: &[(&str, &str)] = &[
    ("Shared", include_str!("../conformance/client/Shared.luau")),
    (
        "EditableBudget",
        include_str!("../conformance/client/EditableBudget.luau"),
    ),
    (
        "CameraInput",
        include_str!("../conformance/client/CameraInput.luau"),
    ),
    (
        "RemoteArgs",
        include_str!("../conformance/client/RemoteArgs.luau"),
    ),
    (
        "RemoteOrder",
        include_str!("../conformance/client/RemoteOrder.luau"),
    ),
    (
        "RemotePayload",
        include_str!("../conformance/client/RemotePayload.luau"),
    ),
    (
        "ReplicationOrder",
        include_str!("../conformance/client/ReplicationOrder.luau"),
    ),
    (
        "Ownership",
        include_str!("../conformance/client/Ownership.luau"),
    ),
    (
        "GuiRuntime",
        include_str!("../conformance/client/GuiRuntime.luau"),
    ),
    (
        "GuiLayout",
        include_str!("../conformance/client/GuiLayout.luau"),
    ),
    (
        "CoreGuiApi",
        include_str!("../conformance/client/CoreGuiApi.luau"),
    ),
    (
        "ToolEquip",
        include_str!("../conformance/client/ToolEquip.luau"),
    ),
    (
        "ClientReplication",
        include_str!("../conformance/client/ClientReplication.luau"),
    ),
    (
        "TweenRuntime",
        include_str!("../conformance/client/TweenRuntime.luau"),
    ),
    (
        "SoundPlayback",
        include_str!("../conformance/client/SoundPlayback.luau"),
    ),
    (
        "TextChat",
        include_str!("../conformance/client/TextChat.luau"),
    ),
    (
        "ToolPose",
        include_str!("../conformance/client/ToolPose.luau"),
    ),
    (
        "ClientApis",
        include_str!("../conformance/client/ClientApis.luau"),
    ),
    (
        "InputActions",
        include_str!("../conformance/client/InputActions.luau"),
    ),
    (
        "JoinTimeline",
        include_str!("../conformance/client/JoinTimeline.luau"),
    ),
];
const PARALLEL_PROBE: &str = include_str!("../conformance/client/ParallelProbe.luau");
const CLIENT_RUNNER: &str = include_str!("../conformance/client/Runner.luau");
const FIRST_PROBE: &str = include_str!("../conformance/client/FirstProbe.luau");
const JOIN_CLOCK: &str = include_str!("../conformance/tests/JoinClock.server.luau");

/// The tests named in `only` (all of them when it's empty).
pub fn selected(only: &[String]) -> impl Iterator<Item = (usize, &'static Test)> + '_ {
    TESTS
        .iter()
        .enumerate()
        .filter(move |(_, t)| only.is_empty() || only.iter().any(|n| n == t.name))
}

/// The runner `Script`: each test in `only` (every test when empty) in
/// turn, in a corner of its own that depends only on its place in
/// [`TESTS`], the whole lot `runs` times over.
pub fn runner_source(runs: u32, only: &[String]) -> String {
    let names: Vec<String> = selected(only)
        .map(|(i, t)| format!("{{ {}, \"{}\" }}", i + 1, t.name))
        .collect();
    format!(
        r#"local HttpService = game:GetService("HttpService")
local tests = script.Parent:WaitForChild("{FOLDER}")
local h = require(tests:WaitForChild("Harness"))
task.wait({SETTLE})
for _ = 1, {runs} do
	for _, entry in {{ {names} }} do
		local i, name = entry[1], entry[2]
		-- Every test starts just after a Heartbeat, whatever ran before it.
		h.beat()
		local ok, result = pcall(require(tests:WaitForChild(name)), h, Vector3.new(2000 + i * 300, 300, 2000))
		if not ok then
			result = {{ error = tostring(result) }}
		end
		print("{PREFIX}" .. name .. " " .. HttpService:JSONEncode(result))
	end
end
print("{DONE}")
"#,
        names = names.join(", ")
    )
}

fn child(dom: &mut WeakDom, class: &str, name: &str, parent: Ref) -> Ref {
    dom.new_instance(class, name, Some(parent))
}

fn service(dom: &mut WeakDom, class: &str) -> Ref {
    let found = dom
        .root_refs()
        .iter()
        .copied()
        .find(|r| dom.get(*r).is_some_and(|i| i.class() == class));
    found.unwrap_or_else(|| dom.new_instance(class, class, None))
}

/// Adds the tests and a runner doing `runs` passes to
/// `ServerScriptService` in `dom`.
/// `ServerScriptService` in `dom`, only the tests in `only` when it isn't
/// empty.
pub fn add_to(dom: &mut WeakDom, runs: u32, only: &[String]) {
    let sss = service(dom, "ServerScriptService");
    let folder = child(dom, "Folder", FOLDER, sss);
    let modules = std::iter::once(("Harness", HARNESS))
        .chain(selected(only).map(|(_, t)| (t.name, t.source)));
    for (name, source) in modules {
        let module = child(dom, "ModuleScript", name, folder);
        let _ = dom.set_property(module, "Source", Variant::String(source.into()));
    }
    // ScriptLimits' require cases: a cycle, a module that errors, one that
    // returns nothing.
    for (name, source) in [
        ("LimitsCycleA", "return require(script.Parent.LimitsCycleB)"),
        ("LimitsCycleB", "return require(script.Parent.LimitsCycleA)"),
        ("LimitsBroken", "error(\"broken module\")"),
        ("LimitsEmpty", "local x = 1"),
    ] {
        let module = child(dom, "ModuleScript", name, folder);
        let _ = dom.set_property(module, "Source", Variant::String(source.into()));
    }
    // ParallelLuau: a script inside an Actor, from the start.
    let actor = child(dom, "Actor", "ConfActor", sss);
    let probe = child(dom, "Script", "ParallelProbe", actor);
    let _ = dom.set_property(probe, "Source", Variant::String(PARALLEL_PROBE.into()));
    let clock = child(dom, "Script", "JoinClock", sss);
    let _ = dom.set_property(clock, "Source", Variant::String(JOIN_CLOCK.into()));
    let runner = child(dom, "Script", RUNNER, sss);
    let _ = dom.set_property(runner, "Source", Variant::String(runner_source(runs, only)));
    add_client_half(dom);
}

/// What the client/server tests need on the client: the modules and
/// remotes in `ReplicatedStorage`, the runner in `StarterPlayerScripts`, a
/// probe in `ReplicatedFirst`, and one script per run context in
/// `Workspace` and `ReplicatedStorage` that marks `_G` where it runs.
fn add_client_half(dom: &mut WeakDom) {
    let source = |dom: &mut WeakDom, script: Ref, text: &str| {
        let _ = dom.set_property(script, "Source", Variant::String(text.into()));
    };
    let storage = service(dom, "ReplicatedStorage");
    let folder = child(dom, "Folder", CLIENT_FOLDER, storage);
    for (name, text) in CLIENT_MODULES {
        let module = child(dom, "ModuleScript", name, folder);
        source(dom, module, text);
    }
    // A LocalScript the GuiRuntime test clones into a ScreenGui (a game
    // script can't write `Source`): counts the times it ran.
    let probe = child(dom, "LocalScript", "GuiProbe", folder);
    source(dom, probe, "_G.confGuiRuns = (_G.confGuiRuns or 0) + 1");
    child(dom, "RemoteFunction", "Call", folder);
    child(dom, "RemoteEvent", "Ready", folder);
    let starter_player = service(dom, "StarterPlayer");
    let existing = dom.get(starter_player).and_then(|i| {
        i.children().iter().copied().find(|c| {
            dom.get(*c)
                .is_some_and(|c| c.class() == "StarterPlayerScripts")
        })
    });
    let starter = existing.unwrap_or_else(|| {
        child(
            dom,
            "StarterPlayerScripts",
            "StarterPlayerScripts",
            starter_player,
        )
    });
    let runner = child(dom, "LocalScript", CLIENT_FOLDER, starter);
    source(dom, runner, CLIENT_RUNNER);
    // GuiRuntime: a ScreenGui that isn't reset on spawn, there from the
    // first spawn on.
    let starter_gui = service(dom, "StarterGui");
    let keep = child(dom, "ScreenGui", "ConfKeep", starter_gui);
    let _ = dom.set_property(keep, "ResetOnSpawn", Variant::Bool(false));
    let first = service(dom, "ReplicatedFirst");
    let probe = child(dom, "LocalScript", "FirstProbe", first);
    source(dom, probe, FIRST_PROBE);
    let workspace = service(dom, "Workspace");
    // (where, class, name, RunContext: 0 legacy, 1 server, 2 client)
    for (parent, class, name, context) in [
        (workspace, "Script", "rc_client_ws", 2),
        (workspace, "LocalScript", "local_ws", 0),
        (storage, "Script", "legacy_rs", 0),
        (storage, "Script", "rc_server_rs", 1),
        (storage, "Script", "rc_client_rs", 2),
    ] {
        let script = child(dom, class, name, parent);
        if context != 0 {
            let _ = dom.set_property(script, "RunContext", Variant::Enum(context));
        }
        source(dom, script, &format!("_G.{name} = true"));
    }
}

/// A bare place: the services a fresh place has, a baseplate and a spawn.
/// `record_goldens` publishes it (with the suite) when given `bare`.
pub fn bare_place() -> WeakDom {
    let mut dom = WeakDom::new();
    let workspace = service(&mut dom, "Workspace");
    for class in [
        "Players",
        "Lighting",
        "ReplicatedFirst",
        "ReplicatedStorage",
        "ServerScriptService",
        "ServerStorage",
        "StarterGui",
        "StarterPack",
        "Teams",
        "SoundService",
    ] {
        service(&mut dom, class);
    }
    // A new place's chat: TextChatService with its default channels.
    let chat = service(&mut dom, "TextChatService");
    let _ = dom.set_property(chat, "ChatVersion", Variant::Enum(1));
    let player = service(&mut dom, "StarterPlayer");
    child(
        &mut dom,
        "StarterPlayerScripts",
        "StarterPlayerScripts",
        player,
    );
    child(
        &mut dom,
        "StarterCharacterScripts",
        "StarterCharacterScripts",
        player,
    );
    let at = |y: f32| {
        Variant::CFrame(CFrameData {
            position: Vector3Data { x: 0.0, y, z: 0.0 },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        })
    };
    let size = |x: f32, y: f32, z: f32| Variant::Vector3(Vector3Data { x, y, z });
    child(&mut dom, "Terrain", "Terrain", workspace);
    let base = child(&mut dom, "Part", "Baseplate", workspace);
    let _ = dom.set_property(base, "Anchored", Variant::Bool(true));
    let _ = dom.set_property(base, "Size", size(512.0, 20.0, 512.0));
    let _ = dom.set_property(base, "CFrame", at(-10.0));
    let spawn = child(&mut dom, "SpawnLocation", "SpawnLocation", workspace);
    let _ = dom.set_property(spawn, "Anchored", Variant::Bool(true));
    let _ = dom.set_property(spawn, "Size", size(12.0, 1.0, 12.0));
    let _ = dom.set_property(spawn, "CFrame", at(0.5));
    dom.take_changes();
    dom
}

/// [`bare_place`] with about `n` loose blocks (whole clumps) built the way real maps leave
/// them: in clumps of four that overlap each other by a stud, every third
/// clump sunk 3 studs into the baseplate, and a big slab sunk 6 studs with
/// clumps on it. Roblox pushes them all out within a frame and they sleep
/// (SleepPile); the busy-place test and `bench_props --pile`.
pub fn prop_pile(n: usize) -> (WeakDom, Vec<Ref>) {
    let mut dom = bare_place();
    let workspace = service(&mut dom, "Workspace");
    let mut props = Vec::new();
    let mut add = |dom: &mut WeakDom, position: Vector3Data, size: Vector3Data| {
        let prop = child(dom, "Part", "Prop", workspace);
        let rotation = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
        let _ = dom.set_property(
            prop,
            "CFrame",
            Variant::CFrame(CFrameData { position, rotation }),
        );
        let _ = dom.set_property(prop, "Size", Variant::Vector3(size));
        props.push(prop);
    };
    let v = |x: f32, y: f32, z: f32| Vector3Data { x, y, z };
    add(&mut dom, v(0.0, -2.0, 0.0), v(20.0, 8.0, 20.0));
    let clumps = n.saturating_sub(1).div_ceil(4);
    let side = (clumps as f32).sqrt().ceil() as usize;
    for i in 0..clumps {
        let (x, z) = (
            (i % side) as f32 * 8.0 - 60.0,
            (i / side) as f32 * 8.0 - 60.0,
        );
        let on_slab = x.abs() < 8.0 && z.abs() < 8.0;
        let floor = if on_slab {
            2.0
        } else if i % 3 == 0 {
            -3.0
        } else {
            0.0
        };
        for k in 0..4 {
            let k = k as f32;
            add(
                &mut dom,
                v(x + k * 1.0, floor + 1.0 + k * 0.6, z + (k % 2.0)),
                v(2.0, 2.0, 2.0),
            );
        }
    }
    dom.take_changes();
    (dom, props)
}

/// [`bare_place`] with `n` unanchored rock-like `MeshPart`s dropped in a
/// grid onto the baseplate, a busy place's props: the busy-place test and
/// bench. Returns the place and the props, whose mesh the caller hands to
/// [`crate::physics::share_mesh`] (with [`rock`]).
pub fn prop_field(n: usize) -> (WeakDom, Vec<Ref>) {
    let mut dom = bare_place();
    let workspace = service(&mut dom, "Workspace");
    let side = (n as f64).sqrt().ceil() as usize;
    let half = side as f32 * 1.5;
    let props = (0..n)
        .map(|i| {
            let (x, z) = ((i % side) as f32, (i / side) as f32);
            let prop = child(&mut dom, "MeshPart", "Rock", workspace);
            let position = Vector3Data {
                x: x * 3.0 - half,
                y: 2.0 + (i % 3) as f32 * 0.5,
                z: z * 3.0 - half,
            };
            let rotation = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
            let _ = dom.set_property(
                prop,
                "CFrame",
                Variant::CFrame(CFrameData { position, rotation }),
            );
            let size = Vector3Data {
                x: 2.0,
                y: 1.5,
                z: 2.0,
            };
            let _ = dom.set_property(prop, "Size", Variant::Vector3(size));
            prop
        })
        .collect();
    dom.take_changes();
    (dom, props)
}

/// [`prop_field`]'s rock, an irregular solid: `(vertices, triangles)`.
pub fn rock() -> (Vec<[f32; 3]>, Vec<[u32; 3]>) {
    let vertices = vec![
        [0.0, 1.0, 0.0],
        [0.0, -0.8, 0.0],
        [1.0, 0.1, 0.0],
        [-0.9, 0.0, 0.2],
        [0.1, 0.0, 1.0],
        [-0.2, 0.2, -0.95],
    ];
    let triangles = vec![
        [0, 4, 2],
        [0, 3, 4],
        [0, 5, 3],
        [0, 2, 5],
        [1, 2, 4],
        [1, 4, 3],
        [1, 3, 5],
        [1, 5, 2],
    ];
    (vertices, triangles)
}

/// [`bare_place`] with the suite.
pub fn place() -> WeakDom {
    let mut dom = bare_place();
    add_to(&mut dom, PASSES, &[]);
    dom.take_changes();
    dom
}

/// The results in `lines` (a runner's output), by test name, one per pass.
pub fn parse(lines: &[String]) -> BTreeMap<String, Vec<Value>> {
    let mut out: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for line in lines {
        let Some((name, json)) = line.strip_prefix(PREFIX).and_then(|r| r.split_once(' ')) else {
            continue;
        };
        if let Ok(value) = serde_json::from_str(json) {
            out.entry(name.to_owned()).or_default().push(value);
        }
    }
    out
}

/// Several passes of one test folded into one result and its spread: a
/// number field is the median, its spread `max - min`; a text field is the
/// most common value, its spread every distinct value when they differ.
pub fn aggregate(passes: &[Value]) -> (Value, Value) {
    let mut fields: Vec<&String> = passes
        .iter()
        .filter_map(Value::as_object)
        .flat_map(|o| o.keys())
        .collect();
    fields.sort();
    fields.dedup();
    let (mut result, mut spread) = (serde_json::Map::new(), serde_json::Map::new());
    for field in fields {
        let values: Vec<&Value> = passes.iter().filter_map(|p| p.get(field)).collect();
        let mut numbers: Vec<f64> = values.iter().filter_map(|v| v.as_f64()).collect();
        if numbers.len() == values.len() && !numbers.is_empty() {
            numbers.sort_by(f64::total_cmp);
            let median = numbers[numbers.len() / 2];
            let range = numbers[numbers.len() - 1] - numbers[0];
            result.insert(field.clone(), serde_json::json!(median));
            spread.insert(field.clone(), serde_json::json!(range));
            continue;
        }
        let mut counts: Vec<(&Value, usize)> = Vec::new();
        for value in &values {
            match counts.iter_mut().find(|(v, _)| v == value) {
                Some((_, n)) => *n += 1,
                None => counts.push((value, 1)),
            }
        }
        counts.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        if let Some((common, _)) = counts.first() {
            result.insert(field.clone(), (*common).clone());
        }
        if counts.len() > 1 {
            let seen = counts.iter().map(|(v, _)| (*v).clone()).collect();
            spread.insert(field.clone(), Value::Array(seen));
        }
    }
    (Value::Object(result), Value::Object(spread))
}

/// Runs the suite in [`Simulation`], [`PASSES`] times like the goldens,
/// and folds the passes the same way ([`aggregate`]); `Err` with what it
/// printed when the runner never finished within `limit` s of game time.
pub fn run_in_sim(limit: f64) -> Result<BTreeMap<String, Value>, String> {
    run_some_in_sim(limit, &[], None)
}

/// What a test prints to have input done on the player's client: the
/// recorder makes it XTest input on the real client's window, rbx_sim
/// Session input (`wheel in|out N`, `drag DX DY` with the right button,
/// `key NAME` pressed and released, `click X Y` with the left button at
/// window pixel X, Y).
pub const GESTURE: &str = "CONFORMANCE_GESTURE ";

/// A gesture as rbx_sim input, one batch per frame (a drag's button is
/// down for the frame its motion comes in, as with a real mouse).
pub fn gesture_inputs(gesture: &str) -> Vec<Vec<crate::Input>> {
    use crate::Input;
    let words: Vec<&str> = gesture.split_whitespace().collect();
    let number = |i: usize| {
        words
            .get(i)
            .and_then(|w| w.parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    match words.first().copied() {
        Some("wheel") => {
            let step = if words.get(1) == Some(&"in") {
                1.0
            } else {
                -1.0
            };
            (0..number(2) as usize)
                .map(|_| vec![Input::Wheel { notches: step }])
                .collect()
        }
        Some("drag") => {
            let center = [400.0, 300.0];
            let delta = [number(1), number(2)];
            let button = |down| vec![Input::Button { button: 2, down }];
            vec![
                button(true),
                vec![Input::Move {
                    position: [center[0] + delta[0], center[1] + delta[1]],
                    delta,
                }],
                button(false),
            ]
        }
        Some("click") => {
            let position = [number(1), number(2)];
            let button = |down| vec![Input::Button { button: 1, down }];
            vec![
                vec![Input::Move {
                    position,
                    delta: [0.0, 0.0],
                }],
                button(true),
                button(false),
            ]
        }
        Some("key") => {
            let code = words.get(1).copied().unwrap_or_default().to_owned();
            let key = |down| {
                vec![Input::Key {
                    code: code.clone(),
                    down,
                }]
            };
            vec![key(true), key(false)]
        }
        _ => Vec::new(),
    }
}
/// [`run_in_sim`] for the tests in `only` (all when empty). A local player
/// joins first, as one did on the recording server (`Player1`, user id -1,
/// the way Studio names its test players): the noob, no avatar. Animations
/// come from the asset cache, or through `fetcher` when given.
pub fn run_some_in_sim(
    limit: f64,
    only: &[String],
    fetcher: Option<std::sync::Arc<dyn rbx_assets::AssetFetcher>>,
) -> Result<BTreeMap<String, Value>, String> {
    run(limit, only, fetcher, crate::link::Network::default())
}

/// [`run_some_in_sim`] with the player behind `network`, as the recording
/// server's player was behind a real one.
pub fn run_on_network(
    limit: f64,
    only: &[String],
    network: crate::link::Network,
) -> Result<BTreeMap<String, Value>, String> {
    run(limit, only, None, network)
}

fn run(
    limit: f64,
    only: &[String],
    fetcher: Option<std::sync::Arc<dyn rbx_assets::AssetFetcher>>,
    network: crate::link::Network,
) -> Result<BTreeMap<String, Value>, String> {
    let (timed, rest): (Vec<String>, Vec<String>) = selected(only)
        .map(|(_, t)| t.name.to_owned())
        .partition(|name| SCRIPT_PACED.contains(&name.as_str()));
    // Each test only depends on its own corner of the world (its place in
    // `TESTS`) and simulated time, so the suite splits over servers run
    // side by side: test i goes to server i mod `jobs`.
    let jobs = std::env::var("CONFORMANCE_JOBS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, |n| n.get().min(4)))
        .clamp(1, rest.len().max(1));
    let mut groups = vec![Vec::new(); jobs];
    for (i, name) in rest.into_iter().enumerate() {
        groups[i % jobs].push(name);
    }
    std::thread::scope(|scope| {
        let mut runs: Vec<_> = groups
            .into_iter()
            .filter(|g| !g.is_empty())
            .map(|group| (group, PASSES, false))
            .collect();
        if !timed.is_empty() {
            runs.push((timed, 1, true));
        }
        let handles: Vec<_> = runs
            .into_iter()
            .map(|(group, passes, paced)| {
                let fetcher = fetcher.clone();
                std::thread::Builder::new()
                    .stack_size(16 << 20)
                    .spawn_scoped(scope, move || {
                        run_session(limit, &group, passes, fetcher, paced, network)
                    })
                    .map_err(|err| err.to_string())
            })
            .collect();
        let mut out = BTreeMap::new();
        for handle in handles {
            let ran = handle?
                .join()
                .map_err(|_| "a conformance server panicked".to_owned())?;
            out.extend(ran?);
        }
        Ok(out)
    })
}

/// Roblox's public web answers the suite asks for (bundle and catalog item
/// details, the avatar rules, a catalog search), as they were when the
/// goldens were recorded, by `"METHOD url"`. A run without the network
/// answers from them, as an offline Local Run answers from what it kept;
/// online the live answer wins (the golden marks what churns `volatile`).
const WEB: &str = include_str!("../conformance/web.json");

/// A web cache for one run holding [`WEB`], removed when dropped.
struct WebCache(PathBuf);

impl WebCache {
    fn seeded() -> Self {
        static RUNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "rbx_sim_conformance_web_{}_{run}",
            std::process::id()
        ));
        let answers: serde_json::Map<String, Value> =
            serde_json::from_str(WEB).expect("conformance/web.json is a JSON object");
        for (request, answer) in answers {
            let Some((method, url)) = request.split_once(' ') else {
                continue;
            };
            let file = rbx_lua::server::web_cache_file(&dir, url, method, None);
            if let Some(parent) = file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(file, answer.to_string());
        }
        WebCache(dir)
    }
}

impl Drop for WebCache {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Tests of frames that overrun: they run in a session of their own whose
/// frames last as long as their Luau ran ([`crate::Pacing::Scripts`], so a
/// loaded machine can't stretch them), with the real 10 s script timeout,
/// one pass (each stalls for 3 s of real time).
const SCRIPT_PACED: &[&str] = &["FrameOverrun"];

/// The user who owns the sandbox place goldens are recorded in.
const SANDBOX_OWNER: u64 = 925308243;

/// A mesh and an image that user made, for the editables tests: private
/// assets the anonymous route can't download.
const FIXTURES: &[(u64, &[u8])] = &[
    (
        122810350731801,
        include_bytes!("../conformance/assets/122810350731801.mesh"),
    ),
    (
        126400952100520,
        include_bytes!("../conformance/assets/126400952100520.png"),
    ),
];

/// Roblox's own assets a test loads (animations, sounds, meshes): an
/// offline run finds them only in the asset cache, which a fresh machine
/// (CI) doesn't have.
const CACHED_ASSETS: &[(&str, &[u64])] = &[
    ("AnimationTrack", &[2510197830, 2510202577]),
    ("SoundPlayback", &[12222216, 9125644905]),
    ("AudioApi", &[12222216]),
    ("ToolPose", &[182393478]),
    ("EditableAssets", &[1082802, 1082804]),
];

/// The assets test `name` loads that neither the fixtures nor the asset
/// cache have: an offline run can't check it.
pub fn missing_assets(name: &str) -> Vec<u64> {
    let cache = rbx_assets::AssetCache::new(None).ok();
    CACHED_ASSETS
        .iter()
        .filter(|(test, _)| *test == name)
        .flat_map(|(_, ids)| ids.iter().copied())
        .filter(|id| !FIXTURES.iter().any(|(f, _)| f == id))
        .filter(|id| cache.as_ref().and_then(|c| c.get_id(*id)).is_none())
        .collect()
}

/// The fixtures, then `fetcher` if there is one (an offline run has none,
/// and finds nothing else).
struct Fixtures(Option<std::sync::Arc<dyn rbx_assets::AssetFetcher>>);

impl rbx_assets::AssetFetcher for Fixtures {
    fn fetch_id(&self, id: u64) -> Result<Vec<u8>, rbx_assets::FetchError> {
        if let Some((_, bytes)) = FIXTURES.iter().find(|(f, _)| *f == id) {
            return Ok(bytes.to_vec());
        }
        match &self.0 {
            Some(fetcher) => fetcher.fetch_id(id),
            None => Err(rbx_assets::FetchError::NotFound(id)),
        }
    }

    fn fetch_thumb(&self, spec: &str) -> Result<Vec<u8>, rbx_assets::FetchError> {
        match &self.0 {
            Some(fetcher) => fetcher.fetch_thumb(spec),
            // Offline, the one thumbnail the editables tests ask for is a
            // flat stand-in of its size: what they check is the size.
            None if spec.ends_with("&w=150&h=150") => Ok(OFFLINE_THUMB.to_vec()),
            None => Err(rbx_assets::FetchError::NotFound(0)),
        }
    }
}

const OFFLINE_THUMB: &[u8] = include_bytes!("../conformance/assets/thumb-150.png");

fn run_session(
    limit: f64,
    only: &[String],
    passes: u32,
    fetcher: Option<std::sync::Arc<dyn rbx_assets::AssetFetcher>>,
    script_paced: bool,
    network: crate::link::Network,
) -> Result<BTreeMap<String, Value>, String> {
    let mut dom = bare_place();
    add_to(&mut dom, passes, only);
    dom.take_changes();
    let mut session = Session::new(dom)?;
    session.set_network(network);
    // Before the services, which download models through it.
    session.set_fetcher(std::sync::Arc::new(Fixtures(fetcher)));
    let web = WebCache::seeded();
    session.set_services(rbx_lua::server::Services {
        cache_dir: Some(web.0.clone()),
        // The sandbox place's owner, whose assets the editables tests open.
        asset_owners: Some(std::sync::Arc::new(rbx_lua::server::Creators {
            users: vec![SANDBOX_OWNER],
            groups: Vec::new(),
        })),
        ..Default::default()
    });
    // A test's verdict mustn't hang on how loaded the machine is.
    if script_paced {
        session.set_pacing(crate::Pacing::Scripts);
    } else {
        session.set_script_timeout(std::time::Duration::from_secs(86_400));
    }
    session.set_join_after(crate::JOIN_AFTER);
    session.start();
    session.join("Player1", -1, None)?;
    let mut lines = Vec::new();
    let mut gestures = std::collections::VecDeque::new();
    while session.time() < limit {
        for input in gestures.pop_front().unwrap_or_default() {
            session.input(0, input);
        }
        session.step();
        for (peer, output) in session.take_output() {
            if peer != Peer::Server {
                // `CONFORMANCE_CLIENT_OUTPUT=1`: what the clients print, for
                // working out a client half.
                if std::env::var_os("CONFORMANCE_CLIENT_OUTPUT").is_some() {
                    for line in output.lines() {
                        eprintln!("client: {line}");
                    }
                }
                continue;
            }
            for line in output.lines() {
                if let Some(gesture) = line.strip_prefix(GESTURE) {
                    gestures.extend(gesture_inputs(gesture));
                }

                lines.push(line.clone());
            }
        }
        if lines.iter().any(|l| l == DONE) {
            return Ok(parse(&lines)
                .into_iter()
                .map(|(name, passes)| (name, aggregate(&passes).0))
                .collect());
        }
    }
    Err(format!(
        "the runner never finished; it printed:\n{}",
        lines.join("\n")
    ))
}

pub fn goldens_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("conformance/goldens")
}

/// A golden as stored: `status` is `recorded` or `unrecorded`.
pub fn golden(name: &str) -> Option<Value> {
    let text = std::fs::read_to_string(goldens_dir().join(format!("{name}.json"))).ok()?;
    serde_json::from_str(&text).ok()
}

/// One result field, Roblox's against ours.
#[derive(Debug, Clone)]
pub struct Row {
    pub test: String,
    pub field: String,
    pub golden: String,
    pub ours: String,
    /// `ours - golden` for numbers.
    pub delta: Option<f64>,
    pub pass: bool,
    /// Passed only against this one recorded pass (1-based), not the median.
    pub matched_pass: Option<usize>,
    /// Why this field is allowed to differ (the golden's `known_gaps`).
    pub gap: Option<String>,
    /// Passed only as live data (the golden's `volatile`): web answers
    /// that change over time (a catalog item's fields, the avatar rules)
    /// need only be the same kind of value.
    pub volatile: bool,
}

fn show(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "-".to_owned(),
    }
}

/// Two texts of whitespace-separated numbers (TweenCurves' samples, or
/// a UDim2's `{0.25, 0}, {0.5, 0}`), the same count, each pair within
/// `tolerance`: Roblox computes in single precision, so a sample can round
/// the other way at the fourth decimal.
fn numbers_match(g: Option<&Value>, o: Option<&Value>, tolerance: f64) -> bool {
    let list = |v: Option<&Value>| -> Option<Vec<f64>> {
        v?.as_str()?
            .split(|c: char| c.is_whitespace() || matches!(c, ',' | '{' | '}'))
            .filter(|w| !w.is_empty())
            .map(|w| w.parse().ok())
            .collect()
    };
    match (list(g), list(o)) {
        (Some(g), Some(o)) => {
            g.len() == o.len()
                && !g.is_empty()
                && g.iter()
                    .zip(&o)
                    .all(|(a, b)| (a - b).abs() <= tolerance + 1e-9)
        }
        _ => false,
    }
}

/// Whether two values are the same kind: both numbers, both booleans, or
/// text that opens the same way, up to its first `,` or `:` (a list with
/// the same first entry, a table's shape with the same first key, the same
/// error), so live data may change but an error never passes for an answer.
fn same_kind(a: &Value, b: &Value) -> bool {
    let opening = |s: &str| s.split([',', ':']).next().unwrap_or_default().to_owned();
    match (a, b) {
        (Value::String(a), Value::String(b)) => opening(a) == opening(b),
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

/// Field by field, every field either side has. A number passes inside the
/// test's tolerance or Roblox's own spread across passes, whichever is
/// wider; text must match.
/// Compares `ours` with a golden file (`result`, `spread`, `raw` passes).
pub fn compare(test: &Test, file: &Value, ours: &Value) -> Vec<Row> {
    let empty = serde_json::Map::new();
    let golden = file["result"].as_object().unwrap_or(&empty);
    let spread = &file["spread"];
    let raw = file["raw"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let ours = ours.as_object().unwrap_or(&empty);
    // `_` fields are traces for working things out, not results.
    let mut fields: Vec<&String> = golden
        .keys()
        .chain(ours.keys())
        .filter(|f| !f.starts_with('_'))
        .collect();
    fields.sort();
    fields.dedup();
    fields
        .into_iter()
        .map(|field| {
            let (g, o) = (golden.get(field), ours.get(field));
            let delta = match (g.and_then(Value::as_f64), o.and_then(Value::as_f64)) {
                (Some(g), Some(o)) => Some(o - g),
                _ => None,
            };
            let tolerance = test.tolerance_of(field);
            // The pass (1-based) ours is within tolerance of, when the
            // median check fails.
            let near_pass = || {
                let o = o.and_then(Value::as_f64)?;
                raw.iter()
                    .position(|pass| {
                        pass[field]
                            .as_f64()
                            .is_some_and(|p| (o - p).abs() <= tolerance + 1e-9)
                    })
                    .map(|i| i + 1)
            };
            let mut matched_pass = None;
            let mut pass = match delta {
                Some(d) => {
                    let noise = spread.get(field).and_then(Value::as_f64).unwrap_or(0.0);
                    d.abs() <= tolerance.max(noise) + 1e-9 || {
                        matched_pass = near_pass();
                        matched_pass.is_some()
                    }
                }
                // Text matches the common value or any a pass printed; a
                // list of numbers with a tolerance matches number by number.
                None => {
                    g.is_some()
                        && (g == o
                            || spread
                                .get(field)
                                .and_then(Value::as_array)
                                .is_some_and(|seen| o.is_some_and(|o| seen.contains(o)))
                            || (tolerance > 0.0 && numbers_match(g, o, tolerance)))
                }
            };
            let volatile = !pass
                && file["volatile"].get(field.as_str()).is_some()
                && matches!((g, o), (Some(g), Some(o)) if same_kind(g, o));
            pass |= volatile;
            Row {
                test: test.name.to_owned(),
                field: field.clone(),
                golden: show(g),
                ours: show(o),
                delta,
                pass,
                matched_pass,
                gap: file["known_gaps"][field.as_str()]
                    .as_str()
                    .map(str::to_owned),
                volatile,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{compare, Test};

    #[test]
    fn tolerance_keys_are_globs_and_the_most_specific_wins() {
        let test = Test {
            name: "Globs",
            source: "",
            tolerance: &[
                ("*", 0.3),
                ("*_settled", 20.0),
                ("*bounce", 0.5),
                ("first*", 4.0),
                ("*mass*", 0.001),
                ("first_bounce", 1.0),
                ("a*b*c", 7.0),
            ],
        };
        assert_eq!(test.tolerance_of("d03_settled"), 20.0);
        assert_eq!(test.tolerance_of("settled_y"), 0.3);
        assert_eq!(test.tolerance_of("big_bounce"), 0.5);
        assert_eq!(test.tolerance_of("first_dy"), 4.0);
        assert_eq!(test.tolerance_of("first_bounce"), 1.0);
        assert_eq!(test.tolerance_of("big_mass_y"), 0.001);
        assert_eq!(test.tolerance_of("a_b_c"), 7.0);
        assert_eq!(test.tolerance_of("a_c_b"), 0.3);
        let bare = Test {
            name: "Bare",
            source: "",
            tolerance: &[("x", 1.0)],
        };
        assert_eq!(bare.tolerance_of("y"), 0.0);
    }

    #[test]
    fn volatile_fields_pass_as_the_same_kind_of_value() {
        let test = Test {
            name: "Live",
            source: "",
            tolerance: &[],
        };
        let golden = serde_json::json!({
            "result": { "item": "{1,Red Cap,Hat}", "shape": "A:string,B:number", "fixed": "x" },
            "volatile": { "item": "live", "shape": "live", "fixed": "live" },
        });
        let pass = |ours: serde_json::Value| -> Vec<(String, bool, bool)> {
            compare(&test, &golden, &ours)
                .into_iter()
                .map(|r| (r.field, r.pass, r.volatile))
                .collect()
        };
        let row = |f: &str, pass: bool, volatile: bool| (f.to_owned(), pass, volatile);
        assert_eq!(
            pass(
                serde_json::json!({ "item": "{1,Blue Cap,Hat}", "shape": "A:string", "fixed": "x" })
            ),
            [
                row("fixed", true, false),
                row("item", true, true),
                row("shape", true, true)
            ]
        );
        assert_eq!(
            pass(serde_json::json!({ "item": "err: HTTP 400", "shape": 3, "fixed": "x" })),
            [
                row("fixed", true, false),
                row("item", false, false),
                row("shape", false, false)
            ]
        );
    }
}
