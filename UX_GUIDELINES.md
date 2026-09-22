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
literal `rgb(0x…)` or `px(13.)` in chrome is a bug — it drifts the moment
the design moves, and a literal size also escapes the UI scale, which is
how this app meets WCAG 1.4.4. If no token fits, add one there, named for
what the design calls it.

## 1. Where the design comes from

Two sources, and they answer different questions.

**Geometry comes from the project's own Figma file** — frame
`RbxNative - Studio App`
([`pezVJOZ5zSmIfZ3r0fWzOP`](https://www.figma.com/design/pezVJOZ5zSmIfZ3r0fWzOP/RBX-NATIVE),
node `1:2`). Every number in `tokens.rs` was measured off that frame, not
chosen here.

That has a consequence worth stating plainly: **the frame wins arguments.**
If the editor and the frame disagree, the editor is wrong — unless the
disagreement is recorded in §10 as something GPUI cannot express. When the
frame is silent (it has no focus states, no hover, no open menu, no
scrollbars), this file decides, and says so.

plus its `InputsStyle` frame (node `6:2`), which is where every field,
select, stepper and checkbox gets its height, padding, radius and fill.

The frame is deliberately incomplete in places — it names six ribbon
categories and fills one, and its right dock is empty. Filling those gaps
means extending the frame's own vocabulary (its surfaces, its 3px radius),
never inventing a second one.

**Accessibility floors come from WCAG 2.1/2.2 and the WAI-ARIA APG**, via
this project's compiled reference (`UIANDUX.md`, kept outside the repo).
Where the two sources disagree, the floor wins and the deviation is recorded
in §11.

### The numbers, and where each comes from

| Thing | Floor | Criterion |
|---|---|---|
| Body text vs its surface | **4.5:1** | 1.4.3 Contrast (Minimum), AA |
| Large text (≥24px, or ≥18.66px bold) | 3:1 | 1.4.3, AA |
| Control edges, focus ring, state fills, tool accents | **3:1** | 1.4.11 Non-text Contrast, AA |
| Focus indicator | **solid 2px**, outset, 3:1 against *both* the control and its background | 2.4.13 Focus Appearance |
| Pointer target | **24×24** (44×44 with Large Click Targets on) | 2.5.8 Target Size (Minimum), AA / 2.5.5, AAA |
| Text resize | reaches **200%** | 1.4.4 Resize Text, AA |

Two things about that table are easy to get wrong, and the reference calls
them out specifically:

- **2.4.13 is AAA, not AA.** The only AA focus rule is 2.4.7 Focus Visible,
  which sets no numbers at all; 2.4.11 Focus Not Obscured is the other AA
  one. Plenty of secondary sources mislabel the measurable ≥2px/3:1 spec as
  "2.4.11". This project takes the AAA number anyway, because it is cheap to
  hit and "visible" without one is an opinion.
- **These are CSS reference pixels, not device pixels.** They map to GPUI's
  logical pixels — which is exactly why every size in `tokens.rs` is a
  function of the UI scale rather than a constant.

### How a native app is held to a web standard

WCAG is written for web content. Applying it to desktop software is accepted
practice and required of software by EN 301 549, but two criteria have no
literal native equivalent, so this is the interpretation — stated here
because the reference asks for it to be documented rather than assumed:

- **1.4.4 Resize Text (200%)** has no browser zoom to lean on. The
  settings-based UI scale *is* the mechanism: 0.5×–2.0×, persisted, over
  fonts and the boxes they sit in together. Blender's Resolution Scale model
  and range, for Blender's reason — text that grows while its row does not
  is a clipped UI, not a larger one.
- **1.4.10 Reflow (320px)** is not met literally and is not meant to be. The
  criterion explicitly exempts "interfaces where it is necessary to keep
  toolbars in view while manipulating content", which is this editor
  exactly. What stands in for it is that nothing is *lost* as things grow:
  the ribbon scrolls rather than dropping buttons, and a dock is capped at a
  share of the window so it cannot squeeze the viewport out.

### Conformance, honestly

The reference's Stage 1 is the non-negotiable AA baseline; Stage 2 is the
native-app accommodations; Stage 3 is polish. This is where the editor
actually stands — including where it does not.

**Stage 1 — all six met.** Text contrast, non-text contrast, the focus
indicator, target sizes, never-colour-alone, and the keyboard model. The
first four are asserted in `tokens::tests` rather than eyeballed, and the
target-size test runs at **every** scale, not just at 1.0× where none of
those tokens can fail.

**Stage 2 — met, with one gap.** The UI scale (item 7) is there; the
reference's separate editor/viewport font size is not, and is not missed
yet. Large Click Targets and a Reduce Motion toggle (item 8) live in the
View menu, the latter overriding a desktop preference read at startup.
The whole dock arrangement persists — which edge, dock and tab each panel
sits in, every size, what is floating and what is closed — with a Reset
Layout command beside it (item 9); named layouts do not exist.

**Stage 3 — not started.** No high-contrast theme (item 10), no command
palette (item 12). Item 11's 44×44 is reachable through Large Click
Targets but is not the default on primary controls.

**No Level A gap left.** `Select`, `ColorPicker` and `NumberInput` are in
the Tab order — every `Color3`, every enum and the snap increments
included. None of them needed the focusable wrapper this section used to
promise: each one's state entity (`SelectState`, `ColorPickerState`,
`InputState`) already implements `Focusable` and hands out the same handle
its own `.focus` uses, so the window's own order (`shell::roving::TabOrder`)
records that handle directly. The menu bar, which was the last
mouse-only control, takes the usual desktop answer rather than a Tab stop:
**F10** or a bare **Alt** tap enters it, Left/Right walk the titles,
Enter/Space/Down open one, and Escape backs out to wherever focus came from
(`menu_bar`). That closes WCAG 2.1.1 Keyboard, Level A.

