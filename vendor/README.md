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
expanding the parent selects it again. All are marked
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
`~/.cargo/registry/src/*/`, re-apply the two methods, the two diagnostic fixes, the tree changes and the gutter slot, and bump the version.
Delete this directory and the patch once upstream has an equivalent.
