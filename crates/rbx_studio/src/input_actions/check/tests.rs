use super::*;
use crate::input_actions::{model, Family};
use rbx_dom::{Variant, Vector2Data, Vector3Data, WeakDom};
use rbx_reflection::ReflectionDatabase;

const BOOL: u32 = 0;
const DIRECTION_1D: u32 = 1;
const DIRECTION_2D: u32 = 2;
const VIEWPORT_POSITION: u32 = 4;
const OTHER_KIND: u32 = 9;
const SCRIPTABLE: u32 = 1;

fn key(name: &str) -> Variant {
    let items = ReflectionDatabase::shared()
        .enum_items("KeyCode")
        .expect("KeyCode enum is in the dump");
    let (_, value) = items
        .iter()
        .find(|(item, _)| item == name)
        .unwrap_or_else(|| panic!("KeyCode has no item {name}"));
    Variant::Enum(*value)
}

fn set(dom: &mut WeakDom, referent: Ref, property: &str, value: Variant) {
    let _ = dom.set_property(referent, property, value);
}

fn context(dom: &mut WeakDom, name: &str, priority: i32) -> Ref {
    let referent = dom.new_instance("InputContext", name, None);
    set(dom, referent, "Priority", Variant::Int32(priority));
    referent
}

fn action(dom: &mut WeakDom, parent: Option<Ref>, name: &str, kind: u32) -> Ref {
    let referent = dom.new_instance("InputAction", name, parent);
    set(dom, referent, "Type", Variant::Enum(kind));
    referent
}

fn binding(dom: &mut WeakDom, parent: Ref, name: &str) -> Ref {
    dom.new_instance("InputBinding", name, Some(parent))
}

fn run(dom: &WeakDom) -> Vec<Issue> {
    check(&model(dom, ReflectionDatabase::shared()))
}

fn has(issues: &[Issue], target: Ref, kind: &IssueKind) -> bool {
    issues
        .iter()
        .any(|issue| issue.target == target && issue.kind == *kind)
}

/// One action of `kind` in one enabled context at priority 1000, holding one
/// binding that `configure` fills in. `configure` also gets a button instance
/// to point `UIButton` at.
fn one_binding(kind: u32, configure: impl FnOnce(&mut WeakDom, Ref, Ref)) -> (Vec<Issue>, Ref) {
    let mut dom = WeakDom::new();
    let ctx = context(&mut dom, "Gameplay", 1000);
    let act = action(&mut dom, Some(ctx), "Act", kind);
    let b = binding(&mut dom, act, "Binding");
    let button = dom.new_instance("TextButton", "Sprint", None);
    configure(&mut dom, b, button);
    (run(&dom), b)
}

#[test]
fn clean_setup_has_no_issues() {
    let mut dom = WeakDom::new();
    let ctx = context(&mut dom, "Gameplay", 1000);
    let jump = action(&mut dom, Some(ctx), "Jump", BOOL);
    let keyboard = binding(&mut dom, jump, "KeyboardBinding");
    set(&mut dom, keyboard, "KeyCode", key("E"));
    let gamepad = binding(&mut dom, jump, "GamepadBinding");
    set(&mut dom, gamepad, "KeyCode", key("ButtonA"));
    let touch = binding(&mut dom, jump, "TouchBinding");
    let button = dom.new_instance("TextButton", "Jump button", None);
    set(&mut dom, touch, "UIButton", Variant::Ref(button));

    let issues = run(&dom);
    assert!(issues.is_empty(), "{issues:?}");
}

#[test]
fn action_outside_context_is_reported() {
    let mut dom = WeakDom::new();
    let loose = action(&mut dom, None, "Loose", BOOL);
    let issues = run(&dom);
    assert!(
        has(&issues, loose, &IssueKind::OutsideContext),
        "{issues:?}"
    );
}

#[test]
fn bool_action_without_gamepad_or_touch_is_missing_families() {
    let mut dom = WeakDom::new();
    let ctx = context(&mut dom, "Gameplay", 1000);
    let jump = action(&mut dom, Some(ctx), "Jump", BOOL);
    let keyboard = binding(&mut dom, jump, "KeyboardBinding");
    set(&mut dom, keyboard, "KeyCode", key("E"));

    let issues = run(&dom);
    assert!(
        has(&issues, jump, &IssueKind::MissingFamily(Family::Gamepad)),
        "{issues:?}"
    );
    assert!(
        has(&issues, jump, &IssueKind::MissingFamily(Family::Touch)),
        "{issues:?}"
    );
}

