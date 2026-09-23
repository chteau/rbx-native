//! The review prompt's data: the pending batch read back out as rows the
//! Diff window can draw, plus the `RBX_STUDIO_ARGON_DIFF` screenshot aid
//! that seeds one.

use gpui_kit::Context;

use rbx_dom::Ref;

use crate::argon_client::{self, ArgonRef};

use super::{DiffRow, DiffRowKind, PendingReview, PropertyDiff, Shell, DIFF_VARIABLE};

impl Shell {
    /// The pending batch's own rows, read back off whatever this DOM (still
    /// unchanged — nothing in a pending review has been applied yet) and
    /// `self.argon.ids` currently hold, so a Diff window rebuilding this
    /// every frame (`shell::argon_diff_window`, the same "no copy of the
    /// value" rule `sequence_window` already follows) always shows the
    /// batch against what the tree actually looks like right now. Empty
    /// once nothing is pending — including right after Accept/Cancel, which
    /// is what tells that window to close itself.
    pub(in crate::shell) fn argon_diff_rows(&self) -> Vec<DiffRow> {
        let Some(pending) = &self.argon.pending else {
            return Vec::new();
        };
        let mut rows = Vec::new();
        for addition in &pending.changes.additions {
            let properties = addition
                .properties
                .iter()
                .filter_map(|(name, encoded)| {
                    let after = argon_client::decode_value(encoded)?;
                    Some(PropertyDiff {
                        name: name.clone(),
                        before: None,
                        after: format!("{after:?}"),
                    })
                })
                .collect();
            rows.push(DiffRow {
                kind: DiffRowKind::Addition,
                name: addition.name.clone(),
                class: addition.class.clone(),
                properties,
                nested: count_descendants(&addition.children),
            });
        }
        for update in &pending.changes.updates {
            let existing = self
                .argon
                .ids
                .get(&update.id)
                .and_then(|&r| self.dom.get(r));
            let name = update
                .name
                .clone()
                .or_else(|| existing.map(|i| i.name().to_owned()))
                .unwrap_or_default();
            let class = update
                .class
                .clone()
                .or_else(|| existing.map(|i| i.class().to_owned()))
                .unwrap_or_default();
            let properties = update
                .properties
                .iter()
                .flatten()
                .filter_map(|(name, encoded)| {
                    let after = argon_client::decode_value(encoded)?;
                    let before = existing
                        .and_then(|i| i.properties().get(name))
                        .map(|value| format!("{value:?}"));
                    Some(PropertyDiff {
                        name: name.clone(),
                        before,
                        after: format!("{after:?}"),
                    })
                })
                .collect();
            rows.push(DiffRow {
                kind: DiffRowKind::Update,
                name,
                class,
                properties,
                nested: 0,
            });
        }
        for &id in &pending.changes.removals {
            let existing = self.argon.ids.get(&id).and_then(|&r| self.dom.get(r));
            let (name, class) = existing
                .map(|i| (i.name().to_owned(), i.class().to_owned()))
                .unwrap_or_default();
            rows.push(DiffRow {
                kind: DiffRowKind::Removal,
                name,
                class,
                properties: Vec::new(),
                nested: 0,
            });
        }
        rows
    }

    /// `RBX_STUDIO_ARGON_DIFF=1` opens the Diff window on a small synthetic
    /// review built out of whatever the DOM already holds — a real one only
    /// exists after a live `argon serve` session pushes a batch of five or
    /// more changes at once, which nothing else can arrange on the editor's
    /// behalf, the same reason every other `RBX_STUDIO_*` var exists.
    pub(in crate::shell) fn apply_debug_argon_diff(&mut self, cx: &mut Context<Self>) {
        if std::env::var(DIFF_VARIABLE).as_deref() != Ok("1") {
            return;
        }
        self.seed_debug_diff_pending();
        self.open_argon_diff(cx);
    }

    /// Builds a `PendingReview` shaped like a real one — one addition (with
    /// a nested child, to exercise the "+N nested" row), and, against
    /// whatever this DOM's first two root services happen to be, one update
    /// (a rename plus a property change) and one removal.
    pub(super) fn seed_debug_diff_pending(&mut self) {
        let addition = argon_client::Snapshot {
            id: ArgonRef::generate(),
            parent: None,
            name: "DebugPart".to_owned(),
            class: "Part".to_owned(),
            properties: Vec::new(),
            children: vec![argon_client::Snapshot {
                id: ArgonRef::generate(),
                parent: None,
                name: "Nested".to_owned(),
                class: "Part".to_owned(),
                properties: Vec::new(),
                children: Vec::new(),
            }],
        };

        let roots: Vec<Ref> = self.dom.root_refs().iter().copied().take(2).collect();
        let mut updates = Vec::new();
        let mut removals = Vec::new();
        for (index, referent) in roots.into_iter().enumerate() {
            let id = ArgonRef::generate();
            self.argon.ids.insert(id, referent);
            self.argon.ids_rev.insert(referent, id);
            if index == 0 {
                let transparency = argon_client::encode_value(&rbx_dom::Variant::Float32(0.5));
                updates.push(argon_client::UpdatedSnapshot {
                    id,
                    name: Some("Renamed".to_owned()),
                    class: None,
                    properties: transparency.map(|value| vec![("Transparency".to_owned(), value)]),
                });
            } else {
                removals.push(id);
            }
        }

        let changes = argon_client::Changes {
            additions: vec![addition],
            updates,
            removals,
        };
        self.argon.pending = Some(PendingReview {
            additions: changes.additions.len(),
            updates: changes.updates.len(),
            removals: changes.removals.len(),
            changes,
        });
    }

    /// The review prompt's Diff button: opens the detail window
    /// (`shell::argon_diff_window`), or raises it if one is already open —
    /// there is only ever one review pending at a time, so a second window
    /// would just be the same rows twice.
    pub(in crate::shell) fn open_argon_diff(&mut self, cx: &mut Context<Self>) {
        if let Some(existing) = &self.argon_diff {
            let _ = existing.update(cx, |_, window, _| window.activate_window());
            return;
        }
        let shell = cx.entity();
        // Deferred for the same reason `open_sequence_editor` defers: this
        // runs inside the click handler's own `Shell` update, and opening a
        // window renders it immediately — reading the entity that update is
        // still holding is a panic, not something the compiler catches.
        cx.defer(move |cx| {
            let opened = crate::shell::argon_diff_window::ArgonDiffWindow::open(shell.clone(), cx);
            shell.update(cx, |shell, _| shell.argon_diff = opened);
        });
    }
}

/// One addition's own subtree, minus its own root — what [`Shell::argon_diff_rows`]
/// reports as a row's "+N nested" count.
pub(super) fn count_descendants(children: &[argon_client::Snapshot]) -> usize {
    children
        .iter()
        .map(|child| 1 + count_descendants(&child.children))
        .sum()
}
