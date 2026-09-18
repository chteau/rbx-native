# UX & Visual Design Guidelines

Rules for anyone (human or agent) touching `rbx_studio`'s look: palette,
spacing, radius, typography, and the dock/panel layout. Companion to
[GUIDELINES.md](GUIDELINES.md) (Rust style) and [SPECS.md](SPECS.md)
(architecture) — this one is about how the editor should *look and feel*,
not how its code is organized. `agents/AGENTS.md`'s "Verifying a change"
section still governs *how* a visual change gets checked (build the binary,
look at a real screenshot); this file is about what that screenshot should
show.

## 1. Tokens, not literals

`rbx_studio` themes through `gpui_kit::component`'s `Theme`
(`cx.theme()...`), configured at startup by `install_theme` in
`crates/rbx_studio/src/main.rs` from
[`assets/themes/dark-soft.json`](assets/themes/dark-soft.json) — a
`ThemeSet`/`ThemeConfig` JSON file in the toolkit's own format (any key left
out falls back to the toolkit's stock dark theme, so the file only needs to
list what this editor actually overrides).

- **Reach for an existing `cx.theme()` field before writing a hex literal.**
  `background`, `border`, `sidebar.*`, `tab.*`, `muted.*`, `accent.*`,
  `selection`/`list.active.*`, `ring`, `popover.*` are all set in
  `dark-soft.json` and read automatically by every stock `gpui_kit`
  component (`ListItem`, `Button`, `Input`, `DockArea`, tabs, scrollbars,
  resize handles). Changing the palette almost always means editing that one
  JSON file, not chasing call sites.
- **A raw `rgb(0x...)`/`rgba(0x...)` literal in panel/dock/toolbar chrome is
  a bug**, not a style choice — it silently drifts from the theme the next
  time the palette changes. The one deliberate exception is
  `workspace_view.rs`'s viewport HUD (status chip, drag readout, the
  orientation cube) — see §6.
