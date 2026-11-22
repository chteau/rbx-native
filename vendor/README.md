# Vendored crates

## `gpui-base` (0.6.1, Apache-2.0, `longbridge/gpui-kit`)

Unmodified crates.io 0.6.1 source (its `tests/` and `benches/` dropped, and
their `[[test]]`/`[[bench]]` entries with them), patched in through the
root `Cargo.toml`'s `[patch.crates-io]`, plus three public methods on the
editor state in `src/input/base/state.rs`, each marked "rbx-native
addition":

- `selected_ranges` — every selection's range, primary first.
- `add_selection` — add a secondary selection from outside the widget.
- `reveal_range` — select a range and scroll it into view on the next paint
  (applied in `src/input/base/element.rs`, marked the same way), since
  `set_selected_range` only scrolls one line toward it, and not at all in a
  tab that hasn't been laid out yet. Script Analysis, the Security Audit and
  the debugger open scripts at a line with it.
- `undo`/`redo` propagate when the input has nothing to undo or redo, so
  Ctrl+Z in a search field with no typing to take back still reaches the
  place's own Undo instead of doing nothing.

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
`TreeState::set_subtree_expanded` opens or closes a row and everything under
it for the Explorer's Expand All / Collapse All.
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
numbers and fold chevrons. While a gutter is set the line-number column
grows by `gutter::MARKER_ROOM` and the numbers move right by as much, so a
marker never sits on a digit.

And two setters on the editor state in `src/input/base/state.rs`, marked the
same way, for an editor set into a card of its own (the Script Templates
window): `set_line_number_gutter(width, gap)` lays the line-number column out
at least `width` wide with the numbers right-aligned `gap` before the text
(upstream sizes it to the digits plus a fixed 10 px), carried to painting as
`LastLayout::line_number_offset`; and `set_surface_colors(background,
active_line)` keeps that editor's background, gutter and current-line colours
over the theme's, which the component otherwise re-projects every frame.

And code folding for the script editor, marked the same way:
`set_fold_markers` draws every fold candidate as a small filled triangle
(Studio's), always visible and dimmed until the pointer is over the gutter,
in place of the component's hover-only chevron (`element.rs`);
`folded_lines`, `foldable_lines` and `set_folded` on the editor state expose
the otherwise private fold set so the Shell can save and restore it;
`set_selected_range` (so `reveal_range`) and the search's next/previous match
open a fold hiding their target (`unfold_offset`); fold candidates are
replaced rather than merged after an edit, and a collapsed fold follows its
candidate's new extent (`fold_map.rs`), because a parse-based highlighter
answers with the whole file and an edit can change a block whose opener is
outside the edited lines.

The two selection methods exist because the script editor's Ctrl+D / Shift+Alt+L (add a cursor
to the next / every match of the selection) has to add selections, and
upstream keeps the selection list private (still true in 0.6.6).

To upgrade GPUI Kit: re-copy the matching `gpui-base` from
`~/.cargo/registry/src/*/`, re-apply the two methods, the two diagnostic fixes, the tree changes (selection by id, the focus cursor, the subtree expand) and the gutter slot, and bump the version.
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
ease instead of snapping and one window fix, each piece marked "rbx-native
addition":

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
- `src/window.rs`: `Window::new` maps the platform window only when
  `WindowOptions::show` asks for it. X11 and Wayland mapped every window
  regardless, so Home's hidden Settings host flashed over the screen.

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
`Cargo.toml`, re-apply the marked pieces, and bump the version.

## `gpui-pre-apple` (0.3.4, Apache-2.0, `zed-industries/zed`'s `gpui_apple` as published)

Unmodified crates.io 0.3.4 source (its `Cargo.lock` and `.cargo-ok` dropped)
but for one fix in `src/metal_renderer.rs`, marked "rbx-native fix": the
sprite and path pipelines blend the destination alpha with
`OneMinusSourceAlpha` instead of `One`. Upstream adds the alphas of
stacked shapes, so on macOS a 67% dock over a 67% background came out fully
opaque (Linux and Windows composite them as 89%), and see-through themes
showed nothing behind the docks, ribbon or text boxes. Colour blending is
unchanged; only the window's coverage, which the macOS compositor reads,
is. Re-apply it (two lines) when upgrading with `gpui-pre`.


## `gpui-pre-windows` (0.3.4, Apache-2.0, `zed-industries/zed`'s `gpui_windows` as published)

Unmodified crates.io 0.3.4 source (its `Cargo.lock` and `.cargo-ok` dropped)
but for the same fix as `gpui-pre-apple`, in `src/directx_renderer.rs` and
marked "rbx-native fix": the main and path-sprite blend states use
`D3D11_BLEND_INV_SRC_ALPHA` for the destination alpha instead of
`D3D11_BLEND_ONE`. Upstream adds the alphas of stacked shapes, so on
Windows the DirectComposition swap chain came out opaque under every
see-through theme. Re-apply it (two lines) when upgrading with `gpui-pre`.

