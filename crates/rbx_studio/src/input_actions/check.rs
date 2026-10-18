//! The Input Actions dock's Check mode: what a place's input setup gets
//! wrong that Studio only shows at play time. Every rule below comes from
//! the class pages and `input/input-action-system.md`; each is a warning,
//! since the engine still runs a mismatched binding (it either ignores it
//! or converts the value).

use rbx_dom::Ref;

use super::{Action, ActionKind, Binding, Family, Model};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum IssueKind {
    /// An action with no non-scriptable binding in a family its kind
    /// expects (see `ActionKind::expected_families`).
    MissingFamily(Family),
    /// Another binding in the same context takes the same input.
    DuplicateInContext { other: Ref },
    /// A binding in another enabled context at the same `Priority` takes
    /// the same input (only when this binding's context is enabled too).
    DuplicateAcrossContexts { other: Ref },
    /// A property set that the parent action's type ignores, or values that
    /// contradict each other.
    Inconsistent,
    /// An automatic binding with no key and no `UIButton`.
    NoInput,
    /// An `InputAction` with no `InputContext` above it.
    OutsideContext,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Issue {
    /// The action (for `MissingFamily`, `OutsideContext`) or binding the
    /// issue is shown on.
    pub(crate) target: Ref,
    pub(crate) kind: IssueKind,
    /// One sentence for the row's tooltip, naming the property or key.
    pub(crate) message: String,
}

/// Every issue in `model`, actions and bindings in model order.
pub(crate) fn check(model: &Model) -> Vec<Issue> {
    let mut flat: Vec<Flat> = Vec::new();
    for (index, context) in model.contexts.iter().enumerate() {
        for action in &context.actions {
            for binding in &action.bindings {
                flat.push(Flat {
                    context: index,
                    action,
                    binding,
                });
            }
        }
    }

    let mut issues = Vec::new();
    for (index, context) in model.contexts.iter().enumerate() {
        for action in &context.actions {
            action_issues(action, &mut issues);
            for binding in &action.bindings {
                binding_issues(model, index, action, binding, &flat, &mut issues);
            }
        }
    }
    for action in &model.loose {
        issues.push(Issue {
            target: action.referent,
            kind: IssueKind::OutsideContext,
            message: format!("{} is not inside an InputContext.", action.name),
        });
        action_issues(action, &mut issues);
        for binding in &action.bindings {
            let mut found = Vec::new();
            binding_local_issues(action, binding, &mut found);
            issues.extend(found);
        }
    }
    issues
}

struct Flat<'a> {
    context: usize,
    action: &'a Action,
    binding: &'a Binding,
}

#[derive(PartialEq)]
enum InputId {
    Key(String),
    Button(Ref),
}

fn inputs(binding: &Binding) -> Vec<(InputId, String)> {
    let mut list: Vec<(InputId, String)> = binding
        .keys
        .iter()
        .map(|(_, name)| (InputId::Key(name.clone()), name.clone()))
        .collect();
    if let Some((referent, name)) = &binding.ui_button {
        list.push((InputId::Button(*referent), format!("Button: {name}")));
    }
    list
}

fn action_issues(action: &Action, issues: &mut Vec<Issue>) {
    for &family in action.kind.expected_families() {
        let covered = action
            .bindings
            .iter()
            .any(|b| !b.scriptable && b.families.contains(&family));
        if !covered {
            issues.push(Issue {
                target: action.referent,
                kind: IssueKind::MissingFamily(family),
                message: format!("{} has no binding for {}.", action.name, family.label()),
            });
        }
    }
}

fn binding_issues(
    model: &Model,
    index: usize,
    action: &Action,
    binding: &Binding,
    flat: &[Flat],
    issues: &mut Vec<Issue>,
) {
    binding_local_issues(action, binding, issues);
    if binding.scriptable {
        return;
    }
    let mine = inputs(binding);
    let this_context = &model.contexts[index];
    let mut seen: Vec<(Ref, bool)> = Vec::new();
    for other in flat {
        if other.binding.scriptable || other.binding.referent == binding.referent {
            continue;
        }
        let in_context = other.context == index;
        if !in_context {
            let other_context = &model.contexts[other.context];
            if !(this_context.enabled
                && other_context.enabled
                && this_context.priority == other_context.priority)
            {
                continue;
            }
        }
        let theirs = inputs(other.binding);
        let shared = mine.iter().find(|(id, _)| {
            theirs.iter().any(|(other_id, _)| other_id == id)
                && same_modifiers(binding, other.binding)
        });
        let Some((_, label)) = shared else { continue };
        let entry = (other.binding.referent, in_context);
        if seen.contains(&entry) {
            continue;
        }
        seen.push(entry);
        let other_ref = other.binding.referent;
        issues.push(Issue {
            target: binding.referent,
            kind: if in_context {
                IssueKind::DuplicateInContext { other: other_ref }
            } else {
                IssueKind::DuplicateAcrossContexts { other: other_ref }
            },
            message: format!(
                "{label} is also bound by {}/{}.",
                other.action.name, other.binding.name
            ),
        });
    }
}