## 2. Tokens

| Group | Tokens | Notes |
|---|---|---|
| Surfaces | `black` `#000000` · `dock` `#0D0D0D` · `chrome` `#171717` · `field_select` `#232323` · `tile` `#2F2F2F` · `menu_bar` `#1B1B1B` | ground → a dock's body → raised panel and every text field → a **dropdown** → a ribbon button. **Neutral** grey, deliberately: a cool cast on a tool whose job is showing somebody else's colours biases every material judged against it. Each step is **asserted** as a luminance delta, not a contrast ratio: this near black, a ratio is dominated by WCAG's `+0.05` term and scores two surfaces 20 levels apart about the same as two 4 apart |
| States | `tab_active` (black 30%) · `ribbon_tab_active` (white 10%) · `hover` (white 8%) · `selection` (blue 35%) | the open document is *recessed*; everything else lights up |
| Borders | `tab_border` `#333333` · `divider` `#3D3D3D` | between document tabs; between ribbon groups, along a dock's edge, and before its trailing cell |
| Text | `text_full` → `text_disabled`, six steps of white alpha, plus `text_error` | alphas, not greys: changing a surface re-tints every label on it. There is no step below `text_disabled` — the one there used to be rendered read-only *values* at 1.7:1 |
| Colour | `check_on` `#4A90D9` + `check_on_border`, `check_off` `#222222` + `check_off_border` | the **only** saturated hue in the design. It is the checkbox. Focus and selection borrow it; nothing else may |
| Radius | `RADIUS` 3 · `RADIUS_TINY` 1 | that is the whole scale. A third radius is a bug |
| Controls | `check_on` `#3AA0FF` · `check_off` + `check_off_border` · `tab_active_bar` · `select_inset` | the accent, the outline the frame's borderless checkbox needed, and the one compensation this file admits to (see §11) |
| Dimensions | `topbar_height()` 34 · `menu_bar_height()` 30 · `tabs_height()` 42 · `ribbon_tabs_height()` 28 · `ribbon_height()` 80 · `dock_tabs_height()` 38 · `dock_width()` 300 · `dock_height()` 178 (the strip and five rows) · `row_height()` 28 · `row_label_width()` 120 · `input_height()` 31 · `checkbox_size()` 15 in a `checkbox_target()` of 26 · … | **functions**, all scaled. Larger than the frame's own numbers because the type is (see below) |
| Type | `text_md()` 14 · `text_sm()` 13 · `text_xs()` 11, with `line_md()`/`line_sm()`/`line_xs()` | 14 is the default; 13 is section headers and tooltips; 11 is a ribbon tile's label. The frame's own 9/8 is overruled — see §11 |
| Toolkit | `FIELD_SIZE` | hand this to any `Input`/`Select`/`ColorPicker` so its text comes out at `TEXT_SM` — the toolkit's own smallest step is 12px |
| Tool accents | `tool_select` `tool_move` `tool_scale` `tool_rotate` `tool_align` `tool_local`, + `tool_wash` and `TOOL_BORDER` | the **only** place a second hue is spent, and only on the active tool's own button. See §4 |
| Scale | `font_scale` · `set_font_scale` · `FONT_SCALE_RANGE` | every size token multiplies by this. See §3 |
| Behaviour | `focus_ring(surface)` · `elevation()` · `reduced_motion` · `DURATION_MENU` · `easing_soft` | the things a static frame cannot contain. See §6, §8, §9 |

