# rbx-native — Roadmap

rbx-native is a from-scratch, native Linux/Windows replacement for Roblox
Studio. This document tracks what's implemented, what's planned, and — since
Roblox's actual game engine is closed-source — what's outright impossible to
match versus what can be approximated with a deliberate workaround. It's the
single source of truth for project direction; if something here looks stale,
[CHANGELOG.md](CHANGELOG.md) has the day-by-day record of what actually
landed and why.

Feasibility legend: ✅ done · 🚧 in progress / partial · 📋 planned, not
started · ⚠️ possible only via a workaround · ❌ not possible without
Roblox's own engine.

## What's been implemented

### Format & parsing (`rbx_dom`, `rbx_binary`, `rbx_xml`, `rbx_reflection`, `rbx_mesh`)
- [x] Binary `.rbxm`/`.rbxl` — read **and** write, exact round-trip verified
  against real files (16k+ instance places), including the shared-string
  (`SSTR`) table.
- [x] XML `.rbxlx`/`.rbxmx` — read **and** write, exact round-trip.
- [x] Reflection database (682 classes, `Service`/`NotCreatable`/
  `NotBrowsable`/property-category tags) parsed straight from Roblox's own
  public API dump, kept current by a daily CI sync.
- [x] Mesh format `.mesh` v1.00–v5.00.
- [x] Mutable in-memory DOM (`set_property`/`remove`/`new_instance`/
  `take_changes`) — what live editing is built on.
- [x] `rbxdump --roundtrip` — a continuous-verification tool that found and
  got two real serialization bugs fixed.

### Scripting (`rbx_lua`)
- [x] Sandboxed Luau VM (`mlua`) with a from-scratch DataModel
  (`Instance`/`game`/`workspace`, `Vector3`/`CFrame`/`Color3`/`UDim2`/
  `NumberSequence`/`ColorSequence`/`Rect`/`PhysicalProperties`/`Font`/
  `Content`, defaults for a fresh `Part`).
- [x] CLI runner (`rbxlua place.rbxl script.luau [--out] [--print-changes]`)
  and an in-editor Command Bar.

### Renderer (`rbx_viewer`)
- [x] Lighting model reverse-engineered from Roblox's own decompiled
  shaders and calibrated pixel-by-pixel against real Studio captures: two
  lamps + sky ambient, Blinn-Phong specular, sky cubemap reflection,
  fog/`Atmosphere`, sun/moon, shadow maps for `SpotLight`/`SurfaceLight`,
  local point lights, bloom/stars/`ColorCorrectionEffect`/
  `ColorGradingEffect`/`BlurEffect`/`SunRaysEffect`/`DepthOfFieldEffect`.
- [x] 43 official Roblox materials (`rbx_materials`) with real texture
  packs, `MaterialVariant`, `SurfaceAppearance`.
- [x] Decals/textures projected on real geometry (not just boxes), and
  patched in place — not just redrawn on a full reload — when the
  `CFrame`, size or shape of the part they're pinned to is edited live.
- [x] Sky: 6-face skybox, environment cubemap, stars, night sky, Roblox's
  own default sky (DDS BC1/2/3 decoder), procedural `Clouds`.
- [x] Shapes: `Part`/`MeshPart`, Ball/Cylinder/Wedge/CornerWedge,
  `TrussPart` (real lattice geometry), `SpecialMesh`.
- [x] Legacy `UnionOperation`/`NegateOperation` — a real, from-scratch CSG
  boolean (BSP tree, dependency-free) over the operation's original
  constituent parts, not just a bounding-box fallback.
- [x] Effects: `ParticleEmitter` (CPU simulation + billboards), `Beam`,
  `Trail`.
- [x] GUI containers: `ScreenGui`/`Frame`/`ImageLabel`, `BillboardGui`,
  `SurfaceGui`.
