//! What the palette lists and in what order: every enabled menu-bar item,
//! the transform tools, and a "focus" command per dock — read from the
//! definitions those already live in, never a second list of names.

use std::any::TypeId;

use gpui_kit::{Action, OwnedMenu, OwnedMenuItem, SharedString};

use crate::change_class::{rank, Tier};
use crate::menu_bar::*;
use crate::shell::Panel;
use crate::transform::Tool;

/// What running a row does.
pub(in crate::shell) enum Run {
    /// A menu-bar item: dispatched exactly as clicking it would be.
    Action(Box<dyn Action>),
    Tool(Tool),
    /// Opens the dock (or brings its tab forward) and, where the dock has
    /// a keyboard entry point, moves focus into it.
    Focus(Panel),
}

pub(in crate::shell) struct Command {
    /// `Category: Name`, VS Code's shape — the category is what tells the
    /// View menu's Explorer toggle apart from focusing the Explorer.
    pub(in crate::shell) label: SharedString,
    /// The name without its category, which a prefix match is tried on
    /// first: "save" should rank Save above Edit: Paste's scattered letters.
    name: SharedString,
    /// In `Keystroke::parse` form; see [`display`].
    pub(in crate::shell) hint: Option<&'static str>,
    pub(in crate::shell) run: Run,
}

/// Every command, in menu-bar order, then the tools, then the docks.
/// `panels` is what the current document can show (see
/// `Shell::document_hides`).
pub(in crate::shell) fn registry(
    menus: &[OwnedMenu],
    panels: impl IntoIterator<Item = Panel>,
) -> Vec<Command> {
    let command = |category: &str, name: &str, hint, run| Command {
        label: format!("{category}: {name}").into(),
        name: name.to_owned().into(),
        hint,
        run,
    };
    let mut commands = Vec::new();
    for menu in menus {
        for item in &menu.items {
            let OwnedMenuItem::Action {
                name,
                action,
                disabled: false,
                ..
            } = item
            else {
                continue;
            };
            // The palette does not list itself: running it from inside
            // itself would only reopen what is already open.
            if action.as_any().is::<MenuCommandPalette>() {
                continue;
            }
            commands.push(command(
                &menu.name,
                name.trim_end_matches('…'),
                hint(action.as_ref()),
                Run::Action(action.boxed_clone()),
            ));
        }
    }
    commands.extend(
        Tool::TRANSFORM
            .into_iter()
            .map(|tool| command("Tool", tool.label(), tool.shortcut(), Run::Tool(tool))),
    );
    commands.extend(panels.into_iter().map(|panel| {
        command(
            "View",
            &format!("Focus {}", panel.key()),
            None,
            Run::Focus(panel),
        )
    }));
    commands
}

/// The shortcuts behind menu items. `shell::save`/`history`/`clipboard`/
/// `group`/`keys` match keystrokes in code rather than through a keymap, so
/// GPUI has no binding to report for them; `tests` holds each entry to the
/// handler it names, so a changed shortcut cannot leave a stale hint here.
pub(super) const HINTS: [(ActionType, &str); 14] = [
    (TypeId::of::<MenuSave>, "ctrl-s"),
    (TypeId::of::<MenuUndo>, "ctrl-z"),
    (TypeId::of::<MenuRedo>, "ctrl-y"),
    (TypeId::of::<MenuCutInstance>, "ctrl-x"),
    (TypeId::of::<MenuCopyInstance>, "ctrl-c"),
    (TypeId::of::<MenuPasteInstance>, "ctrl-v"),
    (TypeId::of::<MenuPasteIntoInstance>, "ctrl-shift-v"),
    (TypeId::of::<MenuDuplicateInstance>, "ctrl-d"),
    (TypeId::of::<MenuDeleteInstance>, "delete"),
    (TypeId::of::<MenuGroup>, "ctrl-g"),
    (TypeId::of::<MenuUngroup>, "ctrl-shift-g"),
    (TypeId::of::<MenuInsertPart>, "ctrl-shift-p"),
    (TypeId::of::<MenuInsertFolder>, "ctrl-shift-f"),
    (TypeId::of::<MenuStudioSettings>, "alt-s"),
];

/// `TypeId::of` for one menu action, called rather than stored because a
/// `TypeId` cannot be built in a `const`.
pub(super) type ActionType = fn() -> TypeId;

fn hint(action: &dyn Action) -> Option<&'static str> {
    let id = action.as_any().type_id();
    HINTS
        .iter()
        .find(|(type_id, _)| type_id() == id)
        .map(|(_, keys)| *keys)
}

/// `ctrl-shift-g` as a person reads it: `Ctrl+Shift+G`.
pub(super) fn display(keys: &str) -> String {
    keys.split('-')
        .map(|part| match part {
            "ctrl" => "Ctrl".to_owned(),
            "shift" => "Shift".to_owned(),
            "alt" => "Alt".to_owned(),
            "delete" => "Del".to_owned(),
            key => key.to_uppercase(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// The rows `query` keeps, best first, as indices into `commands`.
///
/// Studio's Quick Open floats recent items to the top, so `recent` (labels,
/// most recent first) leads an empty query and breaks ties within a match
/// tier; registry order breaks the rest. A leading `>` is dropped: it is
/// how Studio's Quick Open switches to actions, so it is in people's hands.
pub(super) fn filter(commands: &[Command], query: &str, recent: &[SharedString]) -> Vec<usize> {
    let query = query.trim().trim_start_matches('>').trim().to_lowercase();
    let recency = |command: &Command| {
        recent
            .iter()
            .position(|label| *label == command.label)
            .unwrap_or(usize::MAX)
    };
    // `false` sorts first: within a tier, a match on the name beats one
    // that needed the category's letters too.
    let mut kept: Vec<(Tier, bool, usize, usize)> = commands
        .iter()
        .enumerate()
        .filter_map(|(index, command)| {
            let (tier, by_category) = match query.is_empty() {
                true => (Tier::Prefix, false),
                false => [
                    rank(&query, &command.name).map(|tier| (tier, false)),
                    rank(&query, &command.label).map(|tier| (tier, true)),
                ]
                .into_iter()
                .flatten()
                .min()?,
            };
            Some((tier, by_category, recency(command), index))
        })
        .collect();
    kept.sort_unstable();
    kept.into_iter().map(|(.., index)| index).collect()
}