Chrome spacing has no scale and no tokens: the frame uses 4, 5, 6, 7, 10
and 16px in different places for different reasons, and rounding those to a
base-4 ladder would be inventing a design the file doesn't have. Write the
literal the frame measures. The Properties panel is the exception — its
four gaps (`label_gap` < `row_gap` < `section_gap` < `group_gap`) are a
*ratio*, not four independent numbers, and §6 explains why. `header_gap`
sits outside that ladder on purpose: a collapsed category header carries
its own fill, so consecutive headers are tiles in a stack and are spaced
like tiles, not like groups — the grouping is read off the surface there,
not off the void.

**Every size is a function, not a constant**, and multiplies by
`font_scale()`. Anything a pointer has to hit goes through `scaled_target`
instead, which is the same thing with WCAG 2.5.8's 24px floor under it —
`checkbox_target`, `input_height`, `row_height`, `tree_row_height`,
`hit_target`. `tool_border()` scales too, because it is an accessibility
cue and a fixed 1.5px would get relatively thinner exactly when someone has
asked for everything to be bigger. Blender's model: one dial over fonts *and* the boxes they
sit in, because text that grows while its row doesn't is not a larger UI,
it is a clipped one. The range is Blender's too (0.5×–2.0×), reachable with
Ctrl+= / Ctrl+− / Ctrl+0 and persisted in `Settings`. `hit_target()` is the
one exception: it is a WCAG floor rather than a design value, so it never
shrinks below 24 even at 0.5× — and **View → Large Click Targets** raises
that floor to 2.5.5's 44px for pen, touch and motor-impaired use, which is
Blender's "editor-area padding" idea under a clearer name.

`tokens::tests` asserts the palette rather than trusting it — see §4.

## 3. Structure

```
Shell (v_flex)
├─ Topbar          h 31   black       logo · centred title · window buttons  ← the real title bar
├─ MenuBar         h 24   menu-bar    File / Edit / Model / View
├─ ROW A  Tabs     h 42   chrome      w180 document tabs (accent rule on the open one) + a disabled "+"
├─ ROW B  RibbonTabs h 28 dock        Home · Avatar · UI · Script · Model · Test · Plugins
├─ ROW C  Ribbon   h 80   chrome      p5 gap10, groups for Row B's active tab, scrolls at high UI scale
├─ ROW D  Workspace flex-1  black
│   ├─ Properties   w 300 (persisted)  DockTabs + p5 content, on `dock`
│   ├─ ⇔ handle     4px hit on the dock's own surface — no black slot
│   ├─ Centre       flex-1: document, nothing floating over it, over the bottom dock
│   │   └─ Output │ Viewport   tabs; h `dock_height()` (80–400), or Output's strip alone when collapsed
│   ├─ ⇔ handle
│   └─ Explorer     w 300 (persisted)  DockTabs + p5 content, on `dock`
└─ CommandBar      auto    black      a dock's own 5px inset and chrome field
```

**The title bar is ours.** `main::window_options` asks for
`WindowDecorations::Client`, so there is no window-manager title bar above
`Topbar` — that row *is* it. Dragging its middle stretch moves the window;
double-clicking maximizes; the three buttons on the right are the only
minimize/maximize/close there are. The window title still goes through
`TitlebarOptions` so a taskbar, an alt-tab switcher and a screenshot tool
can name the window. `gpui_component`'s `Root` draws the resize edges and
the window's own rounded corners underneath.

**The independence rule.** Explorer, Properties and Output are built once in
`Shell::new` and rendered unconditionally as Row D children. Row A swaps
*only* the centre column's contents; Row B swaps *only* Row C's groups.
Neither can unmount a panel, so scroll positions and selections have nowhere
to get lost. Keep it that way: if a panel ever becomes conditional on a tab,
that rule is broken and the bug it causes (a reset scroll, a lost selection)
will look like a mystery rather than a layout change.

**Row D is hand-rolled, not a `DockArea`.** Fixed columns with their own
min/max and 4px handles can't be expressed through the toolkit's dock, so
that dependency went away — and with it, dragging panels to rearrange them.
Re-adding that is a real feature to spec, not a refactor to sneak in; the
design is in [`agents/dock-rearrangement.md`](agents/dock-rearrangement.md).

