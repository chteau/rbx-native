use glam::Vec3;
use rbx_dom::{CFrameData, Variant, Vector3Data, WeakDom};

use super::*;

fn control() -> Modifiers {
    Modifiers {
        control: true,
        ..Modifiers::none()
    }
}

fn shift() -> Modifiers {
    Modifiers {
        shift: true,
        ..Modifiers::none()
    }
}

#[test]
fn the_tool_digits_pick_their_tools() {
    assert_eq!(
        action_for("1", Modifiers::none()),
        Some(Action::Use(Tool::Select))
    );
    assert_eq!(
        action_for("2", Modifiers::none()),
        Some(Action::Use(Tool::Move))
    );
}

#[test]
fn the_unimplemented_tools_are_not_bound_at_all() {
    // Scale and Rotate have no draggers yet; a shortcut that silently does
    // nothing would be worse than none.
    assert_eq!(action_for("3", Modifiers::none()), None);
    assert_eq!(action_for("4", Modifiers::none()), None);
}

#[test]
fn a_modified_digit_is_not_a_tool() {
    // Studio gives Shift+2 to the move/scale snap increment field, which does
    // not exist here yet: it must not quietly mean Move instead.
    assert_eq!(action_for("2", shift()), None);
    assert_eq!(action_for("2", control()), None);
    assert_eq!(
        action_for(
            "2",
            Modifiers {
                alt: true,
                ..Modifiers::none()
            }
        ),
        None
    );
}

#[test]
fn control_or_command_l_toggles_local_orientation() {
    assert_eq!(action_for("l", control()), Some(Action::ToggleLocal));
    // ⌘L on a Mac, per creator-docs.
    let command = Modifiers {
        platform: true,
        ..Modifiers::none()
    };
    assert_eq!(action_for("l", command), Some(Action::ToggleLocal));
    // Bare L is Studio's Lock toggle, not this one.
    assert_eq!(action_for("l", Modifiers::none()), None);
}

#[test]
fn anything_else_is_left_alone() {
    assert_eq!(action_for("w", Modifiers::none()), None);
    assert_eq!(action_for("s", control()), None);
}

#[test]
fn only_the_move_tool_shows_draggers_or_drags() {
    let select = Transform::default();
    assert_eq!(select.tool, Tool::Select);
    assert_eq!(select.gizmo(), None);
    assert!(!select.drags());

    let moving = Transform {
        tool: Tool::Move,
        local: false,
    };
    assert_eq!(moving.gizmo(), Some(Gizmo { local: false }));
    assert!(moving.drags());
}

#[test]
fn the_local_toggle_reaches_the_renderer() {
    let local = Transform {
        tool: Tool::Move,
        local: true,
    };
    assert_eq!(local.gizmo(), Some(Gizmo { local: true }));
}

#[test]
fn every_tool_has_a_label_and_a_shortcut() {
    for tool in Tool::ALL {
        assert!(!tool.label().is_empty());
        assert_eq!(
            action_for(tool.shortcut(), Modifiers::none()),
            Some(Action::Use(tool)),
            "{}'s own shortcut has to pick it",
            tool.label()
        );
    }
}

#[test]
fn a_target_reads_a_parts_placement_out_of_the_dom() {
    let mut dom = WeakDom::new();
    let part = dom.new_instance("Part", "Part", None);
    let _ = dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data {
                x: 3.0,
                y: 4.0,
                z: -5.0,
            },
            // A quarter turn about Y, row-major as the format stores it.
            rotation: [0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0],
        }),
    );
    let _ = dom.set_property(
        part,
        "size",
        Variant::Vector3(Vector3Data {
            x: 4.0,
            y: 1.0,
            z: 2.0,
        }),
    );

    let target = Target::read(&dom, Some(part)).expect("a part has a placement");
    assert_eq!(target.referent, part);
    assert!((target.position() - Vec3::new(3.0, 4.0, -5.0)).length() < 1e-4);
    // The part's own X axis points along world -Z after that turn, and still
    // carries its 4-stud size — which is why the gizmo normalizes it.
    let rotation = target.rotation();
    assert!((rotation.x_axis.normalize() - Vec3::NEG_Z).length() < 1e-4);
    assert!((rotation.x_axis.length() - 4.0).abs() < 1e-4);
}

#[test]
fn something_with_no_placement_is_no_target() {
    let mut dom = WeakDom::new();
    let folder = dom.new_instance("Folder", "Folder", None);

    assert_eq!(Target::read(&dom, Some(folder)), None);
    assert_eq!(Target::read(&dom, None), None);
}