- [x] GUI — full `GuiObject` compatibility (every class below checked
  against `Roblox/creator-docs`; where the docs are silent the code says so
  in a comment rather than presenting a guess as verified):
  - **Text**: `TextLabel`/`TextButton`/`TextBox` draw their glyphs, shaped
    with `cosmic-text` (already in the tree under GPUI). Roblox's own font
    families are fetched at runtime from the Studio content package the way
    textures already are — the family JSON, then the closest face by
    weight/style — never shipped; a system font stands in until they land
    or where a face needs an API key. `FontFace` and the legacy `Font` enum,
    `TextSize`/`TextScaled`/`TextWrapped`, both alignments, `LineHeight`,
    `TextStrokeColor3`/`Transparency`, `RichText` (`<b>`, `<i>`, `<u>`,
    `<s>`, `<br/>`, `<font>`; other tags stripped), `TextTruncate`,
    `MaxVisibleGraphemes`, a `TextBox`'s placeholder, and `AutomaticSize`
    on a text object. Approximations, each commented: a text stroke is the
    glyphs drawn again in the eight neighbouring pixel offsets (Roblox
    publishes no algorithm), `TextTruncate.SplitWord` truncates like
    `AtEnd`, and `MaxVisibleGraphemes` counts `char`s rather than grapheme
    clusters.
  - `ImageButton`, plus every `ScaleType` (`Slice` with `SliceCenter`/
    `SliceScale`, `Fit`, `Crop`, `Tile`), `ImageRectOffset`/`ImageRectSize`
    and `ResampleMode.Pixelated`. `Tile` combined with an image sub-rect
    tiles the whole image (a wrapping sampler cannot repeat a window into
    it) — the docs describe no such combination.
  - `GuiObject.Rotation` composed the way `GuiBase2d.AbsoluteRotation`
    implies: a rotated element carries its children around its own centre,
    then each turns about its own. `ClipsDescendants` is still a no-op
    under any rotation, per the docs' default
    `ClipsDescendantsSupportsRotation` mode.
  - `UIGradient` — `Color`/`Transparency` sequences, `Rotation`, `Offset`,
    `Scale`, `Type` (`Linear`/`Radial`/`Conical`) and `TileMode`, applied
    to the background, the image and the text alike through a gradient
    ramp texture the GUI shader samples.
  - `UICorner` (per-corner radii, an anti-aliased rounded-box SDF in the
    shader that clips the background, image and text), `UIStroke` (`Border`
    and `Contextual` — a text object's contextual stroke outlines its
    glyphs — with `Thickness`, `Transparency`, `LineJoinMode`,
    `BorderStrokePosition`/`BorderOffset`, `StrokeSizingMode`; one stroke
    per element, a gradient inside a stroke not read), `UIPadding`.
  - Layout: `UIListLayout` with `HorizontalFlex`/`VerticalFlex`, `Wraps`
    and `ItemLineAlignment`, `UIFlexItem`, `UIGridLayout`, `UITableLayout`.
    `UIPageLayout` is not read (one page at a time, animated).
  - Sizing: `UIScale`, `UIAspectRatioConstraint` (both `AspectType`s, both
    `DominantAxis`), `UISizeConstraint`, `UITextSizeConstraint`,
    `AutomaticSize` (children extent, a layout's `AbsoluteContentSize`,
    a text object's own bounds), `SizeConstraint`, `BorderMode`
    (`Middle`/`Inset`), `ScreenGui.ScreenInsets`/`IgnoreGuiInset` (the top
    bar reserved at 36 px — no Roblox page states the number, it is one
    named constant) and `ZIndexBehavior.Global`.
  - `StyleSheet`/`StyleRule`/`StyleLink`/`StyleDerive` — the selector
    engine (class, `.Tag`, `#Name`, `>` and `>>` combinators, `,` lists,
    nested rules merged into their parent's selector), `Priority` then
    document order, derives flattened in priority order, `$Token`s
    resolved along the derive chain, and `StyleRule.Properties` decoded
    from (and written back to) the `PropertiesSerialize` attribute blob,
    verified byte-for-byte against a Studio-written file. State selectors
    (`:Hover`…), `@Query`, `::UIComponent` phantom instances and property
    transitions parse but never apply: a still frame has no input. A matching
    rule always wins over the instance's own value, since a saved place
    cannot tell an explicit override from a default. Real Studio's **Style
    Editor** panel has its equivalent in `rbxstudio` (View ⟩ Style Editor):
    every sheet, its derives and rules, the selector/priority/properties
    editable in place through the undo history, with the viewport
    re-rendering on every edit; sheet tokens and `StyleQuery` are not
    editable there yet.
- [x] Free-flight camera (WASD + mouse look + wheel), exponentially-eased
  movement (mouse look itself stays unfiltered).
- [x] Orthographic camera mode — toggled from the Viewport panel's overflow
  menu (`rbxstudio`) or `rbxview --orthographic`. Flies with the same
  WASD/mouse-look controller; the mouse wheel without the look button held
  zooms the view volume directly (`Pose::ortho_scale`) instead of dollying
  the eye, since dollying does nothing visible under a parallel projection
  and risked flying through geometry with no size cue — independent of
  camera position, so it works the same on any level regardless of layout.
  The sky/star/sun background stays perspective regardless, by design (their
  near-unit-magnitude geometry relies on the perspective divide to spread
  across the screen at all; a parallel scene with a perspective background
  is a deliberate decoupling, not an oversight — see `Camera::orthographic_projection`
  and `Camera::view_rotation_projection`'s doc comments).
- [x] Automatic render-quality scaling (Quality Level 1–21, frame-rate
  driven), live quality switching without a scene rebuild.
- [x] Scoped strictly to `Workspace` — a place's other services
  (`ReplicatedStorage`, `ServerStorage`, …) never leak into the render even
  if they happen to hold `BasePart`s.
- [x] **Downloaded assets are cached on disk, keyed by asset id, and never
  re-fetched from Roblox's CDN once they've landed once.** Reported as
  missing from real use — it isn't; `rbx_assets::AssetCache` writes every
  resolved `rbxassetid://` and Studio content-package fetch to
  `$XDG_CACHE_HOME/rbx-native/assets/` (`%LOCALAPPDATA%` on Windows),
  atomically (temp file + rename, so a crash mid-write can't corrupt an
  entry), and `AssetResolver::resolve_id`/`resolve_native` check it before
  ever calling the network fetcher — a second place, or a second reload of
  the same one, hits disk, not the CDN. Persists across process restarts,
  not just within one session. Still open: no size cap or eviction, so the
  directory only grows; low priority in practice, since Roblox's own
  asset ids are immutable content and the bytes never need invalidating.
- [x] **Assets are resolved off the render thread and swapped into the
  picture as they land**, the way Roblox's own engine streams them: a load
  returns with the scene drawable and its assets still arriving, and an
  edit naming a `MeshId`/`TextureID`/material this session has never
  decoded draws the fallback it already had for one that never resolved —
  the `MeshPart`'s box, the untextured mesh, plain plastic — asks for the
  asset in the background (`load::fetcher`, a worker pool behind
  `load::Resident`'s in-flight state) and folds it in on a later tick,
  instead of falling back to a full reload with a download in front of it.
  The four renderer passes that fetched their own textures inside
  `Renderer::rebuild` (`particles`, `beam`, `trail`, `gui::atlas`) read the
  loader's answer instead, so nothing on the render thread resolves an
  asset any more. On `marked.rbxl`: an unseen-`MeshId` edit is on screen in
  0.9 ms and the mesh itself swaps in 3.1 ms later, against a ~29 ms reload
  plus a fetch; a cold load's first drawable frame went from 1004 ms to
  462 ms, with the finished picture still at 978 ms.
- [x] **Loading a real place no longer spikes CPU (and, on a laptop, the
  fans) the way it was reported to from real use.** Both candidates the
  original report named turned out to matter. Profiled with a synthetic,
  CSG-heavy place (this repository ships no real one): resolving 40 legacy
  `UnionOperation`/`NegateOperation` booleans (30 leaves each) took ~6s
  single-threaded versus ~0.1s to generate the equivalent number of texture
  mip chains, so the CSG boolean was the dominant cost — but that mip-chain
  number only measured CPU-side mip generation, not the actual GPU upload
  burst, which turned out to matter on its own.
  `crates/rbx_viewer/src/scene/union.rs`'s `resolve` now runs each distinct
  asset's from-scratch BSP boolean across a bounded worker pool
  (`evaluate_all`, sized to available CPU parallelism rather than a fixed
  count, since this work is CPU-bound rather than rate-limited by a remote
  server) instead of one after another on the caller's own thread — the
  same synthetic place now resolves in ~0.8-1.1s. Separately,
  `renderer/textured.rs`'s `Textured::new` used to upload every
  `Decal`/`Texture` image's full mip chain to the GPU in one uninterrupted
  burst before the renderer was usable at all; it now seeds each slot with
  a cheap placeholder and `Renderer::draw` uploads a bounded number of the
  real images per call (`texture::PER_FRAME`), so a place with many
  textures spreads that cost across the frames after load instead of
  stalling the first one. Measured directly (real GPU, 60 synthetic
  1024x1024 images, no network, no committed asset): one burst uploading
  all of them took ~245-295ms, while spreading it across 8 budgeted frames
  kept every single frame under ~33ms. The one-shot `rbxview --screenshot`
  path has no next frame to spread across, so it drains any remaining
  upload immediately (`Renderer::finish_loading`) before capturing rather
  than writing out a PNG with textures still mid-upload.

### Editor (`rbx_studio`, binary `rbxstudio`)
- [x] Explorer: real Roblox class icons (fetched at runtime, never
  committed), default service filter with a "show all" toggle, selection
  with a viewport highlight, instance insert/delete.
- [x] Properties panel: real per-type widgets (checkbox, colour picker,
  enum dropdown, numeric fields for vectors/CFrame position), grouped into
  collapsible categories matching Roblox's own Properties panel, live —
  reflects DOM mutations from any source (script, undo/redo, the camera
  moving) without a manual refresh.
- [x] Command Bar (Luau against the live DataModel) with an Output dock:
  run history, Clear, a success/error filter, click-to-recall a past
  command.
- [x] Menu bar (File/Edit/Model/View) wired to real actions where one
  exists; everything else an honestly-disabled placeholder rather than a
  button that looks functional and isn't.
- [x] Undo/redo (`Ctrl+Z`/`Ctrl+Y`), bit-for-bit reversion verified.
- [x] Save (`Ctrl+S`, writes back in the file's original format, atomic
  write).
- [x] Dockable, rearrangeable panel layout (`gpui_component::dock`) with
  persistence — layout position/size/docking state saved across restarts,
  plus persisted settings (quality, service visibility).
- [x] Live camera pose reflected into `Workspace.CurrentCamera.CFrame` as
  you fly, throttled and explicitly excluded from undo history.
- [x] Fast-path scene updates: a `Lighting`/`Atmosphere`/post-effect edit or
  a single part's property change patches the GPU state directly instead
  of rebuilding the whole scene — editing stays interactive on large real
  places.
- [x] Every edit reaches the viewport as a patch of the instances it
  touched, never a rebuild — the Roblox model, where the DataModel is the
  live scene and a property write is an event applied to that one
  instance. `Headless::apply_changes` takes the `Change` log a mutation
  produced — a Properties row, a viewport drag, a Command Bar script
  touching a hundred parts, the Explorer's insert, delete and drag-drop,
  and undo/redo of any of them (`shell/history.rs` hands over the very log
  the undone mutation produced, against the restored DOM) — and re-derives
  only what it names: a part's box or mesh, its shadow caster and outline,
  the decals, lights, emitters and attachments hung off it; a `Model`
  moved reaches its whole subtree; a failed-CSG union drawn as its
  recovered pieces is patched piece by piece, each piece addressed by an
  identity of its own (`scene::PartId`). An edit naming an asset that was
  never downloaded — a mesh, texture, material pack, `SurfaceAppearance`
  set or legacy union asset alike — is patched too, onto its fallback,
  with the asset fetched in the background (see the renderer's
  asset-streaming entry above) instead of forcing a reload the way it once
  did. What still rebuilds the whole scene is the closed list in
  `rbx_viewer::Rebuild`, down to three: a `Sky` edit (the environment
  probe is prefiltered from its six panels), a
  `MaterialVariant`/`MaterialService` edit (the material catalog is
  defined from the service), and a material needing a texture-array layer
  past the ones uploaded.
- [x] **Interactive viewport gizmos — Select/Move/Scale/Rotate, matching
  Studio's real toolbar and behaviour**, checked against
  `Roblox/creator-docs` (`parts/index.md#transform-parts`,
  `parts/models.md#select-models`) rather than assumed:
  - **Select**: clicking an outlined object selects it, resolving to the
    outermost enclosing model; `Alt`/`⌥`-click cycles through whatever's
    under the cursor instead of just the nearest hit ("selection
    cycling") — matching Studio's own mechanism rather than a
    `Shift`-click-into-children behaviour it doesn't actually have.
    Cycling is a plain geometric raycast against every part the ray
    crosses (`rbx_viewer::pick::parts_along`), sorted nearest first, not a
    depth-buffer occlusion test — so it already reaches a part fully
    nested inside another (an invisible hitbox sitting inside a visible
    part, say) or buried several levels into a `Model`, one `Alt`-click at
    a time, without ever needing to select the enclosing part or model
    first; whatever it lands on gets a gizmo exactly as any other
    selected `BasePart` does, since cycling deliberately never resolves up
    to a `Model` the way a plain click does (see "Still open" below for
    the one related case that doesn't yet have a gizmo of its own).
    `Shift`/`Ctrl`/`Cmd`-click adds another top-level object to the
    selection instead of replacing it.
  - **Move** (`2`), **Scale** (`3`), **Rotate** (`4`) — colored axis
    draggers/handles/rings per axis; `Ctrl`/`Cmd`+`L` toggles world/local
    orientation, with an `L` indicator when local is active. Move is also
    draggable by the part's own body ("cursor dragging"), which rests the
    part on whatever the cursor passes over — real geometry, not a
    bounding box — falling back to sliding flat across the view only when
    the cursor is over nothing.
  - **Snapping**: move/scale snap in studs, rotate snap in degrees, each
    with its own toolbar increment field and enable/disable checkbox;
    holding `Shift` mid-drag inverts the current snap state for that one
    drag. A free Move drag soft-snaps its grab point onto nearby
    surfaces/edges when snapping is off. While cursor-dragging a part,
    `T`/`R` tilt/rotate it 90° around the grab point.
  - **Multi-select**: `Shift`/`Ctrl`/`Cmd`-click adds/removes a top-level
    object; the Explorer, viewport outline and Properties panel all follow
    the whole set. One Move gizmo appears, centred on the selection's
    bounding box; dragging any handle, or any selected part's own body,
    moves the whole group together by the same offset, so its layout
    relative to itself never changes. Scale and Rotate act on the whole
    selection too, centred on its bounding box: Rotate turns every part
    about that centre, and Scale grows the group about the box's opposite
    face by one factor (the way `Model:ScaleTo` scales — a per-axis stretch
    is nothing a rotated part's own `Size` can express, and Studio gives
    the tools no per-axis model behaviour). A lone part keeps its own
    per-axis Scale.
  - Clicking (and dragging) resolves against the shape actually drawn —
    sphere, capped cylinder, wedge slope, a downloaded mesh's own
    triangles — not the part's bounding box.
  - **Placement**: directly under the File/Edit/Model/View menu bar and
    above the viewport dock, matching the owner's reference screenshot.
  - **Still open**: the 5th "Transform" toolbar button visible in Studio's
    current toolbar; group/ungroup operations; and a plain click landing
    on a `Model` — unlike cycling's raw parts, a `Model` has no `CFrame`/
    `Size` of its own for the gizmo to read, so it draws neither an
    outline nor a gizmo today (all under "What's planned" → Renderer).

- [x] **A real script editor** — double-clicking a `Script`, `LocalScript`
  or `ModuleScript` in the Explorer opens it in the Script Editor dock
  panel, which shares the viewport's tab group the way Studio's own does.
  Several scripts open as several tabs, each closable; re-opening one
  already open focuses its tab rather than re-seeding it.
  - **Luau syntax highlighting** from a hand-written lexer
    (`script_editor::luau`) plugged into GPUI Kit's own
    `InputHighlighter` seam, so it costs no grammar crate and takes its
    colours from the active theme. Luau, not Lua: type annotations,
    `continue`/`export`/`type`, compound assignment, `0b` literals,
    digit separators and backtick interpolation all lex correctly, where
    a Lua 5.1 grammar treats each as a parse error.
  - **Edits reach the live DOM** through the same `set_property` every
    other property edit ends at, 400 ms after typing stops, with an undo
    snapshot per typing burst. Ctrl+S, undo and redo flush any pending
    write first, so each acts on the text as typed. An open tab is
    reconciled against the DOM on every render, so an undo or a Command
    Bar script re-seeds it and a deleted script closes its tab.
  - `Source` is now read-only in the Properties panel for script classes:
    a one-line field is the wrong shape for code, and that panel's commit
    path trims what it writes, which silently ate a script's trailing
    newline.
  - **Still open** (their own bullets under "What's planned"): the
    table-stakes conveniences (multi-cursor, Find/Replace, Go to
    Declaration, the function filter), `luau-lsp` integration, script
    debugging, and new-script templates. With focus in an editor, Ctrl+Z
    is the editor's own text undo rather than the place's history — the
    split Studio makes — and Edit > Undo still reaches the latter.

### Platform
- [x] Linux (X11) — the daily-driven target.
- [x] Windows — asset cache and settings now fall back to
  `%LOCALAPPDATA%`/`%APPDATA%` when no `XDG_*`/`HOME` is set; the
  rendering/editor stack (`wgpu`, GPUI Kit) is cross-platform by
  construction. Never built on a real Windows machine yet — see
  [Platform: Windows](#platform-windows) below.

## What's planned

### Script authoring — the biggest real gap
- [ ] 📋 **Table-stakes editor conveniences**, real and current per
  `studio/script-editor.md`, worth scoping alongside the editor above
  rather than as an afterthought since retrofitting multi-cursor support
  onto a single-cursor text widget later is real rework: multi-cursor
  editing (add/remove a cursor per click, add-above/below, "add cursor to
  next/every matching selection", column/block select), a **Find/Replace**
  widget plus a separate **Find All/Replace All** that searches every open
  script rather than just the current one, **Go to Declaration**
  (`Ctrl`-click or right-click on a call), and a **Script Function
  Filter** (a searchable list of every function declared in the current
  script, `Alt`+`F`). None of this depends on `luau-lsp`; **Script
  Analysis** (real Studio's static-analysis pass, in-editor squiggles plus
  a details window) is closer to genuinely duplicate work with the
  `luau-lsp` diagnostics below and probably shouldn't be built twice.
- [ ] 📋 `luau-lsp` integration (external LSP-over-stdio process,
  autocomplete/diagnostics) — needs a `sourcemap.json` compatible with
  Rojo's format (see below) and the script editor above.
- [ ] 📋 Script debugging — technically reachable via Luau's own debug
  hooks in `mlua`, large effort, depends on the script editor and, for
  anything beyond the Command Bar, on the Play workaround below. Real
  Studio's actual feature set, per `studio/debugging.md`, is worth
  matching rather than a vague "breakpoints" placeholder: **standard**,
  **conditional** (breaks only when a given expression is true),
  **logpoint** (logs to Output without pausing — no breakpoint hit at
  all), and **temporary** (auto-removes itself after one playtest session)
  breakpoints, plus **Watch** (expression inspection while paused) and
  **Call Stack** panels once paused.
- [ ] ⚠️ **Inline AI code completion**, matching what real Studio calls
  **Code Assist** (`studio/script-editor.md`) — suggests a line/block of
  code inline as you type or pause, distinct from `luau-lsp`'s
  diagnostics/autocomplete above. Explicitly not attemptable as *Studio
  parity*: Roblox's version is backed by a model Roblox trained and hosts
  itself, nothing this project has access to. A workaround exists in the
  same shape as the Material/Texture Generator item under
  [Possible via a workaround](#ai-content-generation) below — wire the
  editor's completion request to any third-party or
  locally-hosted code-completion API instead — but that's a distinct,
  lower-priority idea worth its own decision on which backend (if any),
  not a default this project should ship opinionated about.
- [ ] 📋 **New-script templates** — inserting a `Script`/`LocalScript`/
  `ModuleScript` today starts from an empty `Source`; real Studio offers a
  small set of starting points instead (a plain server script, a
  `ModuleScript` returning a table, an OOP class module with a `.new()`
  constructor and metatable). Worth a small, user-extensible template set
  rather than hardcoding a fixed list — most useful once the script editor
  above exists (a template matters a lot more with somewhere real to edit
  it), though the underlying "insert with starter source" mechanic doesn't
  strictly depend on it.
- [ ] 📋 **Optional, bundled `Fragment` UI framework.** [`Fragment`](https://github.com/chteau/Fragment)
  (MIT, single-file Luau `ModuleScript`, React-inspired: local/global
  state, contexts, reusable components over plain `GuiObject`s) offered as
  an opt-in starter package when scaffolding a new UI-heavy project —
  never forced, a project that doesn't want it should look exactly like
  one built without this editor at all. Lightweight enough (one
  `ModuleScript`, no external runtime) that "bundle it, default off" is
  realistic in a way a heavier framework wouldn't be. Natural pairing with
  the script templates above and, if it lands, the Wally package manager
  below (Fragment installed as an ordinary Wally dependency rather than a
  copy-pasted module would be the more maintainable path once Wally
  exists).

#### Far future: node-based scripting
- [ ] 📋 A visual, node-graph way to write Luau logic — Unreal Blueprint or
  Blender's shader/geometry nodes, not Node.js. Explicitly a long-horizon,
  unscoped idea at this point, recorded here so it isn't lost rather than
  because there's a design yet: no decision made on node-graph-to-Luau
  compilation vs. a node graph that *is* the runtime representation (an
  interpreter over the graph itself), what subset of the language is
  representable as nodes, or how it'd interoperate with hand-written
  script modules in the same place. Depends on the real script editor
  above existing first regardless of which direction it takes.

### Renderer
- [ ] 📋 `Light.Shadows` for `PointLight` (needs 6-face shadow maps; done
  for `SpotLight`/`SurfaceLight`).
- [ ] 📋 Neon/`ForceField` shimmer, `Glass` refraction — currently flat.
- [ ] 📋 **An FPS/frame-time readout**, matching real Studio's own
  performance-debugging surface rather than inventing a new one: Studio's
  `Window > Performance > Stats` toggles a debug stats overlay, and
  `Ctrl`+`F6` opens the MicroProfiler directly for a per-system frame-time
  breakdown. Today's viewport corner label (`rbxstudio`) and title bar
  (`rbxview`) show quality level and flight speed but no frame rate or
  frame time at all, even though the render thread already measures frame
  timing internally to drive automatic quality scaling (see "What's been
  implemented" above) — the number already exists, it just isn't shown
  anywhere yet.
- [ ] 📋 **A 5th "Transform" toolbar button** appears in Studio's current
  toolbar (see the owner-provided screenshot) alongside the now-implemented
  Select/Move/Scale/Rotate (see "What's been implemented" → Editor), but
  creator-docs' `parts/index.md` only documents "Transform parts" as the
  *umbrella name* for Move+Scale+Rotate together, not a distinct 5th
  interactive tool — could not confirm what it does specifically from the
  docs alone. Needs confirming against a real Studio instance (the "Studio
  fallback" Vinegar/Wine workaround elsewhere in this roadmap is one way to
  do that) before implementing it, rather than guessing.
- [ ] 📋 **A `Model`'s aggregate bounding box**, so a plain click that
  resolves to one (see "What's been implemented" → Editor's Select
  bullet) actually shows something instead of silently selecting an
  instance with nothing to draw. Every part inside already carries its
  own `CFrame`/`Size`; the box is the union of all of them, matching what
  the Align tool's "Selection Bounds" mode below and real Studio's own
  `PVInstance:GetPivot()`-adjacent bounding-box concept describe.
  `renderer::selection`'s own doc comment already flags this ("a Model's
  aggregate bounds are a TODO: nothing here derives one yet") — worth
  wiring up rather than leaving as a silent no-op, and a natural fit for
  the Move-only group-gizmo machinery `transform::Targets` already has
  from multi-select (see "What's been implemented" → Editor), since
  moving a `Model` is exactly "move every part inside it by the same
  offset." `Alt`/`⌥`-click cycling remains the way to reach and gizmo one
  specific part inside the model instead of the whole thing, exactly as
  it does today.
- [ ] 📋 **Gizmo papercuts and third-party-tool parity requests**, from
  real use of the Move/Scale/Rotate gizmos above. Checked against
  `Roblox/creator-docs` (`parts.md`'s Transform Parts section) and, where
  noted, **Building Tools by F3X** — a widely-used third-party Studio
  plugin, not native Studio — since some of what was asked for turns out
  to be F3X's own convention rather than something Studio itself does:
  - **Scale handles that lock a Ball/Cylinder to a round cross-section**:
    dragging any Scale handle on a Ball currently grows only the one axis
    grabbed, same as any other part, which turns a sphere oval. Native
    Studio's own docs give Scale no shape-specific behavior at all —
    `BasePart.Size` is three independent numbers regardless of `Shape` —
    so this isn't a Studio-parity gap so much as a genuinely useful
    addition modeled on F3X's real, open-source `Resize.lua`: grabbing a
    Ball's handle grows all three axes together (`Size + (d,d,d)`, keeping
    it a sphere); on a `Cylinder`, only the two axes forming its circular
    cross-section grow together while the length axis scales alone.
    Wedge/CornerWedge weren't confirmed to have their own case in F3X's
    source and need a closer read before assuming one; plain `Part`s and
    `MeshPart`s keep today's per-axis behavior either way. Requested as a
    `Shift`-held modifier during Scale — note this collides with `Shift`'s
    existing meaning (invert the current snap state) on every other tool,
    so it needs its own resolution, not a blind rebind. — see
    `F3XTeam/RBX-Building-Tools`'s `Tools/Resize.lua`.
  - **A live stud-count readout while a Move/Scale drag is in progress**
    (e.g. a floating "12" near the handle showing studs moved/grown so
    far) — genuinely useful, not documented as a specific Studio feature
    either way; would need its own small on-screen label wired to the
    drag's own already-known delta.
  - **`Tab` to "summon" the gizmo's handles to the cursor** — this one
    *is* real, current native Studio behavior (2021 "Pivot Points" beta
    update): holding `Tab` moves the active tool's handles to the cursor's
    location, including Scale's, which stay "within the bounds of [the]
    selected object" rather than sitting on its actual surface. Worth
    implementing as described in Roblox's own DevForum announcement, not
    guessed at.
  - **A small free-drag handle at the gizmo's own origin**, independent of
    clicking the part's body directly — useful in particular once
    `Tab`-summoning (above) can put the handles somewhere that isn't
    sitting on the part's own mesh anymore. Functionally close to what
    Move's existing cursor-drag-and-settle already does (see "What's been
    implemented" → Editor) when clicking the part's body directly; this
    would be the same gesture from a fixed point on the gizmo instead.
  - The requested `Shift+X`/`Shift+C` snap-toggle chord and the `H`
    hotkey-help overlay are also F3X conventions, not native Studio's
    (F3X's own `C` rotate-tool binding is documented to conflict with
    Studio's native `C` = Toggle Comment Cursor) — worth checking against
    a real Studio instance before adopting either verbatim.
  - [x] 🚧 **Selection outline shape and thickness**: the *thickness* half
    has shipped — the outline (see `rbx_viewer::renderer::selection` and
    `renderer::outline`) is no longer a one-pixel `LineList` but a
    screen-space quad per edge, expanded in the vertex shader to a constant
    on-screen width (~3px selection blue, matching Studio's light-blue
    selection box; the hover cue rides the same path in amber). What is
    *still open* is shape-conformance: a `Ball`, `Cylinder`, wedge or mesh
    still outlines as its oriented bounding box, not its own silhouette, and
    Real Studio's own highlight is documented (`parts/models.md`) only as a
    light-blue outline with no shape-conformance spec published — so matching
    the true silhouette still needs checking against a real Studio instance
    rather than guessing.
- [ ] 📋 **Align tool**, matching Studio's real Model-tab tool (checked
  against `studio/align-tool.md` rather than assumed, not the transform
  gizmos under "What's been implemented" → Editor). Aligns the selected
  objects' **Min**/**Center**/**Max** bounds
  along independently-toggled **X**/**Y**/**Z** axes, in **World** or
  **Local** space, relative to either the **Selection Bounds** (the
  selection's collective bounding box) or the **Active Object** (the last
  -selected object in a multi-selection, which stays fixed while the rest
  align to it) — depended on multi-selection existing first, since aligning
  a single object to itself is a no-op; multi-selection is now implemented
  (see "What's been implemented" → Editor), so that dependency is
  satisfied. Self-contained geometry math over whatever's already selected;
  no new dependency.
- [ ] 📋 **Pivot tools**, matching Studio's real Model-tab **Edit Pivot**/
  **Reset** tools (checked against `studio/pivot-tools.md`). Today's
  transform gizmos (see "What's been implemented" → Editor) move/rotate/
  scale a part or model around its existing pivot; nothing lets you *move
  the pivot itself*. Real Studio's Edit
  Pivot tool repositions/reorients a part's or model's pivot independently
  of its geometry (rotation and scaling then happen around the new pivot),
  with **Snap**-to-hotspot behaviour (corners/edges/centers highlighted in
  magenta while dragging) and a one-click **Reset** back to the bounding
  box's center. Assigning a `Class.Model.PrimaryPart` moves the pivot to
  that part's own pivot and — deliberately, per the docs, to avoid a
  sudden jump — does **not** snap back if the `PrimaryPart` is later
  deleted. Also needs `PVInstance:GetPivot()`/`PVInstance:PivotTo()`/
  `BasePart.PivotOffset` exposed to the Command Bar and scripts generally
  (today's Luau DataModel has no pivot-specific API at all), not just the
  interactive tool, since real Studio exposes both.
- [ ] 📋 **Group/ungroup operations** (wrap the selected instances into a
  `Model`, or unwrap one back into its parent) — selecting several
  instances at once is done (see "What's been implemented" → Editor);
  grouping/ungrouping them is not.
- [ ] 📋 **Full DOM editing from the Explorer**, beyond today's plain
  insert/delete:
  - A `+` icon on each row to insert a child instance directly, without
    going through a menu — greying out (not hiding) instance types that
    can't legally be added to the hovered parent, so the constraint is
    visible rather than silently enforced.
  - Right-click context menu: **Cut**, **Copy**, **Duplicate**, **Rename**
    (also bindable to `F2`), **Insert** (same picker as the new `+` icon),
    **Group as model**.
  - Export from the menu: services and the whole place to Roblox (Save/
    Publish, see below), to a local file; individual instances to `.obj`
    and `.gltf` — genuinely useful native additions since Studio itself
    has no built-in mesh export, not something to frame as "matching
    Studio" since it doesn't do this either.
  - **Keep search results browsable without clearing the search field** —
    a real devforum request
    ([`view-descendants-of-matching-instances-in-explorer-search`](https://devforum.roblox.com/t/view-descendants-of-matching-instances-in-explorer-search/4862003),
    read in full, not just the title: today's real Studio forces you to
    clear the Explorer's search box before you can expand a matched
    result's children, so inspecting several matches in a row means
    search → select → clear → expand, repeated per result). Worth getting
    right from the start here rather than reproducing that friction: a
    filtered Explorer tree should stay expandable in place.
  - **Two insertion preferences** real Studio exposes next to the `+`
    icon's search field (`studio/explorer.md`): **increment names for new
    instances** (numbered names for same-type inserts/pastes/duplicates)
    and **expand hierarchy when selecting** (whether inserting/pasting/
    viewport-selecting an instance auto-expands the Explorer tree to
    reveal it, or only highlights the top-level parent) — small, but
    real, user-facing toggles worth including alongside the rest of this
    item rather than hardcoding one behaviour.
- [ ] 📋 Copy/paste/duplicate instances (`Ctrl+C`/`V`/`D`) — Insert/Delete
  exist, these don't yet (the menu bar already has honest placeholders for
  them); folds into the fuller Explorer editing item above rather than
  being separate work.
- [x] 🚧 Drag-and-drop reparenting in the Explorer tree. Dragging a row
  onto another reparents onto it, the way creator-docs describes
  ("simply drag and drop them onto the new parent") — with a ghost under
  the cursor, the hovered row highlighted only while the drop is legal,
  the new parent expanded and revealed afterwards, and one undo step per
  drag. A drop is refused onto the dragged instance itself, into its own
  subtree, onto the parent it already has, and for a service. Still open:
  dragging a row *outside* the current selection collapses that selection
  to the pressed row before the drag starts, so a multi-instance drag only
  carries the whole selection when grabbed by its anchor row — the
  Explorer tree tracks one selected row and already behaves this way for a
  plain click, so fixing it properly belongs with the fuller Explorer
  editing item above. No Escape-to-cancel (GPUI has no drag-cancel hook
  wired), and no drop *between* rows, which Studio does not offer either.
- [ ] 📋 **Save/Publish to Roblox from the editor UI.** The Open Cloud
  client side of this already exists and works —
  `rbx_cloud::Client::publish_place`
  (`POST /universes/v1/{universe}/places/{place}/versions?versionType=Saved|Published`)
  already distinguishes **Save** (`versionType=Saved`) from **Publish**
  (`versionType=Published`), which is exactly the Roblox-side distinction
  between saving a version and publishing a new live version of an already
  -published place. What's missing is wiring it into `rbxstudio`'s own
  File menu: prompting for (or remembering) a universe/place id, offering
  "Save" vs. "Publish" as separate actions once a place is linked to one,
  and "publish as a new version" being the natural behaviour once a place
  already has an associated `placeId` — no new API work, this is an editor
  -UI task on top of an existing, working client. **Version history**
  (browsing and restoring an older saved/published version, not just
  writing a new one) is a separate, real Open Cloud surface —
  `GET /place-version-history-api/v1/{placeId}/history` and
  `.../contributors` — distinct from the publish endpoint above and not
  yet wired into `rbx_cloud` at all; worth treating as its own follow-up
  rather than assuming the existing client already covers it.
- [ ] 📋 **An orientation/axis indicator in the corner of the viewport**
  (top-right, per the owner's reference) — a small `Front`/`Back`/`Left`/
  `Right`/`Top`/`Bottom` cube or similar, colour-coded per axis, toggleable
  off. Flagged honestly: this is a common convention in other 3D tools
  (Blender's own axis gizmo, SketchUp/3ds Max's ViewCube), but nothing in
  `Roblox/creator-docs`' viewport documentation describes Studio itself
  having one — the closest documented feature is the unrelated
  "Visualization Options" menu in the same corner (UI overlays, light
  sources, physics/pathfinding visualization, scroll speed). Treat this as
  a genuine rbx-native addition, not a claim that it matches Studio.

#### Properties panel — remaining type editors
- [ ] 📋 **Layout/UX pass on the panel itself**, separate from the
  per-type editor work below. Reported directly from real use, not yet
  checked against creator-docs or a real Studio instance the way this
  roadmap's other Studio-parity claims are (worth doing before assuming
  what "logical" ordering actually means there — the "Studio fallback"
  Vinegar/Wine workaround elsewhere in this document is one way to check):
  category ordering doesn't read as sensibly grouped as real Studio's own
  panel does. A few smaller, self-contained papercuts worth folding into
  the same pass rather than filing separately: rows feel visually tight
  (more padding would help), and a numeric field's own value can get
  clipped by the input's own width instead of staying fully legible.
- [ ] 📋 `NumberSequence`/`ColorSequence` — a real curve/gradient editor
  widget (keypoints along a timeline, draggable), not just the read-only
  text these currently fall back to. The most valuable of the types
  explicitly left as text tonight, given how much of the GUI/particle work
  above depends on authoring these comfortably.
- [ ] 📋 `Rect`, `PhysicalProperties`, `Font` — smaller, same "still
  read-only text" status.
- [ ] 📋 **Widgets that better match how Studio actually renders specific
  types**, rather than a generic fallback — verified against the real API
  dump and the current code, not assumed:
  - `CFrame` — today's editor is position-only
    (`crates/rbx_studio/src/properties.rs`'s `Variant::CFrame` arm reuses
    the plain `X`/`Y`/`Z` vector fields; there is no way to edit rotation
    at all). Studio splits
    a part's placement into a `Position` row and a separate `Orientation`
    row (`X`/`Y`/`Z` in degrees, Euler XYZ) instead of exposing the raw
    matrix — worth matching that split for both `BasePart` (whose
    `Position`/`Orientation` are already ordinary properties) and any
    other class with a genuine `CFrame`-typed property (`Attachment`,
    `Motor6D.C0`/`C1`, `Camera.CFrame`, …), which today only get the same
    position-only treatment.
  - `BrickColor` — rendered as literal text today
    (`format!("BrickColor({number})")`); Studio shows a colour swatch plus
    a picker of Roblox's named brick colours, not a raw palette index.
  - General pass: go through the API dump's actual type/category coverage
    (`assets/API-Dump.json`, kept current by the daily sync) rather than
    relying on memory for how each Roblox type is conventionally shown, the
    same discipline `AGENTS.md` asks for lighting/material claims.
- [ ] 📋 **"Freeze"/"Apply" a `MeshPart`'s rotation** — zero out
  `Orientation` while leaving the object's *visual* placement unchanged,
  the Blender "Apply Transform" equivalent. A real, well-read devforum
  request
  ([`allow-to-reset-the-orientation-of-a-part-to-000-and-keep-the-object-at-its-current-rotation`](https://devforum.roblox.com/t/allow-to-reset-the-orientation-of-a-part-to-000-and-keep-the-object-at-its-current-rotation/1301322),
  6 replies): developers hit this after importing or hand-rotating
  geometry, then write brittle scripts with per-object rotation
  exceptions because a part's local axes no longer line up with its
  visual orientation. Real Studio has no built-in answer either — the
  thread's own workarounds are all userland (an invisible zero-rotation
  `PrimaryPart` "hitbox", or juggling `PivotOffset` from the pivot tools
  above) — so this would be a genuine rbx-native addition, not Studio
  parity, and it must stay format-honest to remain one: a primitive
  `Part` (Block/Ball/Cylinder/Wedge) has no vertex data of its own to
  rotate into — its shape comes from `Class.Enum.PartType`/`Size` alone —
  so "freezing" one is only meaningful once it's a `MeshPart`. Doing this
  right for a `MeshPart` means generating a genuinely new mesh (rotated
  vertices baked in, written in the same real `.mesh` binary format
  `rbx_mesh` already reads/writes) and uploading it as a real Roblox asset
  through the existing `rbx_cloud` Open Cloud client, then repointing
  `MeshPart.MeshId` at it — never inventing an in-place, non-standard way
  to store rotated geometry, which would make the saved `.rbxm`/`.rbxl`
  fail to open correctly, or open but render wrong, in real Studio. Worth
  noting real Studio can't do this at all to an already-uploaded mesh
  asset it doesn't own the source file for; this project, which controls
  its own import pipeline end to end, genuinely can.
- [ ] 📋 **Hide properties Studio itself never shows.** The API dump carries
  real per-property `Tags` (confirmed by inspection: `Hidden`, `Deprecated`,
  `ReadOnly`, `NotReplicated`, e.g. `BasePart.Position`/`Orientation` are
  tagged `Hidden` since Studio only exposes them through the dedicated
  Position/Orientation UI described above, not as a raw property row) and a
  `Serialization` field (`CanLoad`/`CanSave`) that real Studio's Properties
  panel uses to decide what to list at all. `rbx_reflection`'s
  `PropertyDescriptor` doesn't capture any of this yet (`name`/`value_type`/
  `category` only, see `crates/rbx_reflection/src/class.rs`) — today every
  property with a value shows up, including ones real Studio would never
  display. Needs threading `Tags`/`Serialization` through the reflection
  database and filtering (or at least visually distinguishing read-only
  rows) in the Properties panel to match.

#### Animation
Two genuinely different feasibility tiers here, easy to conflate — verified
against `Roblox/creator-docs` rather than assumed:

- [ ] 📋 **Authoring and playing back a `KeyframeSequence` built locally in
  this editor** — reachable, not blocked by any proprietary format.
  `KeyframeSequence`/`Keyframe`/`Pose` are ordinary Instances (`Pose.CFrame`
  drives the matching `Motor6D.Transform`, `Motor6D`'s `Part0`/`Part1`/
  `C0`/`C1` come from the plain `JointInstance` base class) — exactly the
  kind of object-reference-and-CFrame data `rbx_binary`/`rbx_xml` already
  parse for everything else. An `Animator` playing a locally-built
  `KeyframeSequence` against a rig's `Motor6D` joints is real, scoped work
  (interpolation, easing styles/directions, weight-blending), not a
  reverse-engineering project.
- [ ] 📋 **An actual Animation Editor UI**, distinct from the playback
  engine above: a rig-aware timeline, a keyframe list per animated joint,
  pose editing directly in the viewport (move/rotate a limb, insert a
  keyframe from its current pose), and easing-style/direction pickers —
  the tool a builder would actually use to produce the `KeyframeSequence`
  the engine work above plays back. Shares real UI groundwork with the
  `NumberSequence`/`ColorSequence` curve editor above (both are
  "keypoints along a timeline" widgets); worth scoping the shared
  timeline/keyframe-list component once, not building two.
- [ ] ⚠️ **Playing back an already-published `Animation.AnimationId`** (one
  fetched from Roblox's catalog by asset id, the common case for any rig
  using a stock/marketplace animation) — genuinely uncertain, likely in
  the same risk category as CSGMDL. Roblox's own `KeyframeSequence` docs
  say recovering the sequence from a published `AnimationId` requires
  `AnimationClipProvider:GetAnimationClipAsync()`, a client-side
  conversion call — not a direct asset download. That's evidence (not
  proof) that the raw asset Roblox's CDN actually serves for a published
  animation is a separate, likely non-`.rbxm` format this project would
  need to reverse-engineer from scratch, the same category of risk as the
  CSG mesh format. Needs its own investigation pass before committing to
  it — don't assume it's as easy as the local-authoring case above just
  because they sound like the same feature.

#### CSG
- [x] 🚧 Legacy union/negate parts reconstruct the real constituent
  geometry via a from-scratch CSG boolean.
- [x] **Give each of a failed-CSG union's recovered fallback pieces its
  own identity.** Every piece now carries a `scene::PartId` of its own —
  the union's referent plus its position in the operation tree's additive
  order, which the asset's bytes alone decide, so moving or recolouring a
  union cannot renumber them. `Scene::resync_part` re-derives the pieces
  from the boolean `load::Resident` already carved and patches each in the
  slot it had, instead of refusing; `Rebuild::Union` is gone, and so is
  the reload a union edit used to cost (24.6 ms to 2.1 ms first frame
  readable on `FindTheCode.rbxl` — see `BENCHMARKS.md`). The union stays
  one thing to select, outline, click and cast a shadow from: it is
  placed, and its pieces are not.
- [ ] 📋 `MeshData`/CSGMDL (Roblox's own baked union result format) — see
  [Explicitly impossible](#explicitly-impossible-without-robloxs-engine),
  deliberately not attempted; the from-scratch boolean above is the
  intended long-term answer, not a stopgap.

#### Terrain
- [ ] 📋 Voxel terrain storage (`Terrain.SmoothGrid`) — no work started.
  Roblox documents the general chunk/RLE storage approach in a 2017
  engineering post but not an exact, current binary spec; this is the
  highest-risk reverse-engineering item on the whole roadmap if it's ever
  picked up, and everything below depends on it existing first.
- [ ] 📋 **The Terrain Editor's own tools**, once voxel storage exists —
  checked against `Roblox/creator-docs`' `studio/terrain-editor.md` for
  the real toolset rather than assumed:
  - **Create tab**: **Import** (heightmap + optional colormap applied to a
    region), **Generate** (procedural terrain within a region), **Clear**.
  - **Edit tab**: **Select**, **Transform**, **Fill**, **Sea Level**,
    **Draw**, **Sculpt**, **Smooth**, **Paint**, **Flatten** — `Paint`
    specifically is the material-per-voxel tool (see materials below),
    the rest shape the terrain geometry itself.
  - Real Roblox terrain materials (Grass, Rock, Sand, Water, Mud, …, all
    already implemented as `rbx_materials` texture packs for ordinary
    parts per "What's been implemented") need to be paintable onto voxels
    specifically, not just available for `BasePart.Material`.

### Editor
- [ ] 📋 **The viewport goes black, and the render thread's stats stop
  updating, after a full scene reload triggered by a non-interactive,
  scripted run** (`RBX_STUDIO_RUN`/`RBX_STUDIO_EDIT`-driven screenshot
  automation) — reproduced on a plain, unmodified build too, so no
  particular fast-path change caused it. Not yet confirmed whether real
  interactive use (a human editing live) hits the same thing; needs its
  own investigation of the render thread's state right after
  `Headless::reload`.
- [ ] 📋 Attributes editor (custom `Instance` attributes, distinct from
  built-in properties) — a real, commonly-used modern Studio feature, not
  currently scoped anywhere.
- [ ] 📋 Tags editor (`CollectionService`) — same gap.
- [ ] 📋 **A dedicated UI-editing mode for `StarterGui`.** Today the
  viewport is always the 3D `Workspace` scene; editing a `ScreenGui`'s
  layout means selecting its descendants through the Explorer tree alone,
  with no direct on-screen manipulation and a real risk of misclicking
  into a `Workspace` part instead of the UI element you meant to touch. A
  separate 2D editing surface (real Studio's own "UI Editor" mode is the
  reference point) that shows only the selected `ScreenGui`'s layout,
  scaled to a chosen device resolution, with drag/resize handles on
  `GuiObject`s directly — genuinely separate from accidentally nudging a
  `BasePart` in the 3D view. GUI rendering is now reasonably complete
  (see "What's been implemented" → Renderer → GUI), so there is something
  to edit interactively.
- [ ] 📋 **3D asset import and round-trip through Roblox**, i.e. import a
  local `.fbx`/`.obj`/`.gltf` (drag-and-drop or
  `Insert > Model/Mesh/Image`), upload it to Roblox as a real asset via
  Open Cloud (the same
  client `rbx_cloud` already has for places), and display the result in
  the workspace as an ordinary `MeshPart` — matches how Studio's own
  Importer actually works (you can't reference a mesh in-game without it
  being a real Roblox asset first). Budget check against
  `Roblox/creator-docs`' `art/modeling/specifications.md`: **individual
  meshes can't exceed 20,000 triangles** (avatar body parts have their own,
  separate budgets — see rig support below); worth validating and warning
  on import rather than letting an oversized mesh fail silently or only at
  upload time.
- [ ] 📋 **Rig / avatar insertion**, matching Studio's real **Rig
  Generator** tool (checked against `studio/rig-builder.md` and
  `avatar/character-bodies/specifications.md` rather than assumed — some
  of the originally-requested names don't map onto real, current Roblox
  terminology, corrected below):
  - **Rig type**: legacy **R6** (6 mesh objects) or **R15** (15 mesh
    objects plus a full joint set) — R15 is required for layerable
    clothing/accessories, R6 has a more limited motion range. Both are
    real, current, documented options.
  - **Body shape**: **Masculine** or **Feminine**, for either rig type.
  - **Body scale**: Roblox's own three standards are **Classic** (the
    original blocky proportions — this is what "block avatar" maps to,
    not a 2012-specific variant), **Rthro Normal**, and **Rthro Slender**
    (both realistic-proportion scales, selected via **Rig Type > Rthro**
    in the real Importer) — not the "mesh avatar (2012/2016)" split
    originally guessed; creator-docs doesn't document body scale by
    release year, it documents it by these three named standards.
  - **Which character to spawn**: a generic mannequin (any rig
    type/shape/scale combination above with no specific identity), or —
    genuinely uncertain, flagging rather than assuming — the signed-in
    user's *own* avatar, including their actual equipped characterization
    and accessories. Real Studio's Rig Generator only documents inserting
    generic pre-built rigs; pulling a specific account's real avatar
    configuration would need Roblox's avatar/catalog APIs
    (`Class.Players:GetCharacterAppearanceInfoAsync`-equivalent data over
    Open Cloud or the public avatar API) — needs its own investigation
    pass on what's actually exposed for a third-party tool before
    committing to it, likely in the same risk tier as other
    account-data-dependent items on this roadmap.
  - Whichever combination is chosen, insert as a real character `Model`
    (matching Studio's own output: correct joints/`Motor6D`s and
    `Class.Humanoid` so the rig is immediately animatable — ties directly
    into the Animation work above) — either built locally from known
    proportions/meshes, or, if a specific official asset id is the more
    faithful source for a given rig, imported directly as a real `.rbxm`
    the same way any other asset import works.
- [ ] 📋 **Native Argon integration, not Rojo.** Argon (`argon-rbx/argon`,
  Apache-2.0, open source) is the preferred target — confirmed its project
  format deliberately matches Rojo's (`*.project.json`/
  `default.project.json`, the same `*.meta.json` and
  `*.server.lua`/`*.client.lua`/`*.model.json` conventions), so file-tree ↔
  DOM import/export and `sourcemap.json` generation (for `luau-lsp`) is
  effectively the same work either way — implement it once against that
  shared format. What's genuinely Argon-specific and *not* yet confirmed:
  its live two-way sync protocol between its CLI and its required Studio
  plugin isn't documented publicly (a `msgpack-luau` repo in the same org
  hints at the transport, unconfirmed) — needs its own investigation
  before committing to real-time sync compatibility with Argon's actual
  plugin; the file-format side can start now regardless.
- [ ] 📋 **Wally package manager, built in.** Wally (`UpliftGames/wally`,
  MPL-2.0) is the de facto Luau/Roblox package manager — a `wally.toml`
  manifest, a registry index git repo, packages installed as
  `ModuleScript`s under a `Packages`/`DevPackages` folder that
  `rbx_binary`/`rbx_xml` already read and write like any other instance
  tree. Native support means: an `Install`/`Add package` action in
  `rbxstudio` that shells out to (or reimplements) Wally's resolver and
  writes the resulting `Packages` tree straight into the open place's DOM,
  instead of requiring a separate CLI step and a project re-sync. Natural
  pairing with the Argon/Rojo file-tree work above, since Wally-managed
  packages live in the same kind of synced folder structure.
- [ ] 📋 **Native Git integration** — a real panel in `rbxstudio` (diff view,
  stage/commit, branch switch), not relying on the user's own external git
  client. Not scoped in any detail yet.
- [ ] 📋 **Colour-coded Explorer folders.** Another devforum request from
  the same category ("Colored folders!") — let a `Folder` (and perhaps any
  instance) carry a colour tag shown as a tint on its Explorer icon/row,
  purely a local editor convenience (there's no such real Roblox
  `Folder` property, so this would need to live in rbx-native's own
  settings/metadata, not the saved place file, to avoid inventing a fake
  property that would confuse a real Studio session opening the same
  place).
- [ ] 📋 User settings file (service visibility defaults,
  default quality, sandbox naming for Play) beyond what's already
  persisted — and, worth folding into the same effort rather than treating
  separately, **exposing the renderer's calibration constants as real
  settings** instead of hardcoded Rust values (`SUN_BASE`,
  `ATMOSPHERE_DENSITY_SCALE`, `PLASTIC_SPEC_STRENGTH`, the movement-easing
  time constant, the quality-level bands) — every one of these was tuned
  empirically tonight against specific real captures, and a place with
  different lighting conditions may want to nudge them without a rebuild.
  Everything this bullet and the next few settings-shaped items describe
  needs an actual screen to live in — see **Studio Settings screen**
  below, which is that screen.
- [ ] 📋 **A Studio Settings screen**, matching real Studio's own
  `File > Studio Settings` (`Alt`/`⌥`+`S`) rather than leaving every
  preference above as a value nothing in the UI ever shows or changes.
  Real Studio's dialog is organized into sections; this project's
  equivalent doesn't need to match that organization exactly, but should
  cover at least: free-camera mouse sensitivity (today a hidden CLI flag —
  the movement-smoothing pass explicitly left this for later) and other
  camera/control feel settings; the renderer calibration constants and
  quality/service-visibility defaults from the settings-file item above;
  and the Auto-Recovery interval from the autosave item below, which real
  Studio's own docs place inside this exact dialog. **Keyboard shortcuts**
  are real Studio's own separate `File > Customize Shortcuts` screen
  (view and rebind any hotkey) — closely related, same File-menu
  neighbourhood, but its own screen in real Studio rather than a tab of
  Studio Settings, worth keeping distinct here too rather than merging
  the two into one dialog Studio itself doesn't have.
- [ ] 📋 **A "Beta Features" toggle**, matching real Studio's own
  `File > Beta Features` (experimental features switched on individually,
  applied after a restart). Directly useful for a fast-moving project like
  this one: a real, in-app way to ship a half-finished feature switched
  off by default instead of either blocking a merge on it being complete
  or shipping it fully live before it's ready.
- [ ] 📋 **Autosave and crash recovery**, matching real Studio's own
  Auto-Recovery (`File > Studio Settings` → Studio tab → Auto-Recovery;
  saves on an interval, typically every 5–10 minutes and configurable down
  to 1–2; recovered files reachable afterwards via
  `File > Advanced > Open Auto Saves`). Today's save is manual (`Ctrl+S`)
  only, with nothing kept if the editor crashes or is killed first. Worth
  deliberately avoiding real Studio's own known complaint here rather than
  reproducing it: a recovered file there loses its place/universe link,
  which this project could sidestep since a place here is just a local
  file path to begin with, nothing tying it to a remote id the way a
  crash-recovered copy would need to reconstruct. Distinct from, and a
  local complement to, the **remote** Open Cloud place-version-history
  item under "Save/Publish to Roblox from the editor UI" above — that one
  is versions Roblox's servers already have; this one is unsaved local
  work surviving a crash before anything was ever published at all.
- [ ] 📋 **A Home/launcher screen**, replacing today's file-path-only
  launch (`rbxstudio <file.rbxl>`) with something closer to real Studio's
  own start screen. Real Studio's current one has three real, documented
  areas: **New** (starter templates — Baseplate and Flat Terrain cover the
  minimum useful set; real Studio's own list is much longer and keeps
  growing, worth treating as a starting point rather than a fixed target),
  **My Games**/experiences (a grid or list of places, with thumbnails),
  and **Recent** (recently opened places). Sorting and grouping by
  personal vs. group-owned experiences is real Studio behavior too, but
  its exact current UI wasn't confirmed here — Roblox's own start screen
  is itself reportedly mid-redesign — worth checking against a real
  Studio instance rather than guessing at specifics not to be found in
  today's docs. One thing **not** to copy: real Studio has no account
  -switcher at all, a genuine, well-known pain point for anyone who
  manages more than one Roblox account — if this project builds one, it's
  an rbx-native improvement over Studio, not parity with it, and worth
  being honest about which it is rather than presenting it as matching an
  existing Studio feature. No existing open-source tool (Rojo included)
  was found with a comparable multi-project launcher to learn from either
  — this would be a genuinely original piece of UI design for this
  project, not an adaptation of prior art.
- [ ] 📋 **A first-run setup wizard for the Open Cloud API key** this
  project already depends on for Save/Publish (see above) — rbx-native's
  own onboarding need, not a Studio-parity item, since real Studio never
  asks for one at all (it authenticates by an ordinary Roblox account
  login this project has no way to perform, not being an authorized
  first-party client). Walk the exact real steps rather than a bare
  "paste your key" field: the Creator Dashboard's API Keys tab
  (`create.roblox.com/dashboard/credentials?activeTab=ApiKeysTab`) →
  Create API Key → name it → System = **Places API** → scope to one
  specific experience (or knowingly leave it unrestricted) → choose which
  read/write operations it grants → optional IP restriction and
  expiration → Save & Generate — ending with the generated key pasted
  into this project's own settings. A deep link straight to that
  Dashboard URL, opened from inside the wizard, saves hunting for it.
- [ ] 📋 **A Game Settings dialog**, matching real Studio's own (`Home` tab
  → Game Settings, checked against `studio/experience-settings.md` rather
  than assumed) rather than requiring every place-level setting to be
  edited as a raw DOM property through the Properties panel or the
  Command Bar. Real Studio's current tabs: **Basic Info** (name,
  description, thumbnails), **Permissions** (who can access/edit),
  **Monetization**, **Localization**, **Avatar** (scaling/clothing
  overrides), and **Communication** (voice chat, camera-driven avatar
  animation). Only the subset Open Cloud's own APIs actually expose
  remotely can realistically read/write a live experience's real
  settings; the rest would only ever apply to instance properties already
  in the local place file, worth being clear about which is which rather
  than implying the dialog reaches further than it can.
- [ ] 📋 **A scriptable command-line interface for `rbxstudio` itself.**
  Real Studio has one (`studio/command-line-interface.md`): launch flags
  to open a specific place/version at a specific task
  (`--placeId`/`--universeId`/`--task EditPlace`), focus a given instance
  on load (`--targetInstanceId`), and pipe verbose logs to stdout. Real
  Studio's version is inherently tied to Roblox's own published-place
  identifiers, which doesn't map onto this project's local-file-first
  workflow — the useful equivalent here is flags that open a **local**
  `.rbxl`/`.rbxm` at a given path, optionally focused on a given instance
  by name/path and optionally running a given `.luau` script against it
  headlessly (the existing `rbxlua` CLI runner already does the headless-
  script half; this is about `rbxstudio` itself gaining the same
  scriptability for its GUI session). Useful for CI screenshots, repeat
  -testing a specific scene, and tooling that wants to drive the editor
  without a human clicking through the Explorer first.
- [ ] 📋 **An MCP server exposing the live editor session**, matching the
  shape (not the exact tool surface) of Roblox's own built-in Studio MCP
  server (checked against `studio/mcp.md` rather than assumed — real,
  current, `stdio`-transport, ships in Studio itself, lets an AI client
  read/write scripts, run Luau, inspect the DataModel, and drive
  playtesting). Most of what it exposes is a thin protocol wrapper around
  capabilities this project already has or already plans: `script_read`/
  `multi_edit`/`script_grep` over the script editor above,
  `execute_luau` over the existing Command Bar's DataModel access,
  `inspect_instance`/`search_game_tree` over the Explorer/Properties
  panel's existing DOM access, `screen_capture` over the viewport, and
  `start_stop_play`/`get_console_output` over the sandbox Play workaround
  and Output dock. Two sub-capabilities are **not** reachable the same
  way: `search_asset`/`insert_asset` depend on Roblox's Creator Store/
  Creator Inventory catalog — the same
  [Toolbox/marketplace parity](#explicitly-impossible-without-robloxs-engine)
  wall as everywhere else in this document, arbitrary third-party assets
  aren't fetchable — and `generate_mesh`/`generate_material` are AI
  content generation, covered separately under
  [Possible via a workaround](#ai-content-generation) below. Whatever this
  project exposes over MCP is still bound by the same file-format rule as
  every other write path in this list: an MCP-driven edit has to go
  through the same real-property DOM mutation any other editor action
  does, not a shortcut that could write something a saved place file
  can't actually represent.

### Play / Test workflow
- [ ] 📋 The sandbox-place design (private per-developer place, injected
  probe scripts, a tunnel back to the editor) is fully designed — see
  [Possible via a workaround](#play--test-a-private-sandbox--probe) — but
  not implemented in code yet.
- [ ] 📋 Wiring the Output dock to real script `print`/`warn`/`error` and
  session events (join/leave messages and the like) once a sandbox session
  is running — depends on the sandbox above existing first.
- [x] 🚧 **Routing app-level warnings into the Output dock, independent of
  Play.** `OutputLog::push_warning`/`Feedback::Warning` exist now, and
  asset-fetch/decode failures reach the Output dock from a place's initial
  load and every reload — both the plain fetch-failure case and the
  silent-fallback-to-default-texture case, since both already produced the
  same warning string inside `crates/rbx_viewer/src/assets.rs`'s worker
  pool. The path: `Headless::drain_warnings` → the render thread's
  `Ready.warnings` → `WorkspaceView`'s `AssetWarnings` event → `Shell`.
  Warnings surface continuously (every render-thread tick), which
  supersedes the original ask for an explicit `Ctrl+S`-triggered flush —
  nothing is ever left pending by the time a save happens. Still open: the
  four live-render call sites (`renderer::particles`/`trail`/`beam`/
  `gui::atlas`) still show nothing for a texture that will not resolve —
  they no longer fetch one themselves (the loader does, once, and answers
  them), so what is left is routing that single answer's warning to the
  dock rather than dropping it; no dedicated
  "Warnings" `OutputFilter` bucket; no distinct visual marker for a
  warning row versus a successful Command Bar run.
- [ ] 📋 **Output window: real Studio's filter/display feature set**,
  checked against `studio/output.md` rather than assumed. Only part of
  this depends on the sandbox above — the rest is buildable against what
  the Command Bar and app warnings already put in the dock today:
  - **Not sandbox-dependent, reported directly from real use**: every
    Command Bar run's own immediate feedback shows twice today — once in
    the small label `command_bar::Feedback` renders above the input box,
    and again as a permanent row in the Output dock once
    `shell::command::run_command` logs it — instead of the dock being the
    one place to look, the way Studio's own Output window is. Rows also
    carry no timestamp, and only distinguish error from everything else
    (a plain `✕`/`✓` marker, `danger` color on error only) even though
    `command_bar::Feedback` already has a distinct `Warning` variant
    (`OutputEntry`/`Feedback`, `shell/output.rs`) — a pushed warning renders
    identically to a successful run. Real Studio's Output window docs
    confirm both gaps are real, specific features to match rather than
    invent: a **Show Timestamp** toggle prints a per-row timestamp in
    `HH:MM:SS.SSS`, and its four message kinds each get their own color —
    `print` in the default/black text color, `warn` in orange, `error` in
    red, and `TestService.Message` in blue (this last one has no
    equivalent here yet) — each pairing naturally with its own icon
    (error/warning/info/output) rather than the current binary marker.
  - **Sandbox-dependent**: filtering by **context** (`Client`/`Server`/
    `User Plugin`) only means something once the sandbox's client/server
    split exists to produce it; **Show Context** and **Show Source**
    (script name + line number) toggles are the same story, since neither
    a Command Bar run nor an app warning carries a script/line origin
    today. Free-text search over the log, and whether logged tables show
    expanded by default, apply to both halves equally.
- [ ] ⚠️ **Device Simulator equivalent** — real Studio's tool
  (`studio/device-simulator.md`, itself currently in beta on Roblox's
  side) previews an experience's UI at a chosen phone/desktop/console/
  headset screen size, pixel density, and input mode. The sandbox
  workaround's mirror `LocalScript` (see
  [Possible via a workaround](#play--test-a-private-sandbox--probe))
  already reports real client state back over a `RemoteEvent`, which is
  the right channel for something like this in principle — but whether a
  `LocalScript` can *legitimately* override what the real client reports
  for its own viewport size/platform (as opposed to merely reading
  `Class.Workspace.CurrentCamera.ViewportSize` and similar, which reflect
  the actual window) is genuinely unverified, not assumed here as
  working. Needs an actual doc/API check on what, if anything, a script
  can override versus only observe before this is more than an idea; real
  Studio's own simulator works by substituting the whole render surface at
  the engine level, which this project has no access to.
- [ ] ⚠️ **Network Simulator equivalent** — real Studio's tool
  (`studio/network-simulator.md`) adds configurable latency/jitter/packet
  loss independently to inbound and outbound playtest traffic. More
  directly reachable than Device Simulator above: the sandbox design's own
  Cloudflare-tunnel relay (see
  [Possible via a workaround](#play--test-a-private-sandbox--probe)) is
  already the one chokepoint all probe traffic passes through, so
  delaying/dropping batched diffs and long-poll responses there
  approximates the same effect without needing any Roblox-side cooperation
  at all — this project already owns the transport real Network Simulator
  is throttling a lower-level equivalent of.
- [ ] ⚠️ **Controller Emulator equivalent** — real Studio's tool
  (`studio/controller-emulator.md`) emulates gamepads, VR controllers,
  handhelds, and TV remotes, injecting real input events into a playtest.
  Genuinely uncertain, flagged rather than assumed: unlike the two above,
  this needs synthetic input to reach `Class.UserInputService` *inside the
  real Roblox client process* the sandbox launches — there's no confirmed
  public API for a script (probe or otherwise) to fabricate a gamepad
  `InputBegan`/`InputChanged` event the same way the engine does for real
  hardware. Needs its own investigation pass (is there *any* legitimate
  injection point, e.g. an OS-level virtual gamepad the real client would
  pick up as genuine hardware, similar in spirit to the OS-level mouse
  capture this project's own free camera already does) before assuming
  it's buildable at all, let alone to what fidelity.

### Plugins
- [ ] 📋 rbx-native's own plugin API (Luau via `mlua` sandbox, Rust
  compiled to WASM and run under `wasmtime` for anything that needs to be
  both native and actually sandboxed) — distinct from Roblox plugin
  compatibility, see below.
- [ ] 📋 A allow-listed subset of real Roblox plugin APIs
  (`Plugin`/`PluginToolbar`, `Selection`, `ChangeHistoryService`,
  `HttpService:RequestAsync`) for plugins that don't need Roblox's own UI
  system — see [Explicitly impossible](#explicitly-impossible-without-robloxs-engine)
  for what a plugin fundamentally can't do here.
- [ ] 📋 **The `Handles`/`*HandleAdornment`/`Selection*` Instance family**
  (`Handles`, `ArcHandles`, `BoxHandleAdornment`, `SphereHandleAdornment`
  and siblings, `SelectionBox`, `SelectionSphere`) — real, current,
  documented classes a script or plugin instantiates to draw interactive
  3D handles and outlines directly in the viewport, independent of this
  project's own Move/Scale/Rotate gizmo. Worth building since it's a
  real, general plugin capability (checked directly against
  `reference/engine/classes/Handles`/`BoxHandleAdornment`/
  `SphereHandleAdornment`/`SelectionBox` rather than assumed), **not**
  because it's how **Building Tools by F3X** draws its own tools — it
  isn't: F3X's actual source
  (`F3XTeam/RBX-Building-Tools`, `Libraries/Handles.lua`) renders plain 2D
  `ImageButton`s inside a `ScreenGui`, hand-projected from 3D to screen
  space, the same ordinary GUI machinery this project already renders
  (see "What's been implemented" → Renderer's GUI containers) — worth not
  conflating the two mechanisms just because both are called "handles."
  `SelectionBox` in particular carries its own `LineThickness` property,
  directly relevant to the selection-outline thickness question raised
  under "What's planned" → Renderer's gizmo papercuts item.
- [ ] 📋 **A floating, non-dockable plugin widget surface.** A real,
  well-read devforum request
  ([`allow-floating-non-resizeable-widgets`](https://devforum.roblox.com/t/allow-floating-non-resizeable-widgets/4193893),
  read in full): real Roblox plugins are limited to `DockWidgetPluginGui`
  panels docked into Studio's own layout, with no way to render a floating
  overlay (a cursor-anchored context menu, a `MaterialPicker`-style
  popover) the way Studio's own first-party UI can. Explicitly **not**
  Studio parity — Roblox has never shipped this, so this would be an
  rbx-native-only extension of the plugin API from the item above, not a
  claim about matching real Studio's `Plugin`/`PluginGui` classes. Doesn't
  touch the saved place file at all (this is plugin-rendered UI chrome,
  not DOM data), so it carries none of this document's usual Roblox
  -compatibility risk.
- [ ] 📋 **Expose the built-in colour pickers to the plugin API** —
  another well-read devforum request
  ([`allow-plugins-to-use-color-picker`](https://devforum.roblox.com/t/allow-plugins-to-use-color-picker/63257),
  24 replies, one of the higher-engagement threads in the whole category):
  real Roblox plugins have to build their own `Color3`/`BrickColor` picker
  UI from scratch today, duplicating a widget every user already knows
  from the Properties panel. Since this project's own real Properties
  panel already needs a `BrickColor` swatch-and-picker widget (see above),
  exposing that same widget to the (also already-planned) plugin API as a
  `plugin:PromptColor3()`/`plugin:PromptBrickColor()`-shaped call is
  mostly reuse, not new UI work. Same caveat as the item above: Roblox
  hasn't shipped this on its plugin API either, so frame it as this
  project's own plugin-API addition, not asserted Studio-plugin parity.
- [ ] 📋 **Expose debugger control to the plugin API.** A third well-read
  devforum request
  ([`let-plugins-access-debuggermanager`](https://devforum.roblox.com/t/let-plugins-access-debuggermanager/193584),
  16 replies): real Roblox's `DebuggerManager` is reachable from the
  Command Bar but not from ordinary plugins, which blocks a whole category
  of plugin (the thread's own motivating case: a VS Code user wants a
  plugin that mirrors external editor breakpoints into Studio's debugger
  instead of maintaining two separate breakpoint sets). Depends on the
  Script debugging item under "Script authoring" existing first —
  breakpoint/Watch/Call-Stack state lives in the editor session, never the
  saved place file, so exposing it to a plugin carries the same "no file
  -format risk" property as the two items above.

### Platform: Windows
- [ ] 📋 Mouse capture in the free-flight camera — implemented for X11
  only today (`x11rb`/XFixes); needs a Win32 `ClipCursor`/`SetCursorPos`
  backend.
- [ ] 📋 A Windows-native texture fallback path
  (`%LOCALAPPDATA%\Roblox\Versions\<version>\content\textures\`, read
  directly off a real local Roblox/Studio install) — the Linux equivalent
  (an optional Sober Flatpak fallback) already exists and follows the same
  rule: never the default source, `setup.rbxcdn.com` stays primary.
- [ ] 📋 A first real build on Windows, and CI coverage for it — nothing
  here has ever been verified on the platform beyond code reading.

### Tooling / CI
- [x] Daily API-Dump sync (`.github/workflows/sync-api-dump.yml`).
- [ ] 📋 A job that rebuilds against the current Studio version and
  compares parsed output to reference dumps, to catch a format drift
  before a user does.
- [ ] 📋 Cross-platform build CI (Linux is exercised constantly by
  development itself; Windows and macOS have none).

## Explicitly impossible without Roblox's engine

These aren't missing features — they require Roblox's own closed-source
game engine (rendering, physics, replication, anti-cheat) to exist at all,
and no amount of reverse engineering changes that:

- **Playing or testing a live game with real physics and client/server
  replication.** rbx-native can never have a native "Play" button in the
  way Studio does — see the workaround below for what's actually reachable.
- **Team Create** — proprietary real-time collaboration, no public
  equivalent API.
- **Toolbox/marketplace parity** — public APIs (Creator Store, Open Cloud,
  `catalog.roblox.com`) expose metadata and let you manage your *own*
  assets; none of them let you download and insert an arbitrary third-
  party asset the way Studio's Toolbox does.
- **A pixel-faithful plugin UI** — `DockWidgetPluginGui` is rendered by
  Roblox's own UI engine (`GuiBase2d`/`LayerCollector`); reproducing it
  means reimplementing that whole subsystem, which is out of scope. A
  headless, DataModel-only plugin subset is reachable instead (see above).
- **Bit-exact CSG results** (`MeshData`/CSGMDL, `PhysicalConfigData`) — an
  undocumented, version-unstable format the community's own reference
  researchers haven't fully decoded either; not worth chasing when a real
  from-scratch boolean already exists as the actual answer.
- **Physics simulation and anti-cheat** — proprietary physics engine, no
  real server authority possible from rbx-native.

## Possible via a workaround

### Play / Test: a private sandbox + probe

You can't run Roblox's engine locally, but you *can* run a real, private
Roblox place and puppet it from the editor:

1. A private place, one per developer, created once by hand in a test
   universe. "Play" = `rbx_cloud::publish_place` of the currently-edited
   place after injecting a probe (implemented client, never yet exercised
   for real).
2. Launch the real client — Sober (Flatpak) on Linux, `RobloxPlayerBeta.exe`
   on Windows, both via the `roblox://placeId=…` protocol. Launched as an
   ordinary top-level window today; embedding it borderless inside
   `rbxstudio`'s own dock, the way real Studio's own "Play" runs the
   client inside the editor window rather than a separate one, is a real,
   separate follow-up — X11 supports reparenting another process's window
   into one of this app's own (the same mechanism a panel-embedding
   taskbar or a browser's PiP window uses), but Windows would need its own
   platform-specific approach, and neither has been investigated yet.
   Genuinely optional: everything else in this plan works the same with
   the client in its own window.
3. Before publishing, inject into the in-memory DOM only (never the real
   saved file): `HttpService.HttpEnabled = true`,
   `ServerScriptService.LoadStringEnabled = true`, a probe `Script`, and a
   mirror `LocalScript` that reports client state back over a
   `RemoteEvent`.
4. A Cloudflare quick tunnel in front of a small HTTP server inside
   `rbxstudio`: the probe posts batched diffs (~200ms) and long-polls for
   commands. `HttpService` is HTTP-only (no WebSocket), ~500 req/min.
5. The sandbox place is anonymized by default — randomized name, no
   description/icon, private visibility — configurable via settings.
6. Hot-reload via `loadstring`, so iteration doesn't need republishing
   every change; one publish per session.

### Monetization testing (gamepasses, developer products) inside the sandbox

The sandbox above gets you a running game, but `MarketplaceService`'s real
purchase prompts talk to Roblox's live payment servers — not something to
trigger from a dev sandbox. The workaround is a mocking layer the game's
own scripts call instead of the native API directly:

1. **Force `RunService:IsStudio()`.** Either monkey-patch it in Luau at the
   top of the sandbox's bootstrap script (`RunService.IsStudio = function()
   return true end`), or, if working at the API-binding layer, make the
   binding always return `true` in a playtest build.
2. **A `MonetizationManager` wrapper.** Game code calls this instead of
   `MarketplaceService` directly; it checks `IsStudio()` and, in the
   sandbox, intercepts the purchase locally instead of calling
   `PromptGamePassPurchase`/`PromptProductPurchase`.
3. **A fake "Dev-Wallet".** On join, give the test player a default
   balance of fictional currency (a plain value or table); debit it
   locally when a purchase is "made".
4. **A separate DataStore scope for dev purchases** (e.g.
   `Dev_Purchases_v1`) so test transactions never touch production data.
5. **Manually drive `ProcessReceipt`.** Real Developer Products rely on
   Roblox calling the game's `ProcessReceipt` callback after a real
   purchase — since the real prompt is short-circuited, the wrapper must
   call it itself with a fabricated `ReceiptInfo` table (`PlayerId`,
   `PlaceId`, `ProductId`, a `"DEV-" .. HttpService:GenerateGUID(false)`
   purchase id, a fake `CurrencySpent`), or that callback never fires and
   the purchase never actually resolves in-game.

Not implemented yet — this is a design, not code — but it's a real,
workable path to testing a full monetization loop without touching real
accounts or real Robux, and it composes with the sandbox above rather than
needing a separate mechanism. Open question this roadmap doesn't answer
yet: whether the dev-purchase DataStore scope should live in real Roblox
DataStores (simplest, reuses the sandbox's own persistence) or a fully
local store — revisit when the sandbox itself is actually implemented.

### AI content generation

Real Studio's **Material Generator** and **Texture Generator**
(`studio/material-generator.md`, `studio/texture-generator.md`, the
latter still in beta) turn a text prompt into a paintable
`MaterialVariant` or a mesh-specific texture within seconds. Both are
backed by an image-generation model Roblox trained and hosts itself —
nothing about the *model* is reachable here, so this can never be Studio
parity in the sense of reproducing Roblox's own generations. What *is*
reachable is the same workflow shape with a different backend:

1. A prompt box in the Material/Texture picker UI (same entry points real
   Studio uses — the `Material` widget's picker popup, or a selected
   `MeshPart`) sends the user's text prompt to any external text-to-image
   API the user configures (their own key, their own choice of provider —
   this project shouldn't hardcode or default to one, the same stance the
   Inline AI code completion item under "Script authoring" above takes).
2. The returned image is saved locally and, critically for staying
   format-compatible, **uploaded as a real Roblox image asset** through
   the existing `rbx_cloud` Open Cloud client — the same path the 3D
   asset import item already uses — never embedded or referenced as a
   local file path inside the saved place.
3. The result is applied through ordinary, real properties: a generated
   material becomes a real `Class.MaterialVariant` (`BaseMaterial`,
   `ColorMap`/`NormalMap`/`MetalnessMap`/`RoughnessMap` pointing at the
   uploaded asset ids) assignable to `BasePart.MaterialVariant`; a
   generated texture becomes a real `Class.Texture`/`Class.SurfaceAppearance`
   asset reference. From a saved-file standpoint this is indistinguishable
   from a developer manually uploading custom art and wiring it up by
   hand — which is exactly the property this whole document holds every
   write path to.

Not implemented, not even designed in detail yet — recorded here as a
real, workable shape rather than a rejected idea, the same "design, not
code" status the monetization workaround above has.

### Studio fallback

A literal "Open in Studio" button (Vinegar/Wine) for anything rbx-native
genuinely can't do yet, with sync via a Rojo/Argon-style plugin. Reparenting
the real Studio window into rbx-native's own was considered and rejected —
Wine + Qt + X11 stacked together isn't worth it as an integrated Play
engine, just as an escape hatch.

### Collaboration

Two rbx-native instances editing the same place concurrently (CRDT/OT over
the DOM — a real, unbuilt project of its own) covers Linux-to-Linux
collaboration, i.e. **collaborators** in this project's own editor.
**Reflecting live edits between this editor and real Roblox Studio** — a
separate ask — is the other half: an official Studio-side plugin, in the
two-way-sync mode real Rojo's own plugin already offers
([`rojo-rbx/rojo`](https://github.com/rojo-rbx/rojo)), talking to a small
local server this project would run, the same shape Rojo/Argon already use
to keep an external editor and Studio's own DOM in sync. Neither replaces
**Team Create**, Roblox's actual real-time multi-user editing — that stays
proprietary with no public protocol to build against (see
[Explicitly impossible](#explicitly-impossible-without-robloxs-engine)) —
the sandbox above covers testing as a team instead of editing as one.

---

For more detail on anything above — what was actually tried, what was
measured, what a fix turned out to really be — see [CHANGELOG.md](CHANGELOG.md).