**Dock sizes persist, and there is a way back.** Column widths, the Output
dock's height and whether it is collapsed all land in `Settings` on release
— not on every frame of a drag, which would write the file sixty times a
second to record states nobody chose. **View → Reset Layout** is the
companion every persisted layout needs: a column dragged to four pixels
wide is saved that way, and without a reset the only way out is to find and
delete the settings file.

**A dock is a tab strip over an inset body.** `chrome::dock_tabs` draws the
strip: one `chrome` pill hugging its own title on the dock's black ground,
then — only when there is something to put there — a cell divided off by a
`divider` hairline. The frame puts a "+" in that cell. This editor's docks
hold one panel each and always will, so the cell carries the panel's
overflow menu instead: same geometry, a button with somewhere to go. Same
reasoning kills the tab's close "×" — the frame draws one, and here it
would be a control that lies.

## 4. Contrast: what is asserted, and what is exempt

The design's text colours are white alphas over near-black surfaces, which
is exactly the setup where "looks fine to me" goes wrong. So it isn't
eyeballed: `tokens::tests` composites every meaningful text token over every
surface it can land on and fails below WCAG AA 4.5:1.

| Pair | Bar |
|---|---|
| `text_full`/`strong`/`label`/`muted`/`placeholder` on `black`, `chrome`, `tile`, `menu_bar` | AA text (4.5) ✓ — asserted |
| `text_disabled` on `chrome` | between 2.0 and 4.5 — asserted from *both* sides: dim enough to read as unavailable, not so dim it looks like a rendering fault. WCAG exempts disabled controls |
| `check_on` vs `check_off` | ≥ 3:1 non-text — asserted |
| `tab_active` composited vs `chrome` | asserted *darker*: the open document is recessed. If this ever inverts the whole tab strip reads upside down |
| `check_on` (the focus ring) vs every surface | ≥ 3:1 — asserted. A 2px ring that nobody can see is WCAG 2.4.13's actual failure mode, not a missing ring |
| every tool accent vs `tile` | ≥ 3:1 — asserted, all six |
| `check_off_border` vs `black` *and* `chrome` | ≥ 3:1 on both — asserted |
| the smallest targets (icon button, checkbox, input, rows) | ≥ 24×24 — asserted |

`assets/themes/dark-soft.json` paints everything the toolkit owns — the menu
bar, inputs, buttons, scrollbars — and is a second copy of this palette,
which is exactly the arrangement that drifts. So it is asserted key by key
against the tokens too. **Change a colour in one place and the test tells
you about the other.**

The frame sets body text at **9px**, which is unreadable at arm's length
and fails the intent of WCAG 1.4.4 before the scale is even touched. It is
the one place the design is deliberately overruled: the ramp is the frame's
*hierarchy* at a legible base size (14 / 13 / 11), and §10 records it.

## 5. Icons: two kits, no overlap

- **Lucide** — *affordances*: tools, commands, menu entries, chevrons,
  window buttons. `gpui_kit::assets::IconName` is the full catalog, already
  registered through `AllAssets`, and it is what the design itself is drawn
  with. Use the enum, never a path string: a typo is then a compile error
  rather than an icon that silently renders as nothing.
- **[`class_icons`](crates/rbx_studio/src/class_icons.rs)**
  (`assets/icons/default/`) — *identity*: what a `Part`, `Folder` or
  `Script` is. Flat, multi-colour, never re-tinted, because the colour *is*
  the identity. **Explorer rows only**, by explicit design decision.
- **[`assets/icons/brand/`](assets/icons/brand)** — the logo, exported from
  the same Figma file. GPUI paints an SVG as a mask, so it renders as a
  white silhouette; that is what the frame shows anyway.

A ribbon tile that inserts a `Part` uses **Lucide**: the tile is an
affordance that happens to insert a Part, and it has to go grey when
disabled — which a class icon can't do.

Icon sizes are the frame's and they are small: 20px on a ribbon tile, 12px
in a document tab and a menu row, 10px on a stack row and a checkbox tick,
8px on a section chevron, 7px on a tab's close mark.

## 6. Interaction state matrices

The frame has no states — it is one static picture — so this is the part
this file decides. Every interactive element implements the same vocabulary.
Reuse the existing builders (`chrome::icon_button`, `chrome::dock_tabs`,
`ribbon::tile`, `ribbon::stack_row`, `rows::checkbox`, `menu::item`) rather
than writing a sixth variant.

