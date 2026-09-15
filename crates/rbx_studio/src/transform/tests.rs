use glam::{Mat3, Mat4, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};

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
    assert_eq!(
        action_for("3", Modifiers::none()),
        Some(Action::Use(Tool::Scale))
    );
    assert_eq!(
        action_for("4", Modifiers::none()),
        Some(Action::Use(Tool::Rotate))
    );
}

#[test]
fn a_digit_with_no_tool_behind_it_is_left_alone() {
    assert_eq!(action_for("5", Modifiers::none()), None);
    assert_eq!(action_for("0", Modifiers::none()), None);
}

#[test]
fn a_modified_digit_is_not_a_tool() {
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
fn shift_two_jumps_to_the_move_scale_increment_field() {
    // creator-docs: "To quickly jump to the move/scale increment input, press
    // Shift+2" — so it must not quietly mean Move instead.
    assert_eq!(
        action_for("2", shift()),
        Some(Action::FocusIncrement(SnapKind::Translate))
    );
}

#[test]
fn the_rotate_increment_shortcut_stays_unbound_while_rotate_does_not_exist() {
    // Alt+R is the docs' jump to the rotate increment field; that field has no
    // Rotate tool behind it yet, and a shortcut that does nothing visible is
    // worse than one that visibly isn't there.
    let alt = Modifiers {
        alt: true,
        ..Modifiers::none()
    };
    assert_eq!(action_for("r", alt), None);
}

#[test]
fn shift_inverts_the_snap_state_rather_than_forcing_it_on() {
    // "While transforming, you can temporarily toggle snapping by holding the
    // Shift key" — both directions.
    let on = Snap {
        enabled: true,
        increment: 2.0,
    };
    assert!(on.active(false));
    assert!(!on.active(true));

    let off = Snap {
        enabled: false,
        increment: 2.0,
    };
    assert!(!off.active(false));
    assert!(off.active(true));
}

#[test]
fn the_grid_is_the_increment_only_while_snapping_is_in_force() {
    let on = Snap {
        enabled: true,
        increment: 2.0,
    };
    assert_eq!(on.grid(false), 2.0);
    // Zero is what `rbx_viewer::snap::round_to` passes through untouched.
    assert_eq!(on.grid(true), 0.0);

    let off = Snap {
        enabled: false,
        increment: 2.0,
    };
    assert_eq!(off.grid(false), 0.0);
    assert_eq!(off.grid(true), 2.0);
}

#[test]
fn the_two_increments_are_independent_of_each_other() {
    // One checkbox and one number each, not a single shared flag.
    let mut transform = Transform::default();
    transform.rotate.enabled = false;
    assert!(transform.translate.enabled);
    assert_eq!(transform.translate.increment, 1.0);
    assert_eq!(transform.rotate.increment, 45.0);
}

#[test]
fn an_increment_field_mid_edit_leaves_the_increment_alone() {
    assert_eq!(parse_increment("0.5"), Some(0.5));
    assert_eq!(parse_increment("  4 "), Some(4.0));
    // A grid has no direction.
    assert_eq!(parse_increment("-3"), Some(3.0));
    // Everything a field passes through on its way to a number.
    assert_eq!(parse_increment(""), None);
    assert_eq!(parse_increment("1."), Some(1.0));
    assert_eq!(parse_increment("-"), None);
    assert_eq!(parse_increment("half"), None);
    assert_eq!(parse_increment("inf"), None);
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
fn every_tool_but_select_shows_handles_and_drags() {
    let select = Transform::default();
    assert_eq!(select.tool, Tool::Select);
    assert_eq!(select.gizmo(), None);
    assert!(!select.drags());

    for (tool, kind) in [
        (Tool::Move, Kind::Move),
        (Tool::Scale, Kind::Scale),
        (Tool::Rotate, Kind::Rotate),
    ] {
        let transform = Transform {
            tool,
            local: false,
            ..Transform::default()
        };
        assert_eq!(
            transform.gizmo(),
            Some(Gizmo { kind, local: false }),
            "{} draws the wrong handles",
            tool.label()
        );
        assert!(transform.drags());
    }
}

#[test]
fn the_local_toggle_reaches_the_renderer_for_every_tool() {
    // The toggle is one flag over all three tools, exactly as creator-docs
    // describes it: "you can move, scale, or rotate parts in either world
    // orientation or local orientation".
    for tool in [Tool::Move, Tool::Scale, Tool::Rotate] {
        let local = Transform {
            tool,
            local: true,
            ..Transform::default()
        };
        assert_eq!(
            local.gizmo().map(|gizmo| gizmo.local),
            Some(true),
            "{} ignored the local toggle",
            tool.label()
        );
    }
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

/// The three placements a drag can put a part in are all built back out of the
/// same model matrix, so each has to survive the round trip the next drag step
/// reads it through.
fn block() -> Target {
    let orientation = Mat3::from_rotation_y(std::f32::consts::FRAC_PI_2);
    let size = Vec3::new(4.0, 1.0, 2.0);
    Target {
        referent: Ref::new(1),
        model: Mat4::from_cols(
            (orientation.x_axis * size.x).extend(0.0),
            (orientation.y_axis * size.y).extend(0.0),
            (orientation.z_axis * size.z).extend(0.0),
            Vec3::new(3.0, 4.0, -5.0).extend(1.0),
        ),
    }
}

#[test]
fn a_targets_size_is_what_its_columns_were_scaled_by() {
    assert!((block().size() - Vec3::new(4.0, 1.0, 2.0)).length() < 1e-4);
}

#[test]
fn a_targets_orientation_has_its_size_divided_back_out() {
    let orientation = block().orientation();
    for column in 0..3 {
        assert!((orientation.col(column).length() - 1.0).abs() < 1e-4);
    }
    // A quarter turn about Y puts the part's own X on world -Z.
    assert!((orientation.x_axis - Vec3::NEG_Z).length() < 1e-4);
}

#[test]
fn resizing_a_target_keeps_its_facing_and_takes_the_centre_it_is_given() {
    let resized = block().resized_to(Vec3::new(4.0, 7.0, 2.0), Vec3::new(3.0, 7.0, -5.0));

    assert!((resized.size() - Vec3::new(4.0, 7.0, 2.0)).length() < 1e-4);
    assert!((resized.position() - Vec3::new(3.0, 7.0, -5.0)).length() < 1e-4);
    assert!((resized.orientation().x_axis - Vec3::NEG_Z).length() < 1e-4);
}

#[test]
fn rotating_a_target_keeps_its_size_and_where_it_stands() {
    let turned = block().rotated_to(Mat3::IDENTITY);

    assert!((turned.size() - Vec3::new(4.0, 1.0, 2.0)).length() < 1e-4);
    assert!((turned.position() - Vec3::new(3.0, 4.0, -5.0)).length() < 1e-4);
    assert!((turned.orientation().x_axis - Vec3::X).length() < 1e-4);
}

/// A place holding three parts at distinct positions, returned as `(dom, a,
/// b, c)`.
fn three_parts() -> (WeakDom, Ref, Ref, Ref) {
    let mut dom = WeakDom::new();
    let mut at = |x: f32, y: f32, z: f32| {
        let part = dom.new_instance("Part", "Part", None);
        let _ = dom.set_property(
            part,
            "CFrame",
            Variant::CFrame(CFrameData {
                position: Vector3Data { x, y, z },
                rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
            }),
        );
        // `Target::read` (via `pick::model_of`) needs both `CFrame` and
        // `size` before it will call this a target at all.
        let _ = dom.set_property(
            part,
            "size",
            Variant::Vector3(Vector3Data {
                x: 1.0,
                y: 1.0,
                z: 1.0,
            }),
        );
        part
    };
    let a = at(0.0, 0.0, 0.0);
    let b = at(5.0, 0.0, 0.0);
    let c = at(0.0, 0.0, 5.0);
    (dom, a, b, c)
}

#[test]
fn targets_anchor_at_the_first_referent_with_a_placement() {
    let (mut dom, a, b, _) = three_parts();
    let folder = dom.new_instance("Folder", "Folder", None);

    // A `Folder` ahead of `a` in selection order has no placement of its
    // own, so the anchor skips it rather than coming up empty.
    let targets = Targets::read(&dom, &[folder, a, b]);
    assert_eq!(targets.anchor().map(|t| t.referent), Some(a));
}

#[test]
fn an_empty_selection_has_no_anchor() {
    let (dom, ..) = three_parts();
    let targets = Targets::read(&dom, &[]);
    assert_eq!(targets.anchor(), None);
}

/// The whole point of `Targets::translate`: every part in a group drag moves
/// by the exact same offset, so their positions relative to each other —
/// and to the anchor — never change.
#[test]
fn translating_moves_every_target_by_the_same_offset_and_preserves_their_layout() {
    let (dom, a, b, c) = three_parts();
    let mut targets = Targets::read(&dom, &[a, b, c]);
    let before: Vec<Vec3> = targets.iter().map(Target::position).collect();

    let delta = Vec3::new(1.0, 2.0, 3.0);
    let moves = targets.translate(delta);

    assert_eq!(moves.len(), 3);
    for (&(_, position), original) in moves.iter().zip(&before) {
        assert!((position - (*original + delta)).length() < 1e-4);
    }

    // The relative offsets between the three parts are exactly what they
    // were before the drag — nothing rearranged itself.
    let after: Vec<Vec3> = targets.iter().map(Target::position).collect();
    assert!(((after[1] - after[0]) - (before[1] - before[0])).length() < 1e-4);
    assert!(((after[2] - after[0]) - (before[2] - before[0])).length() < 1e-4);

    // And a second call keeps moving from where the parts now stand, not
    // from where they started — exactly what a drag gesture's repeated
    // `drag_to` calls need.
    let more = targets.translate(delta);
    for (&(_, position), original) in more.iter().zip(&after) {
        assert!((position - (*original + delta)).length() < 1e-4);
    }
}
