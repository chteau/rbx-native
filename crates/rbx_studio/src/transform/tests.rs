use glam::{Mat3, Mat4, Vec3};
use rbx_dom::{CFrameData, Ref, Variant, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

use super::*;

/// `Targets::read` needs one to tell a `BasePart` from a container it has to
/// look inside (see `rbx_viewer::pick::parts_of`).
fn database() -> ReflectionDatabase {
    ReflectionDatabase::embedded()
}

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
fn alt_r_jumps_to_the_rotate_increment_field() {
    // creator-docs: "To quickly jump to the rotate increment input, press
    // Alt+R (Windows) or ⌥R (Mac)".
    let alt = Modifiers {
        alt: true,
        ..Modifiers::none()
    };
    assert_eq!(
        action_for("r", alt),
        Some(Action::FocusIncrement(SnapKind::Rotate))
    );
    // A plain `r` is not it: that one belongs to cursor dragging's quarter
    // turn (see `WorkspaceView::turn_key`).
    assert_eq!(action_for("r", Modifiers::none()), None);
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
    let targets = Targets::read(&dom, &database(), &[folder, a, b]);
    assert_eq!(targets.anchor().map(|t| t.referent), Some(a));
}

#[test]
fn an_empty_selection_has_no_anchor() {
    let (dom, ..) = three_parts();
    let targets = Targets::read(&dom, &database(), &[]);
    assert_eq!(targets.anchor(), None);
    assert_eq!(targets.centre(), None);
}

#[test]
fn one_selected_part_centres_the_gizmo_on_it() {
    let (dom, _, b, _) = three_parts();
    let targets = Targets::read(&dom, &database(), &[b]);
    let centre = targets.centre().expect("one part");
    assert!(
        (centre - Vec3::new(5.0, 0.0, 0.0)).length() < 1e-4,
        "{centre}"
    );
}

/// The fix this pair of methods exists for: with several parts selected the
/// gizmo belongs in the middle of them, not on whichever one happens to be
/// first in selection order.
#[test]
fn several_selected_parts_centre_the_gizmo_between_them_not_on_the_anchor() {
    let (dom, a, b, c) = three_parts();
    let targets = Targets::read(&dom, &database(), &[a, b, c]);

    // Unit cubes at (0,0,0), (5,0,0) and (0,0,5): the bounds run -0.5..5.5 on
    // both X and Z, so the centre is (2.5, 0, 2.5).
    let centre = targets.centre().expect("three parts");
    assert!(
        (centre - Vec3::new(2.5, 0.0, 2.5)).length() < 1e-4,
        "{centre}"
    );

    // And it is deliberately *not* the anchor, which is still `a` — the two
    // answer different questions (see `Targets`' own doc comment).
    let anchor = targets.anchor().expect("three parts");
    assert_eq!(anchor.referent, a);
    assert!((anchor.position() - centre).length() > 1.0);
}

#[test]
fn the_centre_does_not_depend_on_selection_order() {
    // Selection order decides the anchor; it must not decide where the gizmo
    // sits, or clicking the same three parts in a different order would put
    // the handles somewhere else.
    let (dom, a, b, c) = three_parts();
    let one = Targets::read(&dom, &database(), &[a, b, c])
        .centre()
        .expect("three");
    let other = Targets::read(&dom, &database(), &[c, a, b])
        .centre()
        .expect("three");
    assert!((one - other).length() < 1e-4, "{one} vs {other}");
}

/// The whole point of `Targets::translate`: every part in a group drag moves
/// by the exact same offset, so their positions relative to each other —
/// and to the anchor — never change.
#[test]
fn translating_moves_every_target_by_the_same_offset_and_preserves_their_layout() {
    let (dom, a, b, c) = three_parts();
    let mut targets = Targets::read(&dom, &database(), &[a, b, c]);
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

/// One unit cube standing `x` studs along the world X axis.
fn unit_cube_at(dom: &mut WeakDom, parent: Option<Ref>, x: f32) -> Ref {
    let part = dom.new_instance("Part", "Part", parent);
    let _ = dom.set_property(
        part,
        "CFrame",
        Variant::CFrame(CFrameData {
            position: Vector3Data { x, y: 0.0, z: 0.0 },
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }),
    );
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
}

/// A `Model` holding two unit cubes four studs apart, one of them buried in a
/// `Folder` — the shape of a real prop, and what a viewport click actually
/// selects (see `shell::selection::outermost_model`).
fn model_of_two_parts() -> (WeakDom, Ref, Ref, Ref) {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Revolver", None);
    let near = unit_cube_at(&mut dom, Some(model), 0.0);
    let group = dom.new_instance("Folder", "Group", Some(model));
    let far = unit_cube_at(&mut dom, Some(group), 4.0);
    (dom, model, near, far)
}

/// The bug this resolution exists for: selecting a `Model` — which is what
/// clicking any part inside one does — used to leave the gizmo with nothing
/// to stand on at all.
#[test]
fn a_selected_model_targets_every_part_beneath_it() {
    let (dom, model, near, far) = model_of_two_parts();
    let targets = Targets::read(&dom, &database(), &[model]);

    let referents: Vec<Ref> = targets.iter().map(|target| target.referent).collect();
    assert_eq!(referents.len(), 2);
    assert!(referents.contains(&near) && referents.contains(&far));

    // Two unit cubes at 0 and 4 studs: the bounds run -0.5..4.5, so the
    // gizmo stands at 2.
    let centre = targets.centre().expect("a model with parts under it");
    assert!(
        (centre - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-4,
        "{centre}"
    );

    // Scale and Rotate write a `Size` and a `CFrame`, which a `Model` has
    // neither of — so the anchor is a real part beneath it.
    let anchor = targets.anchor().expect("a model with parts under it");
    assert_ne!(anchor.referent, model);
    assert!(anchor.referent == near || anchor.referent == far);
}

/// A group drag of a model moves everything under it by the one offset the
/// gizmo travelled, exactly as a multi-part selection does.
#[test]
fn dragging_a_model_moves_every_part_beneath_it_together() {
    let (dom, model, ..) = model_of_two_parts();
    let mut targets = Targets::read(&dom, &database(), &[model]);
    let before: Vec<Vec3> = targets.iter().map(Target::position).collect();
    let delta = Vec3::new(0.0, 10.0, 0.0);

    let moves = targets.translate(delta);

    assert_eq!(moves.len(), 2);
    for (&(_, position), original) in moves.iter().zip(&before) {
        assert!((position - (*original + delta)).length() < 1e-4);
    }
}

/// A `Model` inside a `Model` is one drag, not two: the outer one covers
/// every part at every depth, so moving it carries the buried ones by the
/// same offset as the rest.
#[test]
fn dragging_a_model_carries_the_parts_of_a_model_nested_inside_it() {
    let mut dom = WeakDom::new();
    let outer = dom.new_instance("Model", "House", None);
    unit_cube_at(&mut dom, Some(outer), 0.0);
    let inner = dom.new_instance("Model", "Door", Some(outer));
    let deep = unit_cube_at(&mut dom, Some(inner), 9.0);

    let mut targets = Targets::read(&dom, &database(), &[outer]);
    assert_eq!(targets.len(), 2, "the nested model's part is a target too");

    let moves = targets.translate(Vec3::new(0.0, 5.0, 0.0));
    let buried = moves
        .iter()
        .find(|&&(referent, _)| referent == deep)
        .expect("the buried part moved");
    assert!((buried.1 - Vec3::new(9.0, 5.0, 0.0)).length() < 1e-4);
}

/// Selecting a model *and* something inside it names the same part twice.
/// Left in, a group drag would move it twice as far as the gizmo went.
#[test]
fn a_model_and_a_part_inside_it_name_that_part_once() {
    let (dom, model, near, far) = model_of_two_parts();
    let targets = Targets::read(&dom, &database(), &[model, near]);

    let referents: Vec<Ref> = targets.iter().map(|target| target.referent).collect();
    assert_eq!(referents.len(), 2);
    assert!(referents.contains(&near) && referents.contains(&far));
}

/// An empty container is not a regression to fix: there is genuinely nothing
/// under it to outline or transform.
#[test]
fn a_model_with_no_parts_beneath_it_is_still_no_target() {
    let mut dom = WeakDom::new();
    let model = dom.new_instance("Model", "Empty", None);
    dom.new_instance("Script", "Script", Some(model));

    let targets = Targets::read(&dom, &database(), &[model]);
    assert_eq!(targets.anchor(), None);
    assert_eq!(targets.centre(), None);
}

/// A box part standing square to the world at `position`, `size` studs a side.
fn part_at(referent: u32, position: Vec3, size: Vec3) -> Target {
    Target {
        referent: Ref::new(referent),
        model: Mat4::from_translation(position) * Mat4::from_scale(size),
    }
}

#[test]
fn a_group_scales_every_part_and_its_offset_from_the_pivot_by_one_factor() {
    // Two 2-stud cubes side by side, x from -3 to 3; the -X face of the
    // group's box stands at x = -3 and holds still while the +X one is pulled.
    let held = Targets(vec![
        part_at(1, Vec3::new(-2.0, 0.0, 0.0), Vec3::splat(2.0)),
        part_at(2, Vec3::new(2.0, 0.0, 0.0), Vec3::splat(2.0)),
    ]);
    let mut targets = held.clone();
    let pivot = Vec3::new(-3.0, 0.0, 0.0);

    let written = targets.scale_about(&held, pivot, 2.0);

    assert_eq!(written.len(), 2);
    let (_, size, position) = written[0];
    assert!(
        (size - Vec3::splat(4.0)).length() < 1e-5,
        "every part doubles"
    );
    assert!((position - Vec3::new(-1.0, 0.0, 0.0)).length() < 1e-5);
    let (_, size, position) = written[1];
    assert!((size - Vec3::splat(4.0)).length() < 1e-5);
    assert!((position - Vec3::new(7.0, 0.0, 0.0)).length() < 1e-5);
    // The far face still stands at x = -3: the first part now spans -3..1.
    assert!((targets.anchor().unwrap().position().x - 2.0 + 3.0).abs() < 1e-5);
    // And the group's own box doubled with it.
    let (min, max) = gizmo::bounds_of(targets.iter().map(|t| t.model)).unwrap();
    assert!((min.x + 3.0).abs() < 1e-5 && (max.x - 9.0).abs() < 1e-5);
}

#[test]
fn a_group_scale_is_absolute_from_the_grab_not_a_running_product() {
    let held = Targets(vec![part_at(1, Vec3::ZERO, Vec3::splat(2.0))]);
    let mut targets = held.clone();
    targets.scale_about(&held, Vec3::ZERO, 3.0);
    targets.scale_about(&held, Vec3::ZERO, 1.5);
    assert!((targets.anchor().unwrap().size() - Vec3::splat(3.0)).length() < 1e-5);
}

#[test]
fn a_group_factor_stops_where_any_part_would_leave_the_size_range() {
    let targets = Targets(vec![
        part_at(1, Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)),
        part_at(2, Vec3::ZERO, Vec3::new(10.0, 1.0, 1.0)),
    ]);
    // The 10-stud part hits a 20-stud ceiling at a factor of 2, however far
    // the handle is pulled.
    assert!((targets.factor_within(5.0, 0.001, 20.0) - 2.0).abs() < 1e-6);
    // And the floor: nothing may shrink below a tenth of a stud, which the
    // 1-stud parts reach at 0.1.
    assert!((targets.factor_within(0.01, 0.1, 20.0) - 0.1).abs() < 1e-6);
    // A factor inside the range passes through.
    assert!((targets.factor_within(1.5, 0.001, 20.0) - 1.5).abs() < 1e-6);
}

#[test]
fn a_group_rotates_about_its_centre_carrying_each_part_round_with_it() {
    let held = Targets(vec![
        part_at(1, Vec3::new(4.0, 0.0, 0.0), Vec3::splat(2.0)),
        part_at(2, Vec3::new(-4.0, 0.0, 0.0), Vec3::splat(2.0)),
    ]);
    let mut targets = held.clone();
    let quarter = Mat3::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);

    let written = targets.rotate_about(&held, Vec3::ZERO, quarter);

    // A quarter turn about Y carries +X onto -Z: the part at x = 4 swings to
    // z = -4, the other to z = 4, and each faces the new way too.
    let (_, orientation, position) = written[0];
    assert!((position - Vec3::new(0.0, 0.0, -4.0)).length() < 1e-5);
    assert!((orientation.x_axis - Vec3::NEG_Z).length() < 1e-5);
    let (_, _, position) = written[1];
    assert!((position - Vec3::new(0.0, 0.0, 4.0)).length() < 1e-5);
    // Sizes are untouched by a turn.
    assert!((targets.anchor().unwrap().size() - Vec3::splat(2.0)).length() < 1e-5);
}