| State | Treatment |
|---|---|
| Default | transparent bg, `text_label` (or `text_strong` where the frame is brighter) |
| Hover | `hover` (white 8%), `text_full` |
| Pressed | `ribbon_tab_active` (white 10%) |
| Active/committed | `ribbon_tab_active` for a ribbon tab or tool; `tab_active` for the open document; `selection` for a selected row |
| Disabled | `text_disabled`, `cursor_not_allowed`, no hover/press, not clickable, tooltip saying why |
| Focus | `focus_ring(surface)` through GPUI's `focus_visible` — a 2px blue ring, one pixel clear of the control's own edge |

Three rules worth stating outright:

- **Focused is not selected.** A selected ribbon tab, active tool or
  highlighted tree row is a *background* — a wash, a pill, a tint. Focus is
  an *outline*, and only ever an outline. Rendering either as the other is
  what made every mouse click in the previous pass leave a hard blue
  rectangle behind, and it is why an active tool and a focused one are
  distinguishable at a glance.
- **Always `focus_visible`, never `focus`.** GPUI's `focus_visible` is its
  `:focus-visible` equivalent: it already tracks input modality and
  refreshes the window when it flips, so a mouse click never paints a ring.
  Tracking modality separately would be a second source of truth for the
  same question.
- **The order of a shadow list is load-bearing.** GPUI paints shadows
  front-to-back in array order, so `focus_ring` puts the blue step *first*
  and the surface step second. Reverse them and an unfilled control — a
  ribbon tab, a section header — comes back flooded solid blue instead of
  outlined. There is a comment on the token saying so; keep it.
- **Disabled beats absent.** A command Studio has that this editor doesn't
  yet stays visible and greyed, with a tooltip saying so — the same rule
  `menu_bar`'s Cut/Copy/Paste already follow. Seeing the shape of what
  belongs somewhere is worth more than a shorter ribbon, and the frame's own
  row of "Placeholder" tiles says the designer agrees.

## 7. The ribbon

Read [`shell/ribbon.rs`](crates/rbx_studio/src/shell/ribbon.rs)'s module doc
before changing a button. In short:

