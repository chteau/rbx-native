# Dock rearrangement — architecture note

**Status: not implemented.** UX Fixes v5 §5 asked for drag-to-rearrange
docks, with the instruction to flag it first if it needs rework of the
persistent-shell architecture. It does. This is that flag, and the design
the rework would follow.

## What exists today

`shell::workspace` renders Row D as one `h_flex` with three hardcoded
slots — Properties, the document over Output, Explorer — each a child built
once in `Shell::new`:

```rust
h_flex()
    .child(properties_column)   // w = self.properties_width
    .child(handle(Handle::Properties))
    .child(centre_column)       // document over Output
    .child(handle(Handle::Explorer))
    .child(explorer_column)     // w = self.explorer_width
```

Three things are baked into that shape:

1. **Position is source order.** "Properties is on the left" is not data
   anywhere; it is the order of three `.child()` calls. There is nothing to
   change at runtime.
2. **Size is three scalars.** `properties_width`, `explorer_width`,
   `output_height` on `Shell`. A fourth dock, or two docks sharing an edge,
   has nowhere to put its size.
3. **A panel's identity is its call site.** `explorer_dock()` builds *the*
   Explorer, including its own overflow menu and tab title. A panel that can
   move has to be addressable — `Panel::Explorer` — rather than a function
   that happens to be called in the right place.

The persistent-shell rule (`UX_GUIDELINES.md` §3) is **not** the obstacle.
That rule says the three panels are unconditional children so a tab switch
can't unmount them, and it survives rearrangement untouched: a panel that
moves between slots is still mounted continuously, which is all the rule
asks. The obstacle is purely that there are no slots — only source order.

## What it would take

**A layout tree, and panels addressed by identity.**

```rust
enum Slot {
    /// One or more panels sharing an edge, shown as tabs.
    Panels { panels: Vec<Panel>, active: usize },
    /// Two slots side by side, with the split between them.
    Split { axis: Axis, fraction: f32, before: Box<Slot>, after: Box<Slot> },
    /// The open document. Exactly one of these exists.
    Document,
}
```

`Shell` then holds one `Slot` instead of three scalars, `workspace()` walks
it, and `Panel` becomes an enum whose `render` dispatches to the existing
`explorer_dock`/`properties_dock`/`output_dock` bodies — which do not
otherwise change.

Four pieces follow from that, in dependency order:

1. **`Slot` plus `Panel`, replacing the three scalars.** Resize handles stop
   being three `Handle` variants and become "the split at this path", so
   `Drag` carries a path rather than an enum. This is the whole change in
   `shell::workspace`; nothing outside it moves.
2. **Persistence.** `Settings` already exists and already round-trips
   through one JSON document, so the layout serializes into it rather than
   into a second file. §5.6's "don't add a second persistence path" is
   satisfied for free. A layout naming a panel a future version has dropped
   must fall back to the default rather than refusing to open, the way
   `Settings::load` already treats every other field.
3. **The non-drag path first.** "Move to Left / Right / Bottom", "Float",
   "Reset Layout" as entries on a dock's existing overflow menu. Each is a
   pure `Slot` transform, testable without a window, and it is what makes
   the feature keyboard-operable at all — the reference document is explicit
   that drag-only rearrangement is inaccessible. Building this *before* the
   drag also means the drag has something to call.
4. **The drag last.** GPUI's `on_drag`/`DragMoveEvent` carry the panel;
   drop zones are computed from the same `Slot` tree, and the overlay is an
   `accent`-tinted rectangle over the half-edge under the cursor. Nothing
   here needs new state — the drop just calls the same transform the menu
   entry does.

## Estimate and recommendation

Steps 1–3 are the real work and are self-contained in `shell::workspace`
plus `settings`; step 4 is a day on top of them. **Recommendation: ship
1–3 as their own PR** — that delivers keyboard-operable rearrangement,
persistence and a reset command, which is the accessible half and the half
the reference document treats as non-negotiable — **and leave the drag to a
follow-up**, where it is a pure addition rather than a rewrite.

Doing 4 first would mean building the drag against three hardcoded slots
and then throwing it away.
