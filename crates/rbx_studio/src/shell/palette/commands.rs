//! What the palette lists and in what order: every enabled menu-bar item,
//! the transform tools, the ribbon's live commands, and a "focus" command
//! per dock and per document — read from the definitions those already
//! live in, never a second list of names. Quick Open's instance rows are
//! built by `instances`.

use std::any::TypeId;

use gpui_kit::{Action, OwnedMenu, OwnedMenuItem, SharedString};
use rbx_dom::Ref;

use crate::change_class::{letters, rank_split, words, Tier};
use crate::menu_bar::*;
use crate::shell::chrome::Document;
use crate::shell::ribbon::{self, RibbonCommand};
use crate::shell::Panel;
use crate::transform::Tool;

/// What running a row does.
pub(in crate::shell) enum Run {
    /// A menu-bar item: dispatched exactly as clicking it would be.
    Action(Box<dyn Action>),
    Tool(Tool),
    Ribbon(RibbonCommand),
    /// Opens the dock (or brings its tab forward) and, where the dock has
    /// a keyboard entry point, moves focus into it.
    Focus(Panel),
    /// Brings a document tab to the front and focuses its surface.
    Document(Document),
    /// Quick Open's own row: a script opens in the Script Editor, anything
    /// else is selected in the Explorer, as Studio's does.
    Instance(Ref),
}

pub(in crate::shell) struct Command {
    /// `Category: Name`, VS Code's shape — the category is what tells the
    /// View menu's Explorer toggle apart from focusing the Explorer. For an
    /// instance, its full path. Also the key recency is remembered by.
    pub(in crate::shell) label: SharedString,
    /// The name without its category, which a prefix match is tried on
    /// first: "save" should rank Save above Edit: Paste's scattered letters.
    pub(in crate::shell) name: SharedString,
    /// An instance's path, shown dimmed beside its name.
    pub(in crate::shell) detail: Option<SharedString>,
    /// In `Keystroke::parse` form; see [`display`].
    pub(in crate::shell) hint: Option<&'static str>,
    pub(in crate::shell) run: Run,
    /// `name` and `label` lower-cased and split into words once, here,
    /// rather than on every keystroke for every row: Quick Open ranks every
    /// instance in the place (see `change_class::rank_split`).
    name_key: Key,
    label_key: Key,
}

struct Key {
    lower: String,
    words: Vec<Vec<char>>,
}

impl Key {
    fn new(text: &str) -> Self {
        Self {
            lower: text.to_lowercase(),
            words: words(text),
        }
    }
}

impl Command {
    pub(in crate::shell) fn new(
        label: SharedString,
        name: SharedString,
        detail: Option<SharedString>,
        hint: Option<&'static str>,
        run: Run,
    ) -> Self {
        Self {
            name_key: Key::new(&name),
            label_key: Key::new(&label),
            label,
            name,
            detail,
            hint,
            run,
        }
    }

    /// What the row reads: an instance by its name (its path is the
    /// detail), a command by its whole label.
    pub(in crate::shell) fn text(&self) -> SharedString {
        match self.detail {
            Some(_) => self.name.clone(),
            None => self.label.clone(),
        }
    }
}

/// The documents' palette names. The first document's tab reads the
/// place's file name, which says nothing about what focusing it does.
const DOCUMENTS: [(Document, &str); 3] = [
    (Document::Viewport, "3D View"),
    (Document::Scripts, "Script Editor"),
    (Document::UiEditor, "UI Editor"),
];

/// Every command, in menu-bar order, then the tools, the ribbon, the docks
/// and the documents. `panels` is what the current document can show (see
/// `Shell::document_hides`).
pub(in crate::shell) fn registry(
    menus: &[OwnedMenu],
    panels: impl IntoIterator<Item = Panel>,
) -> Vec<Command> {
    let command = |category: &str, name: &str, hint, run| {
        Command::new(
            format!("{category}: {name}").into(),
            name.to_owned().into(),
            None,
            hint,
            run,
        )
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
            // The palette does not list its own openers: from inside it,
            // either would only do what typing or deleting `>` does.
            let any = action.as_any();
            if any.is::<MenuCommandPalette>() || any.is::<MenuQuickOpen>() {
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
    commands.extend(
        ribbon::palette_entries()
            .into_iter()
            .map(|(category, name, hint, run)| command(category, &name, hint, Run::Ribbon(run))),
    );
    commands.extend(panels.into_iter().map(|panel| {
        command(
            "View",
            &format!("Focus {}", panel.key()),
            None,
            Run::Focus(panel),
        )
    }));
    commands.extend(DOCUMENTS.map(|(document, name)| {
        command(
            "View",
            &format!("Focus {name}"),
            None,
            Run::Document(document),
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

/// Whether `query` asks for commands rather than instances: Studio's Quick
/// Open switches to its actions on a leading `>`, and so does this.
pub(super) fn wants_actions(query: &str) -> bool {
    query.trim_start().starts_with('>')
}

/// The rows `query` keeps, best first, as indices into `commands`.
///
/// Studio's Quick Open floats recent items to the top, so `recent` (labels,
/// most recent first) leads an empty query and breaks ties within a match
/// tier; registry order breaks the rest. Action mode's leading `>` is not
/// part of what is searched for. The label (category or path) counts only
/// as a prefix or by word starts: scattered across a long dotted path,
/// almost any letters match.
pub(super) fn filter(commands: &[Command], query: &str, recent: &[SharedString]) -> Vec<usize> {
    let query = query.trim().trim_start_matches('>').trim().to_lowercase();
    let letters = letters(&query);
    let rank_one = |key: &Key, scattered: bool| {
        rank_split(&query, &letters, &key.lower, &key.words, scattered)
    };
    let recency = |command: &Command| {
        recent
            .iter()
            .position(|label| *label == command.label)
            .unwrap_or(usize::MAX)
    };
    // `false` sorts first: within a tier, a match on the name beats one
    // that needed the category's (or the path's) letters too.
    let mut kept: Vec<(Tier, bool, usize, usize)> = commands
        .iter()
        .enumerate()
        .filter_map(|(index, command)| {
            let (tier, by_label) = match query.is_empty() {
                true => (Tier::Prefix, false),
                false => [
                    rank_one(&command.name_key, true).map(|tier| (tier, false)),
                    rank_one(&command.label_key, false).map(|tier| (tier, true)),
                ]
                .into_iter()
                .flatten()
                .min()?,
            };
            Some((tier, by_label, recency(command), index))
        })
        .collect();
    kept.sort_unstable();
    kept.into_iter().map(|(.., index)| index).collect()
}
