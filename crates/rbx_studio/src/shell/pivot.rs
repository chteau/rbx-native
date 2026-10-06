//! Studio's pivot tools, on the Model tab (`creator-docs`,
//! `studio/pivot-tools.md`): Edit Pivot's writes, Reset, and the ribbon
//! group holding both with the Snap checkbox. The gestures themselves are
//! `workspace_view::pivot`'s; the pivot underneath is `rbx_lua::pivot`'s,
//! the same one Luau's `GetPivot` and the Properties panel's `Origin` row
//! read.

use glam::Mat4;
use gpui_kit::assets::IconName;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use rbx_dom::Ref;
use rbx_viewer::pick;

use crate::tokens;
use crate::transform::{self, Tool};

use super::ribbon;
use super::Shell;

/// Why Reset is greyed.
const NOTHING: &str = "select a part or a model";

impl Shell {
    /// One step of an Edit Pivot drag: the pivot of the one part or model
    /// selected put on `to`, nothing else moving. One undo step for the
    /// gesture, opened by its `first` write, as for every viewport drag.
    pub(super) fn pivot_step(&mut self, to: Mat4, first: bool, cx: &mut Context<Self>) {
        let owners = self.pivot_owners();
        let [owner] = owners[..] else {
            return;
        };
        if first {
            self.push_history();
        }
        let written =
            rbx_lua::pivot::set_pivot(&mut self.dom, &self.database, owner, &transform::cframe(to));
        self.finish_pivot_write(written, cx);
    }

    /// Reset: every selected part's or model's pivot back on the centre of
    /// its bounding box (see `rbx_lua::pivot::reset`), as one undo step.
    pub(super) fn reset_pivot(&mut self, cx: &mut Context<Self>) {
        let owners = self.pivot_owners();
        if owners.is_empty() {
            return;
        }
        self.push_history();
        let written = owners.into_iter().try_for_each(|owner| {
            rbx_lua::pivot::reset(&mut self.dom, &self.database, owner).unwrap_or(Ok(()))
        });
        self.finish_pivot_write(Some(written), cx);
    }

    fn finish_pivot_write(&mut self, written: Option<Result<(), String>>, cx: &mut Context<Self>) {
        let changes = self.dom.take_changes();
        self.reflect_changes(&changes, cx);
        self.record_history_change(changes);
        if let Some(Err(err)) = written {
            self.output.push_warning(&format!("pivot: {err}"));
        }
        cx.notify();
    }

    /// Every selected part or model, a part inside a selected model left to
    /// the model (`pick::selection`'s rule) — the instances with a pivot of
    /// their own to edit.
    pub(super) fn pivot_owners(&self) -> Vec<Ref> {
        pick::selection(&self.dom, &self.database, self.selected_all())
            .iter()
            .map(|entry| entry.referent())
            .filter(|&referent| {
                rbx_lua::pivot::pivot(&self.dom, &self.database, referent).is_some()
            })
            .collect()
    }

    /// Edit Pivot's tile, and beside it Snap and Reset — Studio keeps Snap
    /// in a dropdown under the Pivot button; a row beside it is this
    /// ribbon's own way of showing a setting (see `ribbon`'s snap stack).
    pub(super) fn pivot_tiles(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let nav = &self.ribbon_nav;
        let active = self.transform.tool == Tool::Pivot;
        let edit = ribbon::tile(nav, "ribbon-pivot", IconName::LocateFixed, "Edit Pivot", cx)
            .when(active, |this| ribbon::selected(this, tokens::tool_pivot()))
            .tooltip(|window, cx| {
                super::tooltip::text(
                    "Edit Pivot — move or turn the pivot, not the part",
                    window,
                    cx,
                )
            })
            .on_click(cx.listener(|shell, _, _, cx| {
                ribbon::RibbonCommand::EditPivot.run(shell, cx);
            }));

        let snap_on = self.transform.pivot_snap;
        let snap = ribbon::live_stack_row(nav, "ribbon-pivot-snap", IconName::Magnet, "Snap", cx)
            .when(snap_on, |this| ribbon::selected(this, tokens::tool_pivot()))
            .tooltip(move |window, cx| {
                let state = if snap_on { "on" } else { "off" };
                super::tooltip::text(
                    format!("Snap ({state}) — the pivot jumps to corners, edges and centres"),
                    window,
                    cx,
                )
            })
            .on_click(cx.listener(|shell, _, _, cx| {
                ribbon::RibbonCommand::PivotSnap.run(shell, cx);
            }));
        let reset = if self.pivot_owners().is_empty() {
            ribbon::unavailable_row("ribbon-pivot-reset", IconName::RotateCcw, "Reset", NOTHING)
        } else {
            ribbon::live_stack_row(nav, "ribbon-pivot-reset", IconName::RotateCcw, "Reset", cx)
                .tooltip(|window, cx| {
                    super::tooltip::text(
                        "Reset — the pivot back to the centre of the bounding box",
                        window,
                        cx,
                    )
                })
                .on_click(cx.listener(|shell, _, _, cx| {
                    ribbon::RibbonCommand::PivotReset.run(shell, cx);
                }))
        };

        vec![
            edit.into_any_element(),
            ribbon::stack(vec![snap, reset]).into_any_element(),
        ]
    }
}
