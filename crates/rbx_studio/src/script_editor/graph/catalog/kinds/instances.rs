//! The instance tree and its properties.

use super::super::{Category::*, Code, Kind, PinType::*, Prec};
use super::*;

pub(super) const KINDS: &[Kind] = &[
    // Instances.
    node(
        "get_parent",
        "Get Parent",
        Instances,
        pins![wired("Instance", Instance)],
        pins![wired("Parent", Instance)],
        expression("{Instance}.Parent", Prec::Atom),
    ),
    node(
        "get_children",
        "Get Children",
        Instances,
        pins![wired("Instance", Instance)],
        pins![wired("Children", List)],
        call("{Instance}:GetChildren()"),
    ),
    node(
        "get_descendants",
        "Get Descendants",
        Instances,
        pins![wired("Instance", Instance)],
        pins![wired("Descendants", List)],
        call("{Instance}:GetDescendants()"),
    ),
    Kind {
        names_local: Some("Name"),
        ..node(
            "find_first_child",
            "Find First Child",
            Instances,
            pins![wired("Parent", Instance), literal("Name", String, "")],
            pins![wired("Result", Instance)],
            call("{Parent}:FindFirstChild({Name})"),
        )
    },
    Kind {
        names_local: Some("Class"),
        ..node(
            "find_child_of_class",
            "Find Child Of Class",
            Instances,
            pins![
                wired("Parent", Instance),
                literal("Class", String, "Humanoid")
            ],
            pins![wired("Result", Instance)],
            call("{Parent}:FindFirstChildOfClass({Class})"),
        )
    },
    Kind {
        names_local: Some("Class"),
        ..node(
            "find_ancestor_of_class",
            "Find Ancestor Of Class",
            Instances,
            pins![
                wired("Instance", Instance),
                literal("Class", String, "Model")
            ],
            pins![wired("Result", Instance)],
            call("{Instance}:FindFirstAncestorOfClass({Class})"),
        )
    },
    Kind {
        names_local: Some("Name"),
        ..node(
            "wait_for_child",
            "Wait For Child",
            Instances,
            pins![wired("Parent", Instance), literal("Name", String, "")],
            pins![wired("Result", Instance)],
            call("{Parent}:WaitForChild({Name})"),
        )
    },
    node(
        "is_a",
        "Is A",
        Instances,
        pins![
            wired("Instance", Instance),
            literal("Class", String, "BasePart")
        ],
        pins![wired("Result", Bool)],
        call("{Instance}:IsA({Class})"),
    ),
    Kind {
        names_local: Some("Service"),
        ..node(
            "get_service",
            "Get Service",
            Instances,
            pins![literal("Service", String, "Players")],
            pins![wired("Service", Instance)],
            call("game:GetService({Service})"),
        )
    },
    node(
        "player_from_character",
        "Get Player From Character",
        Instances,
        pins![wired("Character", Instance)],
        pins![wired("Player", Instance)],
        call("game:GetService(\"Players\"):GetPlayerFromCharacter({Character})"),
    ),
    node(
        "clone",
        "Clone",
        Instances,
        pins![wired("Instance", Instance)],
        pins![wired("Clone", Instance)],
        call("{Instance}:Clone()"),
    ),
    node(
        "new_instance",
        "New Instance",
        Instances,
        pins![literal("Class", String, "Part")],
        pins![wired("Instance", Instance)],
        call("Instance.new({Class})"),
    ),
    node(
        "set_parent",
        "Set Parent",
        Instances,
        pins![
            IN,
            wired("Instance", Instance),
            literal("Parent", Instance, "workspace")
        ],
        pins![THEN],
        Code::Statement("{Instance}.Parent = {Parent}"),
    ),
    node(
        "destroy",
        "Destroy",
        Instances,
        pins![IN, wired("Instance", Instance)],
        pins![THEN],
        Code::Statement("{Instance}:Destroy()"),
    ),
    // Properties.
    node(
        "get_property",
        "Get Property",
        Properties,
        pins![
            wired("Target", Instance),
            literal("Property", String, "Name")
        ],
        pins![wired("Value", Any)],
        expression("{Target}{.Property}", Prec::Atom),
    ),
    node(
        "set_property",
        "Set Property",
        Properties,
        pins![
            IN,
            wired("Target", Instance),
            literal("Property", String, "Name"),
            literal("Value", Any, "nil")
        ],
        pins![THEN],
        Code::Statement("{Target}{.Property} = {Value}"),
    ),
    node(
        "get_attribute",
        "Get Attribute",
        Properties,
        pins![wired("Instance", Instance), literal("Name", String, "")],
        pins![wired("Value", Any)],
        call("{Instance}:GetAttribute({Name})"),
    ),
    node(
        "set_attribute",
        "Set Attribute",
        Properties,
        pins![
            IN,
            wired("Instance", Instance),
            literal("Name", String, ""),
            literal("Value", Any, "nil")
        ],
        pins![THEN],
        Code::Statement("{Instance}:SetAttribute({Name}, {Value})"),
    ),
    node(
        "get_tag",
        "Get Tag",
        Properties,
        pins![wired("Instance", Instance), literal("Tag", String, "")],
        pins![wired("Result", Bool)],
        call("{Instance}:HasTag({Tag})"),
    ),
    node(
        "add_tag",
        "Add Tag",
        Properties,
        pins![IN, wired("Instance", Instance), literal("Tag", String, "")],
        pins![THEN],
        Code::Statement("{Instance}:AddTag({Tag})"),
    ),
    node(
        "remove_tag",
        "Remove Tag",
        Properties,
        pins![IN, wired("Instance", Instance), literal("Tag", String, "")],
        pins![THEN],
        Code::Statement("{Instance}:RemoveTag({Tag})"),
    ),
    node(
        "get_name",
        "Get Name",
        Properties,
        pins![wired("Instance", Instance)],
        pins![wired("Name", String)],
        expression("{Instance}.Name", Prec::Atom),
    ),
];
