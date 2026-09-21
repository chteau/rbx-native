//! The Align tool's control surface and its commit path: computes the plan
//! (see `crate::align`) against the current selection and writes every moved
//! part's `CFrame` back in one undo step, the same take/put-back discipline
//! `shell::drag`'s own group drag already uses.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::{h_flex, v_flex, Selectable as _, Sizable as _};
use gpui_kit::*;
use rbx_dom::WeakDom;

use crate::align::{self, Axis, Mode, RelativeTo, Space};
use crate::properties;

use super::drag::{vector, CFRAME_PROPERTY};
use super::Shell;

/// `RBX_STUDIO_ALIGN='x,y,center,world,bounds'`: sets the toolbar's toggles
/// — axis letters (`x`/`y`/`z`, replacing the default set entirely rather
/// than adding to it), a `Mode`, a `Space` and a `RelativeTo`, in any order —
/// and runs the alignment once, right after startup — a debugging aid for a
/// screenshot of parts actually lining up, since nothing else can click the
/// popover's own buttons on the editor's behalf (see `AGENTS.md`'s safety
/// rules).
pub(crate) const ALIGN_VARIABLE: &str = "RBX_STUDIO_ALIGN";

impl Shell {
    pub(super) fn align_toggle_axis(&mut self, axis: Axis, cx: &mut Context<Self>) {
        self.align.toggle_axis(axis);
        self.refresh_align_preview(cx);
        cx.notify();
    }

    pub(super) fn align_set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        self.align.mode = mode;
        self.refresh_align_preview(cx);
        cx.notify();
    }

    pub(super) fn align_set_space(&mut self, space: Space, cx: &mut Context<Self>) {
        self.align.space = space;
        self.refresh_align_preview(cx);
        cx.notify();
    }

    pub(super) fn align_set_relative_to(
        &mut self,
        relative_to: RelativeTo,
        cx: &mut Context<Self>,
    ) {
        self.align.relative_to = relative_to;
        self.refresh_align_preview(cx);
        cx.notify();
    }

    /// Whether the popover is open, which is the whole of when the preview
    /// is drawn: ghost boxes standing around the selection with no tool
    /// open to explain them would be noise.
    pub(super) fn align_opened(&mut self, open: bool, cx: &mut Context<Self>) {
        self.align_open = open;
        self.refresh_align_preview(cx);
    }

    /// Sends the viewport the boxes the current toggles would move the
    /// selection into — `studio/align-tool.md`'s "dynamically previewing
    /// the point of alignment before confirming" — or clears them when the
    /// popover is closed.
    ///
    /// Called from every toggle, from opening and closing the popover, and
    /// from a selection change, since all four change where the alignment
    /// would put things.
    pub(crate) fn refresh_align_preview(&mut self, cx: &mut Context<Self>) {
        let boxes = if self.align_open {
            let entries = align::read_entries(&self.dom, &self.database, self.selected_all());
            let active_index = entries.len().saturating_sub(1);
            align::preview(&entries, active_index, self.align)
        } else {
            Vec::new()
        };
        self.viewport
            .update(cx, |viewport, _| viewport.set_preview(boxes));
    }

    /// Runs the current toolbar toggles against the current selection and
    /// writes every part [`align::plan`] moved, in one undo step — see this
    /// module's doc comment. A no-op (nothing pushed to history, nothing to
    /// undo) whenever the plan has nothing to move: fewer than two top-level
    /// selected instances, or every toggled axis already lined up.
    pub(crate) fn align_selected(&mut self, cx: &mut Context<Self>) {
        let entries = align::read_entries(&self.dom, &self.database, self.selected_all());
        let active_index = entries.len().saturating_sub(1);
        let moves = align::plan(&entries, active_index, self.align);
        if moves.is_empty() {
            return;
        }

        // See `shell::history`: snapshotted before the writes below, so one
        // Align is one undo step however many parts it moves.
        self.push_history();
        let mut dom = std::mem::replace(&mut self.dom, WeakDom::new());
        for (referent, position) in moves {
            let written = properties::edit::commit(
                &mut dom,
                &self.database,
                referent,
                CFRAME_PROPERTY,
                &vector(position),
            );
            if let Err(err) = written {
                self.output.push_warning(&format!("align: {err}"));
            }
        }
        self.dom = dom;
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        // Everything just moved, so where it *would* move has too.
        self.refresh_align_preview(cx);
        cx.notify();
    }

    /// [`ALIGN_VARIABLE`]: documented above. Applied after
    /// `Shell::apply_debug_select`, so it aligns whatever that (or the file
    /// itself) already selected.
    pub(super) fn apply_debug_align(&mut self, cx: &mut Context<Self>) {
        let Ok(spec) = std::env::var(ALIGN_VARIABLE) else {
            return;
        };

        let mut axes = [false; 3];
        for word in spec.split(',').map(|word| word.trim().to_ascii_lowercase()) {
            match word.as_str() {
                "x" => axes[Axis::X as usize] = true,
                "y" => axes[Axis::Y as usize] = true,
                "z" => axes[Axis::Z as usize] = true,
                "min" => self.align.mode = Mode::Min,
                "center" => self.align.mode = Mode::Center,
                "max" => self.align.mode = Mode::Max,
                "world" => self.align.space = Space::World,
                "local" => self.align.space = Space::Local,
                "bounds" => self.align.relative_to = RelativeTo::SelectionBounds,
                "active" => self.align.relative_to = RelativeTo::ActiveObject,
                "" => {}
                other => eprintln!("rbxstudio: {ALIGN_VARIABLE}: no option called {other:?}"),
            }
        }
        self.align.set_axes(axes);
        self.align_selected(cx);
    }

    /// The toolbar's own trigger: a compact popover (see this crate's
    /// `Popover`, also reachable from the Style Editor's colour fields) over
    /// a full dock panel — Align's controls are a handful of toggle buttons,
    /// the same shape `shell::toolbar`'s Move/Scale/Rotate strip already
    /// renders, not a dialog's worth of chrome.
    pub(super) fn align_popover(
        &self,
        trigger: super::chrome::Trigger,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + 'static {
        let handle = cx.entity();
        let options = self.align;

        Popover::new("align-popover")
            .trigger(trigger)
            .on_open_change({
                let handle = handle.clone();
                move |open, _, cx| {
                    let open = *open;
                    handle.update(cx, |shell, cx| shell.align_opened(open, cx));
                }
            })
            .content(move |_, _, _| align_popover(handle.clone(), options))
    }
}