- Custom-drawn chrome that can't go through a stock component (e.g. a tagged
  Explorer row's own hover/selected paint in `shell/rows.rs::tagged_row`)
  should still pull *individual* values from `cx.theme()` (radius, the
  row's own tag colour) rather than inventing new constants.

## 2. Palette

Dark only (`rbxstudio` hardcodes `ThemeMode::Dark`; there is no light theme
to keep in sync). The ramp in `dark-soft.json` is deliberately shallow and
neutral:

| Token | Value | Role |
|---|---|---|
| `background` / `title_bar.background` | `#1a1a1a` | Floor of the ramp — window chrome, menu bar. Never go darker than this. |
| `sidebar.background` | `#1e1e1e` | Explorer/Properties panel body. |
| `tab_bar.background` | `#202020` | Inactive tab strip. |
| `popover.background` | `#242424` | Menus, dropdowns — one step "raised" above the panel it opens from. |
| `muted.background` | `#262626` | Hover/secondary surface. |
| `border` / `sidebar.border` | `#2b2b2b` | Seams — see §3, deliberately low-contrast. |
| `foreground` | `#e8e8e6` | Primary text. Never go lighter than roughly `#f0f0f0`. |
| `muted.foreground` | `#9c9c9a` | Secondary text/labels (Property names, tab labels). |
| `ring` / `selection.background` / `list.active.*` | `#4d8dff` / `#3b82f6` | The one saturated hue — see §4. |

If a redesign needs a darker/lighter floor or ceiling, change these two rows
together (`background` and `foreground`) and re-check every contrast pair in
§3 — don't nudge one without the other.

## 3. Contrast: two different bars for two different jobs

Text and *functional* UI elements are held to WCAG 2.1 AA
([nngroup.com/articles/ten-usability-heuristics](https://www.nngroup.com/articles/ten-usability-heuristics/),
[w3.org/WAI/WCAG21/Understanding/contrast-minimum](https://www.w3.org/WAI/WCAG21/Understanding/contrast-minimum.html)):

- **Text**: ≥ 4.5:1 against its background (≥ 3:1 for large/bold text).
  Every text/background pair in the palette above clears 5.2:1 or better —
  keep it that way when editing colours.
- **Functional non-text elements** — input borders, the focus `ring`,
  button outlines, anything a user has to *locate* to interact with — need
  ≥ 3:1 against their surroundings
  ([w3.org/WAI/WCAG21/Understanding/non-text-contrast](https://www.w3.org/WAI/WCAG21/Understanding/non-text-contrast.html)).
  `ring` (`#4d8dff` on `#1e1e1e`) sits at 5.2:1 — don't soften it to match
  the decorative seams below.

**Decorative panel/row seams are a deliberate exception, not an oversight.**
`border`/`sidebar.border` (`#2b2b2b`) against `sidebar.background`
(`#1e1e1e`) is ~1.2:1 — intentionally close to invisible. This is the
"hard 1px border → soft luminance-step" softening a real IDE dark theme
wants (VS Code, JetBrains, Zed all do this for panel-to-panel and row
dividers); it does not need to, and should not, meet the 3:1 non-text bar,
because it isn't identifying an interactive control — it's just telling two
static regions apart. **Don't "fix" this contrast as an accessibility bug**
without checking which of the two categories above it actually falls into
first.

## 4. Accent discipline

One saturated hue exists in the whole theme (`#3b82f6`/`#4d8dff`, a
moderate blue), reserved for: the selection highlight
(`selection.background`, `list.active.*`), the focus `ring`, and — if a
future change adds one — an active-tab indicator. `accent.background` itself
is intentionally *neutral* (`#2a2a2a`, a plain hover shade), not the
saturated blue — don't repoint it at the accent hue, that's exactly the
"saturated colour used everywhere" problem this theme exists to fix.
Everything else (icons, secondary buttons, borders) stays desaturated grey.
The 3D viewport's own selection outline (yellow, `rbx_viewer`) is a
separate, Studio-parity colour and out of scope here — see §6.

Semantic colours (`danger`, `warning`, `info`, `success` — property-edit
error text, validation states) inherit the toolkit's stock values via
`dark-soft.json`'s fallback (§1) rather than being redefined; don't add a
project-specific danger/warning red without a reason `cx.theme().danger`
can't already cover.

## 5. Spacing, radius, density

`gpui_kit`'s spacing/radius helpers (`.px_2()`, `.py_1p5()`, `.gap_1()`,
`.rounded(cx.theme().radius)`, ...) follow a fixed Tailwind-style scale —
there is no separate "density" theme knob these custom-drawn widgets read
(`SpacingTokens`/`RadiusTokens` exist in the toolkit's Base layer but only
stock components consume them automatically). Practical rules:

- **Grow padding by moving to the next scale step**, not by hand-picking a
  pixel value — `py_1()` → `py_1p5()` (4px → 6px, +50%) rather than
  `.py(px(5.))`. A discrete +50-100% step reads as "less dense" without
  inventing a number nothing else in the file uses.
- **Toolbar buttons and list/tree rows (Explorer, Properties)** are the
  bullet this padding pass targets — see `shell/rows.rs`
  (`row`/`property_row`/`property_row_control`/`tagged_row`) and
  `shell/toolbar.rs`. Other panels (Style Editor, Output) weren't touched by
  that pass; match their existing density unless a specific task calls for
  changing them too.
- **Radius**: `cx.theme().radius` (5px) is the standard for general
  elements — buttons, list rows, tagged-row selection outlines.
  `cx.theme().radius_lg` (8px, the toolkit default, left unset in
  `dark-soft.json`) is for large surfaces — dialogs, notifications, popovers
  a whole panel's worth of chrome. Don't hand-write a radius literal; both
  are theme fields for a reason (a future theme/pack swap should be able to
  change them without a code edit).

## 6. What this theme does *not* cover

Two areas are a **different, deliberately distinct visual language** from
dockable panel chrome — don't reflexively pull them onto the panel palette:

- **The 3D viewport's own HUD** (`workspace_view.rs`: the quality/speed
  corner chip, the drag readout, the axis-orientation cube). These paint
  directly over an arbitrary, unpredictable 3D scene rather than a flat
  panel background, so they use their own semi-transparent dark chips
  (already documented in that file) instead of `cx.theme()` panel tokens —
  a panel-coloured chip can disappear against a same-toned part of the
  scene behind it in a way a panel never has to worry about.
- **The 3D selection outline colour** (`rbx_viewer`, yellow) — a
  Studio-parity choice, unrelated to the 2D editor theme.

## 7. Dock/panel structure

The dock (`shell/dock.rs`) has exactly six sections: Viewport, Explorer,
Properties, Output, Scripts, Style Editor — three side-by-side columns
today: **Properties tabbed with Output** on the left, **Viewport tabbed
with Scripts and Style Editor** in the middle (the "document" tabs — see
§8), **Explorer** alone on the right. This is `dock.rs::build`'s one
`h_split` of three `tabs()` groups — a deliberate three-column layout
(Properties/Output left, Explorer right) rather than the two-column,
stacked-panel arrangement an earlier pass shipped, chosen to match a
ribbon-style Studio redesign reference the maintainer supplied.
**Rearranging which sections tab together, or which side of the split they
sit on, is fair game for a UX-driven change** — `dock.rs::build` is the one
function that decides it. **Adding a *new* dock section is a bigger,
structurally-visible decision** (new persisted layout state, a new
`Section` variant, a new place for the maintainer to reason about) and
should go through the roadmap rather than ride along inside a theming pass.

## 8. The document tab row, and what lives in it

The Viewport/Scripts/Style-Editor tab strip is the first thing under the
menu bar — no separate toolbar row sits above it. The transform toolbar
(Select/Move/Scale/Rotate, snap, Align — `shell/toolbar.rs`) and the
graphics-quality dropdown both render *inside* the Viewport tab's own title
row instead, via `ComponentPanel::title_suffix` in `dock.rs` (the same
mechanism Output's filter/Clear controls and Viewport's own overflow menu
already used). **If a tab's title-row controls start to feel cramped**
(a narrow window, a long tool list), trimming what's *in* the suffix is the
right fix before reintroducing a separate full-width toolbar row — a
second row pushes the tab strip back down from directly under the menu bar,
which is the thing this arrangement exists to avoid.

**Tool buttons use an icon, not a text label**, with the name as a
`.tooltip(...)` instead (`shell/toolbar.rs::tool_icon` maps each `Tool` to
a Lucide `IconName` — check what a candidate icon actually depicts before
picking it: Lucide's own `Scale` is a balance/weighing-scale glyph, not a
resize one, which is why the Scale tool uses `Scale3d` instead). Reserve
this icon-only treatment for a small, fixed set of frequently-used tools a
user learns once; a rarely-used or one-off action is still better served by
a labelled button — an unlabelled icon nobody has memorized yet is a
lookup cost, not a saving (Nielsen's "recognition over recall" cuts both
ways here:
[nngroup.com/articles/ten-usability-heuristics](https://www.nngroup.com/articles/ten-usability-heuristics/)).

## 9. Verifying a visual change

No shortcut around `agents/AGENTS.md`'s existing rule: build the real
`rbxstudio` binary and look at a screenshot, for every change this file
covers. A few things specific to this kind of change:

- **Check a tagged Explorer row, not just a plain one** — `shell/rows.rs`'s
  tagged-folder path paints its own hover/selected chrome independently of
  `ListItem`, so a theme change can silently miss it (see §1's last bullet).
- **Check hover and selected state, not just the resting screenshot** — most
  of what changes (accent, seam contrast) only shows up in an interactive
  state.
- **A before/after pair beats a single screenshot** for anything touching
  the palette — it's the only way a reviewer can tell "softer" from "just
  different."