Also marked "rbx-native fix", in `src/directx_renderer.rs`:
`CreateTargetForHwnd` gets `topmost: false`, so the editor's composition
tree sits behind child windows. Upstream draws it on top, which hid the
embedded Roblox client (a child `HWND` over the 3D view) under the editor.

And in `src/platform.rs`, the save dialog's file types come from the
suggested name's extension (`place.rbxl` offers Roblox place, Roblox XML
place, then All files) instead of only "All files", and a dialog with no
active window of ours to hang off uses our first window, so it opens over
the editor rather than in the screen's corner.

And one more in `src/window.rs`, marked the same way: `WindowBorderOffset`
measures how far the client area sits in from the window's left and top
edge (`ClientToScreen`) and `calculate_window_rect`/`calculate_client_rect`
use that instead of half of each offset. A window with a hidden title bar
has no frame at the top, so upstream opened it half the bottom frame
(about 4 px at 125%) above the bounds it was given, and a window reopened
at its saved bounds crept up every time.

And, in the same file, a `WindowKind::Floating` window is created owned by
the root owner of the window active when it opens (`GetActiveWindow`,
`GetAncestor(GA_ROOTOWNER)`) and without `WS_EX_APPWINDOW`. Upstream made it
a plain top-level window, so a torn-out plugin dock went behind the editor
the moment the editor was clicked. Owned, it stays above the editor, hides
with it when minimized and has no taskbar button of its own.


## `luau0-src` (0.24.1+luau748, MIT, `mlua-rs/luau-src-rs`)

luau0-src 0.24.0+luau747 (its `testcrate` dropped, and its `[workspace]`
with it) with the Luau sources under `luau/` (`Ast`, `Bytecode`, `CodeGen`,
`Common`, `Compiler`, `Config`, `Inliner`, `Require`, `VM`) replaced by
upstream `luau-lang/luau`'s 0.748 release, unmodified, and the version
raised to 0.24.1+luau748 so mlua-sys's `luau0-src = "0.24.0"` takes it.
`luau/Custom` (mlua's `luau_setfflag` and two helpers) is luau0-src's own.
One patch, marked `rbx-native patch` in `Compiler/src/Compiler.cpp`: the
fast `pcall`/`xpcall` call is only compiled for a global left out of
`mutableGlobals`. Scripts run in a safe environment, where the fast call
goes straight to the builtin; `rbx_lua` lists `pcall`/`xpcall` as mutable
so a call reaches its own `pcall`, which hands scripts a binding error's
message rather than its error object. Re-apply it on a bump.
Patched in through the root `Cargo.toml`, which also takes `mlua` and
`mlua-sys` from mlua's master at a pinned commit: no mlua release has the
Luau 0.747 C API yet (the `integer` type is gone, so the type tags moved),
and 0.747 to 0.748 changed no public header.

Why: scripts must parse and run as on live Roblox, the day Roblox turns a
feature on. `rbx_lua::LUAU_VERSION` records the Luau live Roblox runs, and
`luau::tests::the_linked_luau_is_not_behind_roblox` fails when this build
links an older one.

Finding Roblox's version: every fast flag a Luau release adds is in the
Studio binary that ships it and every flag it removes is gone, so compare
`strings RobloxStudioBeta.exe` against each release's
`LUAU_FASTFLAGVARIABLE`s (Studio 0.748 matches 0.748 exactly). The flags
Roblox has on are in `https://clientsettingscdn.roblox.com/v2/settings/application/PCStudioApp`.

Bumping:

1. Download the release
   (`https://codeload.github.com/luau-lang/luau/tar.gz/refs/tags/0.NNN`) and
   replace each of the nine directories above with its `include` and `src`.
2. Raise the version here and in `vendor/luau0-src/Cargo.toml`
   (`+luauNNN`; keep it semver-compatible with what mlua-sys asks for),
   then `cargo update -p luau0-src`.
3. If the release changed `VM/include` or `Compiler/include`, mlua-sys's
   bindings must change with it: move the `mlua`/`mlua-sys` pin to an mlua
   commit that bumped to that Luau, or back to crates.io once a release has.
4. Raise `rbx_lua::LUAU_VERSION`, and refresh `ROBLOX_FFLAGS` in
   `crates/rbx_lua/src/luau.rs` from the live settings (the flags Luau's
   VM, compiler and code generator define that Roblox has on).
5. Run `cargo test -p rbx_lua luau::` and `-p rbx_bytecode` (the Bytecode
   dock's shim compiles against these headers).

Once an mlua release bundles a Luau at least as new as Roblox's, drop this
directory and the three patch lines.
