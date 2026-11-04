//! Whether a name is something a class has itself: a property, method or
//! event. In Roblox that wins over a child of the same name, so `part.Size`
//! never looks up a child called "Size".
//!
//! The embedded dump only lists properties, so methods and events are named
//! here, for `Instance` and for the classes where scripts meet them most.

use rbx_reflection::ReflectionDatabase;

const INSTANCE: &[&str] = &[
    "AddTag",
    "AncestryChanged",
    "Archivable",
    "AttributeChanged",
    "Changed",
    "ChildAdded",
    "ChildRemoved",
    "ClassName",
    "ClearAllChildren",
    "Clone",
    "Destroy",
    "Destroying",
    "DescendantAdded",
    "DescendantRemoving",
    "FindFirstAncestor",
    "FindFirstAncestorOfClass",
    "FindFirstAncestorWhichIsA",
    "FindFirstChild",
    "FindFirstChildOfClass",
    "FindFirstChildWhichIsA",
    "FindFirstDescendant",
    "GetActor",
    "GetAttribute",
    "GetAttributeChangedSignal",
    "GetAttributes",
    "GetChildren",
    "GetDebugId",
    "GetDescendants",
    "GetFullName",
    "GetPropertyChangedSignal",
    "GetTags",
    "HasTag",
    "IsAncestorOf",
    "IsDescendantOf",
    "IsA",
    "Name",
    "Parent",
    "RemoveTag",
    "SetAttribute",
    "WaitForChild",
];

/// (class, members its subclasses inherit).
const BY_CLASS: &[(&str, &[&str])] = &[
    (
        "BasePart",
        &[
            "ApplyAngularImpulse",
            "ApplyImpulse",
            "CanCollideWith",
            "CanSetNetworkOwnership",
            "GetConnectedParts",
            "GetJoints",
            "GetMass",
            "GetNetworkOwner",
            "GetPivot",
            "GetTouchingParts",
            "GetVelocityAtPosition",
            "PivotTo",
            "SetNetworkOwner",
            "TouchEnded",
            "Touched",
            "BreakJoints",
        ],
    ),
    (
        "Model",
        &[
            "GetBoundingBox",
            "GetExtentsSize",
            "GetModelCFrame",
            "GetModelSize",
            "GetPivot",
            "MoveTo",
            "PivotTo",
            "ScaleTo",
            "GetScale",
            "TranslateBy",
            "BreakJoints",
        ],
    ),
    (
        "Humanoid",
        &[
            "Died",
            "EquipTool",
            "GetState",
            "MoveTo",
            "Move",
            "TakeDamage",
            "UnequipTools",
        ],
    ),
];

/// Whether `name` is a property, method or event of `class`.
pub fn is_class_member(class: &str, name: &str) -> bool {
    let db = ReflectionDatabase::shared();
    db.resolve_property(class, name).is_some()
        || INSTANCE.contains(&name)
        || BY_CLASS
            .iter()
            .any(|(base, names)| db.is_subclass_of(class, base) && names.contains(&name))
}
