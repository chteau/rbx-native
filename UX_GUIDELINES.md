# UX & Visual Design Guidelines

Rules for anyone — human or agent — touching how `rbx_studio` looks.
Companion to [GUIDELINES.md](GUIDELINES.md) (Rust style) and
[SPECS.md](SPECS.md) (architecture): this one is about what the editor
should look and feel like. `agents/AGENTS.md`'s "Verifying a change" section
still governs *how* a visual change is checked (build the binary, look at a
real screenshot); this file is about what that screenshot should show.

**The one rule everything else follows:** every colour, radius, spacing
step, elevation, duration and text style comes from
[`crates/rbx_studio/src/tokens.rs`](crates/rbx_studio/src/tokens.rs). A
literal `rgb(0x…)` or `px(13.)` in chrome is a bug — it drifts the moment a
token changes. If no token fits, add one there, named for what it is for.

## 1. Where the design comes from

The discipline is borrowed from [Vercel's Geist design
system](https://vercel.com/geist/introduction): a fixed radius scale, a
base-4 spacing scale, and "shadow-as-border" elevation — a 1px ring drawn
*as a shadow* plus a soft blur layer, instead of a hard `border` that takes
up layout space and can shift what it outlines.

What is deliberately **not** borrowed is Geist's light, monochrome,
tight-and-flat look. This editor diverges on purpose: a dark desaturated
palette, rounder radii, softer and blurrier shadows, and elastic rather than
minimal motion. Borrow Geist's *rigour*, not its appearance.

## 2. Tokens

| Group | Tokens | Notes |
|---|---|---|
| Surfaces | `bg_0` `#16171A` → `bg_3` `#2A2B33`, `bg_viewport` `#101114` | shell → panel → hover → pressed. The viewport is darker than any panel, so the render reads as a window cut into the shell |
| Borders | `border_soft` (white 6%), `border_mid` (10%) | soft = decorative seam, mid = functional edge. See §4 |
| Text | `text_primary` `text_secondary` `text_disabled` `text_error` | |
| Accent | `accent` `#6C8CFF`, `accent_soft_bg`, `accent_soft_bg_hover` | the only saturated hue in the editor |
| Radius | `RADIUS_XS` 4 · `SM` 8 · `MD` 12 · `LG` 16 · `PILL` | xs chips, sm buttons/inputs/menu items, md ribbon groups/popovers, lg panels/menu containers, pill = the committed tab |
| Spacing | `SPACE_1` 4 → `SPACE_4` 16 | base-4. Need more? Double, and add the token |
| Elevation | `elevation_1` ribbon · `elevation_2` menus/popovers · `elevation_3(Cast)` dock panels | `Cast` points the blur *away* from the viewport: the left column throws right, the right column throws left, a bottom dock throws up |
| Motion | `DURATION_MENU`, `easing_soft` | one-shot entrances only — see §9 |
| Type | `UI_LABEL_*`, `SECTION_HEADER_*`, `TREE_ROW_*`, `INPUT_VALUE_*` | `SECTION_HEADER` is uppercased at the call site; GPUI has no `text-transform` |

Tokens with nothing to bind to were **left out rather than parked as
decoration** — no modal elevation until there's a modal, no press-curve
until GPUI can transform an element. Re-adding one means implementing what
it's for.

## 3. Structure: Rows A–D

```
Shell (v_flex)
├─ MenuBar              h 32   bg-0            File / Edit / Model / View
├─ ROW A  DocumentTabs  h 40   bg-0            + the open document's own controls, right-aligned
├─ ROW B  RibbonTabs    h 36   bg-0            Home | Model | Test
├─ ROW C  Ribbon        h 88   bg-1  elev-1    groups for Row B's active tab
├─ ROW D  Workspace     flex-1
│   ├─ Properties   w 280 (200–420)  radius-lg top  elev-3 cast right
│   ├─ ⇔ handle     4px hit / 1px rest / 2px hover, col-resize
│   ├─ Centre       flex-1: document (bg-viewport) over Output
│   │   └─ Output   h 160 (80–400) or 32 collapsed, radius-md top, elev-3 cast up
│   ├─ ⇔ handle
│   └─ Explorer     w 260 (200–420)  radius-lg top  elev-3 cast left
└─ CommandBar           h 32   bg-0
```

**The independence rule.** Explorer, Properties and Output are built once in
`Shell::new` and rendered unconditionally as Row D children. Row A swaps
*only* the centre column's contents; Row B swaps *only* Row C's groups.
Neither can unmount a panel, so scroll positions and selections have nowhere
to get lost. Keep it that way: if a panel ever becomes conditional on a tab,
that rule is broken and the bug it causes (a reset scroll, a lost selection)
will look like a mystery rather than a layout change.

**Row D is hand-rolled, not a `DockArea`.** It was one until this pass;
fixed columns with their own min/max, per-corner radii, directional
elevation and 4px handles can't be expressed through the toolkit's dock, so
that dependency went away — and with it, dragging panels to rearrange them
and the persisted dock layout. Re-adding rearrangeable docks is a real
feature to spec, not a refactor to sneak in.

## 4. Contrast: two bars, two jobs

Text and *functional* elements meet WCAG 2.1 AA; decorative seams
deliberately don't, and that's not a bug.

Measured on the shipped tokens (`tokens::tests` asserts these, so they can't
silently regress):

| Pair | Ratio | Bar |
|---|---|---|
| `text_primary` on bg-0/1/2/3 | 14.65 / 13.76 / 12.79 / 11.50 | AA text (4.5) ✓ |
| `text_secondary` on bg-0/1/2/3 | 6.40 / 6.02 / 5.59 / 5.03 | AA text ✓ (floor is 3.0 for UI text) |
| `accent` on `accent_soft_bg` over bg-0 / bg-1 | 4.96 / 4.62 | AA text ✓ — the active-tab label case |
| `text_error` on bg-1 | 6.07 | AA text ✓ |
| `text_disabled` on bg-1 | 2.46 | exempt: WCAG excludes disabled controls |
| `border_mid` seam on bg-1 | 1.35 | decorative — see below |

A panel seam or a row divider is **not** a "user interface component" under
WCAG 1.4.11 — it separates two static regions, and every serious dark IDE
draws it this quiet. An *input's* boundary is a different question: it's
identified by its `bg_3` fill against the panel (1.20:1), its `border_mid`
outline (1.35:1), and the accent border at focus (5.48:1) stacked together,
not by any one of them. **Don't "fix" the seam contrast**; do keep the fill
step and the focus accent, which is what actually carries identification.

## 5. Icons: two kits, no overlap

- **[`ui_icons`](crates/rbx_studio/src/ui_icons.rs)** (`assets/icons/ui/`) —
  *affordances*: tools, commands, menu entries, chevrons. 24x24 canvas,
  outline, `fill="none"`, `stroke="currentColor"`, round caps and joins.
  They must be `currentColor` line art because every state matrix below
  re-tints them; a colour baked into the file would survive hover, disabled
  and active alike.
- **[`class_icons`](crates/rbx_studio/src/class_icons.rs)**
  (`assets/icons/default/`) — *identity*: what a `Part`, `Folder` or
  `Script` is. Flat, multi-colour, never re-tinted, because the colour *is*
  the identity. Explorer rows only.

A ribbon tile that inserts a `Part` uses the **UI** kit: the tile is an
affordance that happens to insert a Part, and it has to go grey when
disabled — which a class icon can't do.

Stroke widths are authored so the rendered stroke lands where it should once
GPUI scales the asset: `2.0` on the 24-unit canvas renders as 1.5px at the
kit's dominant 18px size; chevrons use `3.0` to land at 1.25px when drawn at
10px. Adding an icon means adding the file first — `ui_icons`' tests fail on
a name with no file, on unparseable SVG, and on any hardcoded colour.

## 6. Interaction state matrices

Every interactive element implements the same five-state vocabulary. Reuse
the existing builders (`chrome::tab`, `chrome::icon_button`, `ribbon::tile`,
`toolbar::tool_button`, `menu::item`) rather than writing a sixth variant.

| State | Treatment |
|---|---|
| Default | transparent bg, `text_secondary` |
| Hover | `bg_2`, `text_primary` |
| Pressed | `bg_3` |
| Active/committed | `accent_soft_bg` + `accent` (tabs also switch to `RADIUS_PILL` and semibold) |
| Disabled | `text_disabled`, `cursor_not_allowed`, no hover/press, not clickable |
| Focus | `focus_ring(surface)` — a 2px gap of the surface colour, then the accent glow |

Two rules worth stating outright:

- **Shape carries the committed state, not just colour.** An active tab is a
  pill; a hovered one is a rounded rectangle. Someone who reads shape faster
  than tint still sees which tab is open.
- **Disabled beats absent.** A command Studio has that this editor doesn't
  yet stays visible and greyed, with a tooltip saying so — the same rule
  `menu_bar`'s Cut/Copy/Paste already follow. Seeing the shape of what
  belongs somewhere is worth more than a shorter ribbon.

## 7. The ribbon

Read [`shell/ribbon.rs`](crates/rbx_studio/src/shell/ribbon.rs)'s module doc
before changing a button. In short:

- **Two button shapes, deliberately.** Tools are 32x32 icon buttons packed
  tight — modes you flip between constantly and know by shape, where a label
  would be noise. Everything else is a 56x64 tile (icon over label) — the
  word is what you scan for on a command you reach for occasionally.
- **Three pages** (Home / Model / Test), because seven groups don't fit the
  centre column at an ordinary width. A real ribbon pages; it doesn't shrink
  until nothing is legible. **If a page needs a scrollbar to be fully seen,
  it's carrying too many groups — split it.** Real Studio's further tabs
  (Avatar/UI/Script/Plugins) aren't built: there's no distinct content for
  them, and an empty page is worse than an absent one.
- **This project's own additions don't go in the ribbon.** The graphics
  quality dropdown lives beside the document tabs, because it belongs to the
  open document, not under a Studio-named group.

## 8. Menus and popovers

[`shell/menu.rs`](crates/rbx_studio/src/shell/menu.rs) owns the dropdown:
container `RADIUS_LG` against item `RADIUS_SM` so items visibly float
inside, 8px padding, 32px rows, 2px between them, `elevation_2`.

Menus are **controlled** — which one is open lives on `Shell::open_menu`,
not inside the popover. That's what lets an item close its own menu, and
guarantees two can't be open at once. The toolkit's `Popover` is still doing
the hard parts underneath: anchoring, outside-click dismissal, layering.

## 9. Motion

GPUI has **no property transitions** and **no element transform**. Hover and
press states therefore change instantly, and there is no `scale()` press
feedback anywhere — `bg_3` does that job. What *is* available is one-shot
`with_animation`, which takes a custom easing curve: menus and popovers fade
in over `DURATION_MENU` on `easing_soft` with a 4px settle.

Don't try to fake transitions with per-frame state. An instant hover is
honest; a hand-rolled 220ms colour lerp driven by notifications is a
performance bug waiting to happen next to a viewport rendering at 75fps.

## 10. Known deviations from the design spec

Every one of these is a GPUI or toolkit limit, not a shortcut:

| Spec | What shipped | Why |
|---|---|---|
| `scale(0.98)` press feedback | `bg_3` pressed background | GPUI's style system has no transform |
| 120/220ms hover & colour transitions | instant state changes | no property transitions in GPUI |
| `:focus-visible` (keyboard only) | focus ring on any focus | GPUI's focus doesn't distinguish input source |
| Focus ring on *every* interactive element | Row A/B tabs, ribbon tiles, tool buttons, panel icon buttons, and every toolkit control (theme `ring`) | menu rows and tree rows have no focus handle — keyboard nav inside a menu is the toolkit's job and this menu is hand-built |
| Keyboard arrow-nav within menus | not implemented | chose §5.4/5.5's exact geometry over the stock `PopupMenu` that has nav; the trade is recorded here rather than hidden |
| Tooltip 4px triangle pointer, 500ms delay | toolkit tooltip (no pointer, toolkit's own delay) | hover timing, flipping and layering are already solved there; a hand-rolled overlay would re-solve them worse |
| Letter-spacing (0.04em on section headers) | uppercase only | GPUI has no letter-spacing |
| Inter / JetBrains Mono | used **if installed**; else the platform UI font | the theme takes one family name, not a CSS stack, so `install_fonts` checks what the text system actually has. Neither is installed on the dev machine — it renders in Noto Sans |
| Disabled items at 40% opacity | `text_disabled` at full opacity | `text_disabled` is already the dim end of the ramp; 40% on top lands at 1.3:1, which is invisible rather than unavailable |
| §1.3 unsaved-document dot | not implemented | the editor has no dirty-state tracking to bind it to |
| §5.1 spinner geometry (16px, hover-revealed) | toolkit `NumberInput`'s own spinners | close, but component-owned |
| §5.5 100ms selection flash | flash lasts the press | tying it to the press means it can't outlive the menu it confirms |
| §3.2 guides as one absolute overlay | drawn per row | the tree is virtualised; rows are the only thing that exists to hang a line on |

## 11. Verifying a visual change

`agents/AGENTS.md`'s rule stands: build `rbxstudio`, look at a screenshot.
Specifics for this kind of change:

- **Screenshot every state in the matrix you touched**, not just the resting
  one — hover, pressed, active and disabled are where the work is.
- **Drive the app for real.** Menus, popovers, resize handles and the Output
  collapse can't be checked from a resting screenshot; synthesise the
  clicks (X11 `XTest` works — see the project's own screenshot recipe).
- **A before/after pair beats a single screenshot** for anything touching
  the palette. It's the only way a reviewer can tell "softer" from "just
  different".
- **Run the token tests** (`cargo test -p rbx_studio tokens::`) after
  touching a colour: contrast is asserted, not eyeballed.