fn same_modifiers(a: &Binding, b: &Binding) -> bool {
    a.modifiers == b.modifiers
}

fn binding_local_issues(action: &Action, binding: &Binding, issues: &mut Vec<Issue>) {
    let target = binding.referent;
    let mut push = |kind: IssueKind, message: String| {
        issues.push(Issue {
            target,
            kind,
            message,
        })
    };
    if !binding.scriptable && binding.keys.is_empty() && binding.ui_button.is_none() {
        push(
            IssueKind::NoInput,
            format!("{} has no key and no UIButton.", binding.name),
        );
    }

    let kind = action.kind;
    let known = !matches!(kind, ActionKind::Other(_));
    let mut inconsistent = |message: String| push(IssueKind::Inconsistent, message);
    let key_code = binding
        .keys
        .iter()
        .find(|(property, _)| *property == "KeyCode")
        .map(|(_, name)| name.as_str());
    let has_key = |property: &str| binding.keys.iter().any(|(p, _)| *p == property);
    let composite = binding.keys.iter().find(|(p, _)| *p != "KeyCode");

    if binding.scriptable && (!binding.keys.is_empty() || binding.ui_button.is_some()) {
        inconsistent(format!(
            "{} is scriptable, so its keys and UIButton are ignored.",
            binding.name
        ));
    }
    if known {
        match kind {
            ActionKind::Bool => {
                if let Some((property, _)) = composite {
                    inconsistent(format!("{property} is ignored by a Bool action."));
                }
            }
            ActionKind::Direction1D => {
                for property in ["Left", "Right", "Forward", "Backward"] {
                    if has_key(property) {
                        inconsistent(format!("{property} is ignored by a Direction1D action."));
                    }
                }
            }
            ActionKind::Direction2D => {
                for property in ["Forward", "Backward"] {
                    if has_key(property) {
                        inconsistent(format!("{property} is ignored by a Direction2D action."));
                    }
                }
            }
            ActionKind::ViewportPosition => {
                if let Some((property, _)) = composite {
                    inconsistent(format!(
                        "{property} is ignored by a ViewportPosition action."
                    ));
                }
                if let Some(name) = key_code {
                    if name != "MousePosition" && name != "TouchPosition" {
                        inconsistent(format!(
                            "{name} is not a position input for a ViewportPosition action."
                        ));
                    }
                }
            }
            _ => {}
        }
        if matches!(kind, ActionKind::Bool | ActionKind::Direction1D) {
            if let Some(name) = key_code {
                if matches!(
                    name,
                    "Thumbstick1"
                        | "Thumbstick2"
                        | "MouseDelta"
                        | "TouchDelta"
                        | "MousePosition"
                        | "TouchPosition"
                ) {
                    inconsistent(format!("{name} gives a vector, not a button or axis."));
                }
            }
        }
        if binding.ui_button.is_some() && kind != ActionKind::Bool {
            inconsistent("UIButton only drives a Bool action.".to_string());
        }
    }
    if binding.pressed_threshold < binding.released_threshold {
        inconsistent("PressedThreshold is below ReleasedThreshold.".to_string());
    }
    if !(1.0..=10.0).contains(&binding.response_curve) {
        inconsistent("ResponseCurve is outside 1 to 10.".to_string());
    }
    if known
        && binding.response_curve != 1.0
        && !(kind == ActionKind::Direction2D
            && matches!(key_code, Some("Thumbstick1" | "Thumbstick2")))
    {
        inconsistent(
            "ResponseCurve only applies to a thumbstick on a Direction2D action.".to_string(),
        );
    }
    if known {
        if binding.vector2_scaled && kind != ActionKind::Direction2D {
            inconsistent("Vector2Scale only applies to a Direction2D action.".to_string());
        }
        if binding.vector3_scaled && kind != ActionKind::Direction3D {
            inconsistent("Vector3Scale only applies to a Direction3D action.".to_string());
        }
    }
    for (i, (_, name)) in binding.keys.iter().enumerate() {
        if binding.keys[..i].iter().any(|(_, earlier)| earlier == name) {
            inconsistent(format!("{name} is set more than once."));
        }
    }
}

#[cfg(test)]
mod tests;