- **Two shapes, both the frame's.** A **tile** is 42px wide, icon over
  label, full ribbon height — for commands worth aiming at. A **stack** is
  78px wide and holds three rows of icon-beside-label — for commands that
  belong to the tile beside them (Copy's paste/cut/duplicate) or that are
  really a readout (the snap increments).
- **Seven pages**, six of them the frame's names plus `Test`, which the
  frame has no tab for and this editor has real content for. Pages exist
  because all the groups at once don't fit the centre column at an ordinary
  width; a real ribbon pages, it doesn't shrink until nothing is legible.
  **If a page needs a scrollbar to be fully seen, it's carrying too many
  groups — split it.**
- **Groups are separated by a hairline, never by a caption.** One `divider`
  between groups, none at either end.
- **A readout is a control.** The snap stack shows what a drag will round to
  *and* opens the fields that set it. Increments are checked far more often
  than they are changed, and a number you have to open a popover to see is a
  number nobody trusts.
- **This project's own additions don't go in the ribbon.** The graphics
  quality dropdown, the viewport's settings and its live frame rate live in
  the **Viewport dock** — a tab beside Output by default — rather than under
  a Studio-named group. They used to float over the viewport's corner, and
  nothing persistent sits over the scene being edited any more: only the
  transient readouts a gesture produces (the flight speed, a drag's studs).
  The ribbon's Home tab carries the dock's open/close tile like every other
  dock's, and the dock samples the frame rate only while it is on screen.

## 8. Keyboard

Read [`shell/roving.rs`](crates/rbx_studio/src/shell/roving.rs) and
[`shell/tree_keys.rs`](crates/rbx_studio/src/shell/tree_keys.rs) before
touching any of this. The short version:

**Tab moves between regions; arrows move within one.** A composite widget —
the ribbon, a tab strip — is *one* Tab stop. Without that, reaching the
Explorer from the menu bar means Tab-ing past every ribbon button on the
current page, and people stop using the keyboard.

**Every tab stop has a unique index, handed out in paint order** by
`roving::TabOrder`, which `Shell::render` restarts each frame. Paint order
is reading order, which is the order WCAG 2.4.3 asks for. Two GPUI
limitations force this shape, both established by driving the real window:

- **`tab_group()` swallows its children.** An element inside one registers
  no tab stop at all. A dock wrapped in a group is simply unreachable.
- **Two stops sharing a `tab_index` do not advance.** Tab reaches the first
  and stays there. Three window buttons at index 0 were a keyboard trap in
  the one place nobody would look for one.

So: never reuse an index, never use `tab_group`, and take every index from
`TabOrder::next()`.

**Roving groups own a focus handle per item.** The current item carries the
group's index and `tab_stop(true)`; every other item is `tab_stop(false)` —
GPUI has no `tabindex="-1"`, and a negative index merely sorts earlier.
Both go on the **`FocusHandle`**, not the element: `InteractiveElement`'s
own `tab_index`/`tab_stop` are silently ignored once `track_focus` is in
play, which costs an afternoon to discover.

**Activation is free and must stay that way.** GPUI already turns Enter and
Space on a focused element into a real `on_click`, so a keyboard activation
is the same code path as a mouse click. Never write a second one.

**The Explorer implements the APG Tree View contract in full** — Right
expands then descends, Left collapses then climbs, Home/End, type-ahead,
and no wrapping at the ends. The toolkit's own bindings get three of those
wrong, so `tree_keys` intercepts in the *capture* phase and hands back only
the two cases the toolkit is right about (expanding a closed node,
collapsing an open one), which are also the two that need its private
`toggle_expand`. Partial tree keyboard support is itself an accessibility
failure — the APG says so outright — so if you add a key here, add all of
its branches.

**No keyboard traps** (WCAG 2.1.2). Escape closes any open menu from
anywhere. Tab and Shift+Tab are handled at the window root in the capture
phase, because a focused text input consumes Tab first — focus starts in
the Command Bar, and without this it could never leave.

**Buttons are `chrome::button`, never the toolkit's.** A filled surface,
the one radius, no border — the same language the fields speak. The
toolkit's own `Button` draws a 1px-outlined pill, which next to a row of
borderless filled fields looks like it came from a different application.

**A property category's header is bold and the brightest step in the ramp.**
On a neutral palette, weight and brightness are the only two axes left to
separate a heading from the properties under it — the one saturated colour
in the design is spent on the accent and stays there.

**A numeric field's label is a drag handle.** Horizontal drag scrubs the
value; Shift coarsens, Alt refines. It lives on the label, not the field,
so a click on the field still means "type here" — and it writes through the
field's own input rather than to the DOM, so a drag and a typed value take
one path, not two that can disagree.

**A field knows what kind of number it holds.** `properties::FieldKind`
is per *field*, never per property, because the DOM's own types are mixed
inside a single row: a `UDim` is an `f32` scale beside an `i32` offset. It
decides the drag step and it is what stops an integer field being handed a
fraction.

## 9. Menus and popovers

[`shell/menu.rs`](crates/rbx_studio/src/shell/menu.rs) owns the dropdown:
`chrome` surface, `RADIUS`, 4px padding, 24px rows, 9px labels, and
`elevation()` — which on a UI this dark is carried by its hairline, not by
its blur.

Menus are **controlled** — which one is open lives on `Shell::open_menu`,
not inside the popover. That's what lets an item close its own menu, and
guarantees two can't be open at once. The toolkit's `Popover` is still doing
the hard parts underneath: anchoring, outside-click dismissal, layering.

## 10. Motion

GPUI has **no property transitions** and **no element transform** (only
`svg()` can be transformed, not a `Div`). Hover and press states therefore
change instantly, and there is no `scale()` press feedback anywhere — the
pressed background does that job. A ribbon icon's hover lift is an instant
size change on its own fixed box, so the label beneath it doesn't move.
What *is* available is one-shot `with_animation`: menus fade in over
`DURATION_MENU` on `easing_soft` with a 4px settle.

**Reduce motion is honoured** (WCAG 2.3.3), from two places. GPUI owns the
flag and `with_animation` already respects it, but no backend ever sets it
— so `scale::install` asks the desktop once at startup (GSettings'
`enable-animations`, overridable with `RBX_STUDIO_REDUCE_MOTION`) and sets
both GPUI's copy and `tokens`' mirror, which is what a styling callback can
reach. **View → Reduce Motion** then overrides that and persists, because a
desktop setting is a sensible default and not a verdict: somebody may want
this editor calm on a machine that animates everything else, and an
accessibility preference reachable only through an environment variable is
not a setting anyone has.

Don't try to fake transitions with per-frame state. An instant hover is
honest; a hand-rolled colour lerp driven by notifications is a performance
bug waiting to happen next to a viewport rendering at 75fps.

## 11. Known deviations from the frame

Every one of these is a GPUI or toolkit limit, or a control that would have
to lie — not a shortcut:

| Frame | What shipped | Why |
|---|---|---|
| 9px body text | 14 / 13 / 11 ramp | **overruled.** 9px fails the intent of WCAG 1.4.4 before the UI scale is touched. The frame's *hierarchy* is kept; only the base size moved |
| Black docks on a black ground | `dock` `#12151B`, a step off the ground, with a hairline where it meets the viewport | **overruled on review.** Painted as drawn, the three docks and the window behind them read as one field; you could not see where the Explorer ended |
| Black ribbon category strip | `dock` | same reason: a black band between two lighter ones read as a gap, not a strip |
| A 26px checkbox | 15px box inside a 26px target | the frame's box was the loudest thing in a property row. The *target* keeps the frame's number, which is also WCAG 2.5.8's floor |
| Right-aligned property labels | left-aligned | tried first, for the tighter label-to-field binding the reference prefers in dense inspectors; the ragged left edge made a long property list harder to scan, which is what this panel is mostly used for |
| A disc behind each Explorer chevron | removed | it existed so a guide line could pass behind rather than through; a filled circle on every folder row was too much noise for one hairline |
| A flat cube-face orientation indicator | a translucent sphere with the three axes' orbital rings through it | the frame's own newer artwork, and the flat version read as a smudge |
| A dropdown and a text field on the same surface | dropdowns get `field_select`, one step up | they do different things — one opens, one takes typing — and were told apart only by noticing the chevron |
| A select sitting level in its field box | `select_inset`, a measured 4.5px top pad | the toolkit's select trigger top-aligns its row inside whatever height it is given, and its text sits ~1.5px above that row's own centre. Measured against a plain text field in the same panel; re-measure if the toolkit is upgraded |
| A borderless unticked checkbox | outlined at `check_off_border` | **overruled.** `#111` on a black dock is 1.11:1 — a WCAG 1.4.11 failure for a control whose only job is to show a state |
| 10px checkbox, 20px icon buttons | 26px and 24px | WCAG 2.5.8's 24×24 target floor. 26 is the `InputsStyle` frame's own number |
| 36px inputs, 8px radius, 1px border (the v5 fallback spec) | 31px, 3px radius, no border | the `InputsStyle` frame gives real numbers, and §7.1 says the frame wins over the fallback |
| Select chevron 12×12 at 12px inset | the toolkit's own, 15×15 at 8px | the frame's numbers, drawn by `Select` itself; `appearance(false)` removes its box but keeps its chevron |
| 120ms 1.06× icon hover animation | instant size change | GPUI has no property transitions and cannot transform a `Div` |
| One dock per edge, fixed | an edge holds a stack of docks, each with tabs, all of it draggable by the tab | the frame draws one arrangement and says nothing about changing it, so §1's rule applies and this file decides. A drop offers both readings — a dock's strip joins it, a dock's half splits the edge — because a single whole-dock target cannot ask which one you meant |
| No drop affordance at all | the target *is* the element that takes the drop, and an empty edge grows a ghost dock at the size the dock will be | a rectangle computed from pointer coordinates is a second piece of geometry that can disagree with where the panel actually lands. Making the highlight the landing site removes the disagreement rather than testing for it |
| A torn-out panel | its own floating window, rendering the same `Shell` | the panel is not copied — see `shell::panel_window`. On X11 a window manager may ignore `WindowKind::Floating`, so the editor raises its own children on its rising edge |
| Focus ring on menu rows and tree rows | not present | neither has a focus handle; the tree's belongs to the toolkit and is not exposed |
| Menu bar: File · Edit · View · Plugins · Test · Window · Help | File · Edit · Model · View | the other three have no commands behind them; an empty menu is worse than an absent one, and the menu bar's own greyed items already carry "not yet" |
| Close "×" on document and dock tabs | absent on both | a document here is a view of the one open place, not a file that closes independently, and a dock's panel can't close at all. A mark that does nothing is a control that lies |
| "+" at the end of a tab strip | disabled on the document strip, replaced by the dock's overflow menu on a dock strip | the editor has no notion of a new document, and its docks hold one panel each |
| Fixed 228px docks with no handles | 300px by default, draggable 200–560, persisted, with Reset Layout | a place file's instance names are not 228px wide just because the design's were, and the wider label column follows from 14px text. The handle draws nothing until it is pointed at |
| Half-pixel (0.5px) tab borders | 1px | GPUI draws whole pixels |
| Centred search placeholder | left-aligned | the toolkit `Input` owns its own text alignment |
| Ribbon stack of three rows | two, where the design's third has no command behind it | a third empty row is decoration |
| 0.5px `#2F2F2F` / `#353535` hairlines | same colours at 1px | as above |
| `scale(0.98)` press feedback | pressed background | GPUI's style system has no transform |
| Hover & colour transitions | instant state changes | GPUI has no CSS-style property transitions. `gpui_base::transition` can drive one value explicitly, which is what the ghost dock's ease uses (and it honours Reduce Motion); applying it to every hover state is a different, larger job |
| `:focus-visible` (keyboard only) | focus ring on any focus | GPUI's focus doesn't distinguish input source |
| Tooltip pointer and delay | toolkit tooltip | hover timing, flipping and layering are already solved there |
| Letter-spacing | none | GPUI has no letter-spacing |
| Inter | used **if installed**; else the platform UI font | the theme takes one family name, not a CSS stack, so `install_fonts` checks what the text system actually has. Inter is not installed on the dev machine — it renders in Noto Sans |
| An unsaved-document dot | not implemented | the editor has no dirty-state tracking to bind it to |
| Hierarchy guides as one absolute overlay | drawn per row | the tree is virtualised; rows are the only thing that exists to hang a line on |
| Keyboard focus visually distinct from selection in the Explorer | they are the same row | `TreeState` tracks one `selected_ix` and nothing else; splitting them means replacing the toolkit's tree |
| Every control in the Tab order | every control but the menu bar, which F10 or a bare Alt tap reaches instead | the desktop convention: a region everyone has to Tab through on the way to the ribbon is not what it asks for. `Select`, `ColorPicker` and `NumberInput` are in the order — their state entities are `Focusable`, so `shell::roving::TabOrder` records each handle directly, no wrapper needed. No Level A gap remains (2.1.1 Keyboard) |
| No slider anywhere in the frame | a rail beside the number field, on the bounded properties only | the frame is silent, so §1's rule applies and this file decides. Built from `gpui_base`'s unstyled slider parts rather than the toolkit's finished `Slider`, whose rail, thumb and target are all sized in `rem` and would ignore the UI scale. Skinned as a field box: `chrome` fill, no border, `check_on` for what is set |
| A slider in the Tab order | mouse and assistive-technology only | `SliderState` is not `Focusable`, so `shell::roving::TabOrder` has no handle to record. Not a Level A gap: the rail is a decoration on a row whose number field *is* a Tab stop and takes the same value typed, and `gpui_base` gives the rail `Role::Slider` with working increment/decrement actions |
| A separate editor/viewport font size | one UI scale over everything | the reference notes VS Code splits them; nothing here needs a text size independent of its chrome yet |
| Named dock layouts | one layout, plus Reset | the arrangement persists and can be reset; saving several under names is a feature, not a floor |

## 12. Verifying a visual change

`agents/AGENTS.md`'s rule stands: build `rbxstudio`, look at a screenshot.
Specifics for this kind of change:

- **Screenshot every state in the matrix you touched**, not just the resting
  one — hover, pressed, active and disabled are where the work is.
- **Drive the app for real.** Menus, popovers, ribbon pages, resize handles
  and the Output collapse can't be checked from a resting screenshot;
  synthesise the clicks (X11 `XTest` works — see the project's own
  screenshot recipe).
- **Compare against the frame, not against the last screenshot.** Pull the
  frame render (`get_screenshot` on node `1:2`) and put the two side by
  side. "Different from before" is not the bar; "the same as the frame" is.
- **Run the token tests** (`cargo test -p rbx_studio tokens::`) after
  touching a colour: contrast, target sizes and the theme-file mirror are
  asserted, not eyeballed.
- **Walk the keyboard, don't read it.** Tab from a cold start to the last
  region and back, arrow through every roving group, and exercise the
  tree's full arrow contract. Three of this pass's real bugs — Tab trapped
  in the Command Bar, Tab trapped in the window buttons, and roving indices
  silently ignored — were all invisible in the code and obvious the moment
  the window was actually driven.
- **Check at 0.5× and 2.0× UI scale**, not just 1.0×. A hardcoded `px()`
  looks fine at 1.0× and breaks the layout at either end, which is exactly
  why sizes are functions.
