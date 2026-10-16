# Vendored crates

## `gpui-base` (0.6.1, Apache-2.0, `longbridge/gpui-kit`)

Unmodified crates.io 0.6.1 source (its `tests/` and `benches/` dropped, and
their `[[test]]`/`[[bench]]` entries with them), patched in through the
root `Cargo.toml`'s `[patch.crates-io]`, plus two public methods on the
editor state in `src/input/base/state.rs`, each marked "rbx-native
addition":

- `selected_ranges` — every selection's range, primary first.
- `add_selection` — add a secondary selection from outside the widget.

And one fix in `src/input/editor/highlighting.rs`, marked the same way:
`InputEditorStyle::resolved` fills unset (transparent) diagnostic colours
from the palette, as it already does for every other colour. Built without
its tree-sitter highlighter, GPUI Kit's status colours are a stub that is
always transparent, so `luau-lsp`'s squiggles painted in nothing. For the
same reason `on_mouse_move` in `src/input/base/state.rs` no longer raises the
kit's diagnostic popover (painted boxless in those colours); rbx_studio's
hover provider shows the problem's message instead.

And, in `src/tree.rs`, a click on a row only selects it rather than also
expanding it (Roblox Studio expands from the arrow alone), plus a public
`TreeState::toggle_expanded` for the Explorer's chevron to call, and
`rebuild_entries` keeping the selection on its item (by id) rather than on
a bare index that expanding a row above it would shift, and remembering
it (`hidden_selection`) while a collapsed parent hides its row, so
expanding the parent selects it again, and a keyboard cursor apart from
the selection (`focused_ix`, with `focused_index`/`set_focused_index` and
`TreeEntryState::is_focused`, and `aria_active_descendant` on its row) so the
Explorer can move focus without selecting, as a multi-selecting tree must,
and `set_selected_ids`/`is_selected` so every row of the caller's
multi-selection reports itself selected (`aria_selected`).
Selecting a row moves the cursor to it; the toolkit's own key handlers act
on the cursor. All are marked
"rbx-native addition"; the last is tested from `rbx_studio`
(`shell/tree_keys/tests.rs`), since this crate's own tests do not run
inside the workspace.

And one gutter slot, `src/input/base/gutter.rs` plus `set_gutter` on the
editor state and the hooks marked the same way in `src/input/base/element.rs`:
a per-line marker cell over the line-number column that reports mouse
presses, and one line painted edge to edge. The script debugger draws its
breakpoints and the paused line through it; upstream's gutter only has line
numbers and fold chevrons.

And two setters on the editor state in `src/input/base/state.rs`, marked the
same way, for an editor set into a card of its own (the Script Templates
window): `set_line_number_gutter(width, gap)` lays the line-number column out
at least `width` wide with the numbers right-aligned `gap` before the text
(upstream sizes it to the digits plus a fixed 10 px), carried to painting as
`LastLayout::line_number_offset`; and `set_surface_colors(background,
active_line)` keeps that editor's background, gutter and current-line colours
over the theme's, which the component otherwise re-projects every frame.

The two selection methods exist because the script editor's Ctrl+D / Shift+Alt+L (add a cursor
to the next / every match of the selection) has to add selections, and
upstream keeps the selection list private (still true in 0.6.6).

To upgrade GPUI Kit: re-copy the matching `gpui-base` from
`~/.cargo/registry/src/*/`, re-apply the two methods, the two diagnostic fixes, the tree changes (selection by id, the focus cursor) and the gutter slot, and bump the version.
Delete this directory and the patch once upstream has an equivalent.

And `src/occlusion.rs` (new, marked "rbx-native addition"): a per-window
registry of where floating layers painted this frame. `popup.rs` and
`tooltip.rs` wrap their content with `occlusion::wrap`, and `dialog.rs`'s
host carries `occlusion::probe()`; rbx_studio's own `deferred` popups do
the same. During Play the editor reads `occlusion::rects` to cut those
areas out of the embedded Roblox window (X Shape), so popups draw above it.

## `gpui-pre` (0.3.4, Apache-2.0, `zed-industries/zed`'s GPUI as published)

Unmodified crates.io 0.3.4 source (its `examples/`, `tests/` and `docs/`
dropped, and their `[[example]]`/`[[test]]` entries with them), patched in
through the root `Cargo.toml`'s `[patch.crates-io]`, plus hover styles that
ease instead of snapping, each piece marked "rbx-native addition":

- `src/hover_transition.rs` (new): the `HoverTransition` global (a
  duration), `HoverFade` (one element's blend: which way it is heading and
  since when, reversing from where it got to), `mix_colors` (premultiplied,
  so a fill fading in from clear never passes through another hue) and
  `blend_refinement` (a hover refinement's fill, border, text and shadow
  colours blended toward the resting style; everything else in it applies
  at once, while hovered).
- `src/elements/div.rs`: `compute_style_internal` hands its `hover` and
  `group_hover` refinements to `refine_hover` instead of refining with
  them outright, and `InteractiveElementState` keeps the two `HoverFade`s.
  A running fade asks for the next frame; a settled one asks for nothing.
- `src/gpui.rs`: the module and its re-export.

GPUI has no property transitions of its own, so this is what lets every
`hover`/`group_hover` style in the editor — and in `gpui-base`'s widgets —
fade without each call site animating a value by hand. With no
`HoverTransition` set (or a zero duration, or `App::reduce_motion`), and for
elements without an id (no element state to keep a blend in), hover snaps
exactly as upstream's does. `rbx_studio` sets it in `scale::install`
(`tokens::DURATION_HOVER`) and tests it in `scale/hover_tests.rs`, since
this crate's own tests need fonts from outside it and do not build here.

To upgrade GPUI: re-copy the matching `gpui-pre` from
`~/.cargo/registry/src/*/`, drop the example and test targets from its
`Cargo.toml`, re-apply the three marked pieces, and bump the version.
