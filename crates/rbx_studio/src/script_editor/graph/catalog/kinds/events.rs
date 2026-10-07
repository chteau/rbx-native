//! Events and run-order flow.

use super::super::{Category::*, Code, Kind, PinType::*};
use super::*;

pub(super) const KINDS: &[Kind] = &[
    // Events.
    node(
        "start",
        "On Start",
        Events,
        pins![],
        pins![THEN],
        Code::Event(None),
    ),
    node(
        "touched",
        "Touched",
        Events,
        pins![literal("Part", Instance, "script.Parent")],
        pins![THEN, wired("hit", Instance)],
        Code::Event(Some("{Part}.Touched")),
    ),
    node(
        "touch_ended",
        "Touch Ended",
        Events,
        pins![literal("Part", Instance, "script.Parent")],
        pins![THEN, wired("hit", Instance)],
        Code::Event(Some("{Part}.TouchEnded")),
    ),
    node(
        "clicked",
        "Clicked",
        Events,
        pins![literal("Detector", Instance, "script.Parent")],
        pins![THEN, wired("player", Instance)],
        Code::Event(Some("{Detector}.MouseClick")),
    ),
    node(
        "player_added",
        "Player Added",
        Events,
        pins![],
        pins![THEN, wired("player", Instance)],
        Code::Event(Some("game:GetService(\"Players\").PlayerAdded")),
    ),
    node(
        "player_removing",
        "Player Removing",
        Events,
        pins![],
        pins![THEN, wired("player", Instance)],
        Code::Event(Some("game:GetService(\"Players\").PlayerRemoving")),
    ),
    node(
        "heartbeat",
        "Heartbeat",
        Events,
        pins![],
        pins![THEN, wired("dt", Number)],
        Code::Event(Some("game:GetService(\"RunService\").Heartbeat")),
    ),
    node(
        "child_added",
        "Child Added",
        Events,
        pins![literal("Parent", Instance, "script.Parent")],
        pins![THEN, wired("child", Instance)],
        Code::Event(Some("{Parent}.ChildAdded")),
    ),
    node(
        "attribute_changed",
        "Attribute Changed",
        Events,
        pins![
            literal("Instance", Instance, "script.Parent"),
            literal("Name", String, ""),
        ],
        pins![THEN],
        Code::Event(Some("{Instance}:GetAttributeChangedSignal({Name})")),
    ),
    // Flow.
    node(
        "branch",
        "Branch",
        Flow,
        pins![IN, wired("Condition", Bool)],
        pins![exec("True"), exec("False")],
        Code::Branch,
    ),
    node(
        "for_each",
        "For Each",
        Flow,
        pins![IN, wired("List", List)],
        pins![
            exec("Loop"),
            exec("Completed"),
            wired("Item", Any),
            wired("Index", Number)
        ],
        Code::ForEach,
    ),
    node(
        "repeat",
        "Repeat",
        Flow,
        pins![IN, literal("Count", Number, "3")],
        pins![exec("Loop"), exec("Completed"), wired("Index", Number)],
        Code::Repeat,
    ),
    node(
        "wait",
        "Wait",
        Flow,
        pins![IN, literal("Seconds", Number, "1")],
        pins![THEN],
        Code::Statement("task.wait({Seconds})"),
    ),
];
