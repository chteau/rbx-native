# Vendored crates

## `gpui-base` (0.6.1, Apache-2.0, `longbridge/gpui-kit`)

Unmodified crates.io 0.6.1 source (its `tests/` and `benches/` dropped, and
their `[[test]]`/`[[bench]]` entries with them), patched in through the
root `Cargo.toml`'s `[patch.crates-io]`, plus two public methods on the
editor state in `src/input/base/state.rs`, each marked "rbx-native
addition":

- `selected_ranges` — every selection's range, primary first.
- `add_selection` — add a secondary selection from outside the widget.

Both exist because the script editor's Ctrl+D / Shift+Alt+L (add a cursor
to the next / every match of the selection) has to add selections, and
upstream keeps the selection list private (still true in 0.6.6).

To upgrade GPUI Kit: re-copy the matching `gpui-base` from
`~/.cargo/registry/src/*/`, re-apply the two methods, and bump the version.
Delete this directory and the patch once upstream has an equivalent.