/// The popover's body: one row per toggle group, then the Align button
/// itself. A free function, not a `Shell` method — `Popover::content` runs
/// with a `Context<PopoverState>`, not `Context<Shell>`, so every button
/// below reaches `Shell` through the captured `handle` the same way
/// `shell::panels`' own checkbox rows do (`Entity::update`), rather than
/// through `cx.listener`.
fn align_popover(handle: Entity<Shell>, options: align::Options) -> impl IntoElement {
    v_flex()
        .gap_2()
        .p_2()
        .min_w(px(200.))
        .child(toggle_group(
            "align-axis",
            Axis::ALL,
            |axis| axis.label(),
            |axis| options.axis_enabled(axis),
            &handle,
            |shell, axis, cx| shell.align_toggle_axis(axis, cx),
        ))
        .child(toggle_group(
            "align-mode",
            Mode::ALL,
            |mode| mode.label(),
            |mode| options.mode == mode,
            &handle,
            |shell, mode, cx| shell.align_set_mode(mode, cx),
        ))
        .child(toggle_group(
            "align-space",
            Space::ALL,
            |space| space.label(),
            |space| options.space == space,
            &handle,
            |shell, space, cx| shell.align_set_space(space, cx),
        ))
        .child(toggle_group(
            "align-relative",
            RelativeTo::ALL,
            |relative_to| relative_to.label(),
            |relative_to| options.relative_to == relative_to,
            &handle,
            |shell, relative_to, cx| shell.align_set_relative_to(relative_to, cx),
        ))
        .child(
            Button::new("align-apply")
                .label("Align")
                .primary()
                .xsmall()
                .on_click({
                    let handle = handle.clone();
                    move |_, _, cx| {
                        handle.update(cx, |shell, cx| shell.align_selected(cx));
                    }
                }),
        )
}

/// One row of buttons, one per `items` entry — every toggle group in the
/// popover (axes, mode, space, relative-to) is exactly this shape, so it is
/// built once and parameterized rather than written out four times.
fn toggle_group<T, const N: usize>(
    id: &'static str,
    items: [T; N],
    label: impl Fn(T) -> &'static str,
    selected: impl Fn(T) -> bool,
    handle: &Entity<Shell>,
    apply: impl Fn(&mut Shell, T, &mut Context<Shell>) + Copy + 'static,
) -> impl IntoElement
where
    T: Copy + 'static,
{
    h_flex()
        .gap_1()
        .children(items.into_iter().enumerate().map(|(index, item)| {
            let handle = handle.clone();
            Button::new((id, index))
                .label(label(item))
                .xsmall()
                .selected(selected(item))
                .on_click(move |_, _, cx| {
                    handle.update(cx, |shell, cx| apply(shell, item, cx));
                })
        }))
}