#[test]
fn viewport_position_expects_no_gamepad() {
    let mut dom = WeakDom::new();
    let ctx = context(&mut dom, "Camera", 1000);
    let look = action(&mut dom, Some(ctx), "Look", VIEWPORT_POSITION);
    let mouse = binding(&mut dom, look, "MouseBinding");
    set(&mut dom, mouse, "KeyCode", key("MousePosition"));
    let touch = binding(&mut dom, look, "TouchBinding");
    set(&mut dom, touch, "KeyCode", key("TouchPosition"));

    let issues = run(&dom);
    assert!(
        !has(&issues, look, &IssueKind::MissingFamily(Family::Gamepad)),
        "{issues:?}"
    );
}

#[test]
fn binding_with_no_input_is_reported() {
    let (issues, b) = one_binding(BOOL, |_, _, _| {});
    assert!(has(&issues, b, &IssueKind::NoInput), "{issues:?}");
}

#[test]
fn scriptable_binding_with_key_is_inconsistent() {
    let (issues, b) = one_binding(BOOL, |dom, b, _| {
        set(dom, b, "Type", Variant::Enum(SCRIPTABLE));
        set(dom, b, "KeyCode", key("E"));
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn scriptable_binding_with_button_is_inconsistent() {
    let (issues, b) = one_binding(BOOL, |dom, b, button| {
        set(dom, b, "Type", Variant::Enum(SCRIPTABLE));
        set(dom, b, "UIButton", Variant::Ref(button));
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn bool_with_composite_key_is_inconsistent() {
    let (issues, b) = one_binding(BOOL, |dom, b, _| set(dom, b, "Up", key("W")));
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn direction1d_with_left_is_inconsistent() {
    let (issues, b) = one_binding(DIRECTION_1D, |dom, b, _| set(dom, b, "Left", key("A")));
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn direction2d_with_forward_is_inconsistent() {
    let (issues, b) = one_binding(DIRECTION_2D, |dom, b, _| set(dom, b, "Forward", key("W")));
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn viewport_with_composite_key_is_inconsistent() {
    let (issues, b) = one_binding(VIEWPORT_POSITION, |dom, b, _| {
        set(dom, b, "KeyCode", key("MousePosition"));
        set(dom, b, "Up", key("W"));
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn viewport_with_other_key_is_inconsistent() {
    let (issues, b) = one_binding(VIEWPORT_POSITION, |dom, b, _| {
        set(dom, b, "KeyCode", key("E"))
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn bool_with_thumbstick_is_inconsistent() {
    let (issues, b) = one_binding(BOOL, |dom, b, _| set(dom, b, "KeyCode", key("Thumbstick1")));
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn direction1d_with_mouse_delta_is_inconsistent() {
    let (issues, b) = one_binding(DIRECTION_1D, |dom, b, _| {
        set(dom, b, "KeyCode", key("MouseDelta"));
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn button_on_non_bool_action_is_inconsistent() {
    let (issues, b) = one_binding(DIRECTION_1D, |dom, b, button| {
        set(dom, b, "UIButton", Variant::Ref(button));
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn pressed_below_released_threshold_is_inconsistent() {
    let (issues, b) = one_binding(BOOL, |dom, b, _| {
        set(dom, b, "KeyCode", key("E"));
        set(dom, b, "PressedThreshold", Variant::Float32(0.1));
        set(dom, b, "ReleasedThreshold", Variant::Float32(0.5));
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn response_curve_out_of_range_is_inconsistent() {
    let (issues, b) = one_binding(BOOL, |dom, b, _| {
        set(dom, b, "KeyCode", key("E"));
        set(dom, b, "ResponseCurve", Variant::Float32(0.5));
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn response_curve_on_non_thumbstick_direction2d_is_inconsistent() {
    let (issues, b) = one_binding(DIRECTION_2D, |dom, b, _| {
        set(dom, b, "KeyCode", key("E"));
        set(dom, b, "ResponseCurve", Variant::Float32(2.0));
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn vector2_scale_on_bool_is_inconsistent() {
    let (issues, b) = one_binding(BOOL, |dom, b, _| {
        set(dom, b, "KeyCode", key("E"));
        set(
            dom,
            b,
            "Vector2Scale",
            Variant::Vector2(Vector2Data { x: 2.0, y: 1.0 }),
        );
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn vector3_scale_on_direction2d_is_inconsistent() {
    let (issues, b) = one_binding(DIRECTION_2D, |dom, b, _| {
        set(dom, b, "KeyCode", key("E"));
        set(
            dom,
            b,
            "Vector3Scale",
            Variant::Vector3(Vector3Data {
                x: 1.0,
                y: 1.0,
                z: 2.0,
            }),
        );
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn same_key_twice_in_one_binding_is_inconsistent() {
    let (issues, b) = one_binding(DIRECTION_1D, |dom, b, _| {
        set(dom, b, "KeyCode", key("E"));
        set(dom, b, "Up", key("E"));
    });
    assert!(has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn other_kind_skips_kind_specific_inconsistency_rules() {
    let (issues, b) = one_binding(OTHER_KIND, |dom, b, button| {
        set(dom, b, "UIButton", Variant::Ref(button));
        set(dom, b, "Left", key("A"));
        set(
            dom,
            b,
            "Vector2Scale",
            Variant::Vector2(Vector2Data { x: 2.0, y: 1.0 }),
        );
    });
    assert!(!has(&issues, b, &IssueKind::Inconsistent), "{issues:?}");
}

#[test]
fn same_input_in_one_context_is_duplicate() {
    let mut dom = WeakDom::new();
    let ctx = context(&mut dom, "Gameplay", 1000);
    let sprint = action(&mut dom, Some(ctx), "Sprint", BOOL);
    let sprint_b = binding(&mut dom, sprint, "KeyboardBinding");
    set(&mut dom, sprint_b, "KeyCode", key("LeftShift"));
    let walk = action(&mut dom, Some(ctx), "Walk", BOOL);
    let walk_b = binding(&mut dom, walk, "KeyboardBinding");
    set(&mut dom, walk_b, "KeyCode", key("LeftShift"));

    let issues = run(&dom);
    assert!(
        has(
            &issues,
            sprint_b,
            &IssueKind::DuplicateInContext { other: walk_b }
        ),
        "{issues:?}"
    );
    assert!(
        has(
            &issues,
            walk_b,
            &IssueKind::DuplicateInContext { other: sprint_b }
        ),
        "{issues:?}"
    );
}

#[test]
fn duplicate_is_reported_once_per_other_binding() {
    let mut dom = WeakDom::new();
    let ctx = context(&mut dom, "Gameplay", 1000);
    let act = action(&mut dom, Some(ctx), "Move", DIRECTION_1D);
    let a = binding(&mut dom, act, "KeyboardBinding");
    set(&mut dom, a, "KeyCode", key("E"));
    set(&mut dom, a, "Up", key("E"));
    let other_act = action(&mut dom, Some(ctx), "Interact", BOOL);
    let other = binding(&mut dom, other_act, "KeyboardBinding");
    set(&mut dom, other, "KeyCode", key("E"));

    let issues = run(&dom);
    let count = issues
        .iter()
        .filter(|issue| issue.target == a && issue.kind == IssueKind::DuplicateInContext { other })
        .count();
    assert_eq!(count, 1, "{issues:?}");
}

#[test]
fn same_key_with_different_modifiers_is_not_duplicate() {
    let mut dom = WeakDom::new();
    let ctx = context(&mut dom, "Gameplay", 1000);
    let save_act = action(&mut dom, Some(ctx), "Save", BOOL);
    let save = binding(&mut dom, save_act, "KeyboardBinding");
    set(&mut dom, save, "KeyCode", key("S"));
    set(&mut dom, save, "PrimaryModifier", key("LeftControl"));
    let walk_act = action(&mut dom, Some(ctx), "Walk", BOOL);
    let walk = binding(&mut dom, walk_act, "KeyboardBinding");
    set(&mut dom, walk, "KeyCode", key("S"));

    let issues = run(&dom);
    assert!(
        !issues
            .iter()
            .any(|issue| matches!(issue.kind, IssueKind::DuplicateInContext { .. })),
        "{issues:?}"
    );
}

#[test]
fn scriptable_binding_does_not_duplicate() {
    let mut dom = WeakDom::new();
    let ctx = context(&mut dom, "Gameplay", 1000);
    let act = action(&mut dom, Some(ctx), "Fire", BOOL);
    let scripted = binding(&mut dom, act, "ScriptBinding");
    set(&mut dom, scripted, "Type", Variant::Enum(SCRIPTABLE));
    set(&mut dom, scripted, "KeyCode", key("E"));
    let other_act = action(&mut dom, Some(ctx), "Interact", BOOL);
    let other = binding(&mut dom, other_act, "KeyboardBinding");
    set(&mut dom, other, "KeyCode", key("E"));

    let issues = run(&dom);
    assert!(
        !has(
            &issues,
            other,
            &IssueKind::DuplicateInContext { other: scripted }
        ),
        "{issues:?}"
    );
}

#[test]
fn same_button_in_one_context_is_duplicate() {
    let mut dom = WeakDom::new();
    let ctx = context(&mut dom, "Gameplay", 1000);
    let button = dom.new_instance("TextButton", "Sprint", None);
    let first_act = action(&mut dom, Some(ctx), "Sprint", BOOL);
    let first = binding(&mut dom, first_act, "TouchBinding");
    set(&mut dom, first, "UIButton", Variant::Ref(button));
    let second_act = action(&mut dom, Some(ctx), "Run", BOOL);
    let second = binding(&mut dom, second_act, "TouchBinding");
    set(&mut dom, second, "UIButton", Variant::Ref(button));

    let issues = run(&dom);
    assert!(
        has(
            &issues,
            first,
            &IssueKind::DuplicateInContext { other: second }
        ),
        "{issues:?}"
    );
    assert!(
        has(
            &issues,
            second,
            &IssueKind::DuplicateInContext { other: first }
        ),
        "{issues:?}"
    );
}

/// Two contexts, each with a Bool action bound to `E`. The first context is
/// enabled or not per `enabled_a`; the second sits at `priority_b`.
fn two_contexts(priority_b: i32, enabled_a: bool) -> (Vec<Issue>, Ref, Ref) {
    let mut dom = WeakDom::new();
    let ctx_a = context(&mut dom, "Menu", 1000);
    set(&mut dom, ctx_a, "Enabled", Variant::Bool(enabled_a));
    let ctx_b = context(&mut dom, "Gameplay", priority_b);
    let open = action(&mut dom, Some(ctx_a), "Open", BOOL);
    let a = binding(&mut dom, open, "KeyboardBinding");
    set(&mut dom, a, "KeyCode", key("E"));
    let jump = action(&mut dom, Some(ctx_b), "Jump", BOOL);
    let b = binding(&mut dom, jump, "KeyboardBinding");
    set(&mut dom, b, "KeyCode", key("E"));
    (run(&dom), a, b)
}

#[test]
fn same_input_in_enabled_contexts_at_equal_priority_is_duplicate() {
    let (issues, a, b) = two_contexts(1000, true);
    assert!(
        has(&issues, a, &IssueKind::DuplicateAcrossContexts { other: b }),
        "{issues:?}"
    );
    assert!(
        has(&issues, b, &IssueKind::DuplicateAcrossContexts { other: a }),
        "{issues:?}"
    );
}

#[test]
fn same_input_at_different_priorities_is_not_duplicate() {
    let (issues, a, _) = two_contexts(500, true);
    assert!(
        !issues.iter().any(
            |issue| matches!(issue.kind, IssueKind::DuplicateAcrossContexts { .. })
                && issue.target == a
        ),
        "{issues:?}"
    );
}

#[test]
fn same_input_in_disabled_context_is_not_duplicate() {
    let (issues, a, _) = two_contexts(1000, false);
    assert!(
        !issues.iter().any(
            |issue| matches!(issue.kind, IssueKind::DuplicateAcrossContexts { .. })
                && issue.target == a
        ),
        "{issues:?}"
    );
}
