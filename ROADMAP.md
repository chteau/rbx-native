# rbx-native — Roadmap

rbx-native is a from-scratch, native Linux/Windows replacement for Roblox
Studio. This document tracks what's implemented, what's planned, and — since
Roblox's actual game engine is closed-source — what's outright impossible to
match versus what can be approximated with a deliberate workaround. It's the
single source of truth for project direction; if something here looks stale,
[CHANGELOG.md](CHANGELOG.md) has the day-by-day record of what actually
landed and why.

Feasibility legend: ✅ done · 📋 planned, not
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
- [x] Per-class property defaults (`assets/reflection-defaults.json`, taken
  from rbx-dom's MIT-licensed database, since Roblox's own dump carries
  none for inherited properties). A binary save fills a property one
  instance of a class stored and another left unset with that class's
  default — infinite ones included — rather than the type's zero, which
  used to save an Explorer-inserted `Part` with `CanCollide` off and no
  size. A load normalises the several names a property can be saved
  under to one, never to a spelling Studio cannot read back (a package
  link keeps `PackageIdSerialize`).

### Scripting (`rbx_lua`)
- [x] Sandboxed Luau VM (`mlua`) with a from-scratch DataModel
  (`Instance`/`game`/`workspace`, `Vector3`/`CFrame`/`Color3`/`UDim2`/
  `NumberSequence`/`ColorSequence`/`Rect`/`PhysicalProperties`/`Font`/
  `Content`). A property the file never stored reads as Roblox's own
  default for that class rather than `nil`, and `BrickColor` carries the
  full 208-colour table the Properties panel uses.
- [x] CLI runner (`rbxlua place.rbxl script.luau [--out] [--print-changes]`)
  and an in-editor Command Bar.
- [x] **New-script templates** — the Model menu now has real
  `Insert Script`/`Insert LocalScript`/`Insert ModuleScript`/
  `Insert ModuleScript (Class)` entries (there was previously no menu item
  or shortcut to insert a script at all), each seeding the new instance's
  `Source` with a starter template instead of leaving it empty: a plain
  `print("Hello, world!")` for `Script`/`LocalScript`, a `ModuleScript`
  returning a table, and a `ModuleScript (Class)` with a `.new()`
  constructor over a metatable. The set is user-extensible now: a
  `script_templates` folder in the config directory holds one `.luau` file
  per template under `Script/`, `LocalScript/` or `ModuleScript/`
  (`script_templates.rs`), each listed in the ribbon's Script menu by its
  file name, and a `Default.luau` in a class's folder replaces the built-in
  starter every new script of that class gets.

### Renderer (`rbx_viewer`)
- [x] Lighting model reverse-engineered from Roblox's own decompiled
  shaders and calibrated pixel-by-pixel against real Studio captures: two
  lamps + sky ambient, Blinn-Phong specular, sky cubemap reflection,
  fog/`Atmosphere`, sun/moon, shadow maps for every local light —
  `SpotLight`/`SurfaceLight` from one perspective down their own cone, a
  `PointLight` from six faces into a depth array of its own
  (`renderer::shadow::point`), the shader picking the face from the major
  axis of the light-to-fragment direction — bloom/stars/
  `ColorCorrectionEffect`/`ColorGradingEffect`/`BlurEffect`/
  `SunRaysEffect`/`DepthOfFieldEffect`.
- [x] 43 official Roblox materials (`rbx_materials`) with real texture
  packs, `MaterialVariant`, `SurfaceAppearance`. `Glass` refracts what is
  behind it — the scene is copied when the opaque pass ends and a pane
  reads it back displaced along its own mapped normal, which is the
  surface detail that gives real glass its wobble (Roblox documents the
  refraction itself, and that it is dropped on mobile "due to
  computational limitations", but publishes no index or displacement, so
  the figure is this renderer's). A `ForceField` is drawn as an energy
  shell: a lattice of cells and a rim that brightens where it is seen
  edge-on, both this renderer's own rendition — see "What's planned" →
  Renderer for what of that material is still open.
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
  `Trail`, and the three preconfigured particle classes — `Fire`
  (the two emitters its docs describe, the inner one longer-lived,
  faster-rising and at `LightEmission` 1), `Smoke` and `Sparkles` — each
  read into the same emitter definition a `ParticleEmitter` produces, so
  the budget, the simulation and a Properties-panel edit all treat them
  identically. Every emitter reads `TimeScale`, and can hang off an
  `Attachment` as well as a `BasePart`; each effect draws through
  Roblox's own image for it out of the Studio content package. What the
  docs state is followed as stated, and every figure they do not publish
  is marked in the code as this renderer's own. An `Explosion` is not
  among them: it is a one-shot that plays when it is parented into the
  world, so one sitting in a place file has nothing to draw.
- [x] The 3D adornment family, from a place file: `SelectionBox` (its
  bars and, where it is not transparent, its surfaces), `SelectionSphere`
  (the documented "ring/outline in addition to a surface", the ring being
  the sphere's own silhouette faced at the camera), `SurfaceSelection`,
  `Handles` (arrows or spheres, per face) and `ArcHandles` (a ring per
  axis), and the `*HandleAdornment` shapes — box, sphere, cylinder
  (`InnerRadius` and `Angle` included), cone, line (its `Thickness` in
  pixels, as documented, where `SelectionBox.LineThickness` is in studs)
  and image. Depth-tested by default and over everything while
  `AlwaysOnTop`, with `ZIndex` ordering those among themselves, exactly as
  the docs describe. `WireframeHandleAdornment`, `ParabolaAdornment` and
  `SelectionLasso` serialize no geometry to draw from and are recognized
  as drawing nothing rather than guessed at.
- [x] `AdGui`: the ad surfaces a place holds show their own
  `FallbackImage` on the face they adorn, which is what Roblox documents
  one as showing whenever no ad is available — and no ad ever is here.
- [x] **`Highlight`** — the real class (checked against
  `reference/engine/classes/Highlight` rather than assumed), drawn the way
  the docs describe it: a **silhouette** outline around the adornee and a
  solid interior over it, each with its own `Color3` and transparency.
  `Adornee` (falling back to the parent), `Enabled`, `FillColor`,
  `FillTransparency`, `OutlineColor`, `OutlineTransparency` and both
  `DepthMode`s — `AlwaysOnTop` shows through whatever stands in front,
  `Occluded` stops where the nearest surface is not the adornee — plus
  Roblox's own documented ceiling of 255 at once. Not the box outline the
  Explorer's selection cue draws (`rbx_viewer::renderer::outline`): a
  position-only mask pass re-draws the covered geometry — unit shapes and
  downloaded `MeshPart`/`SpecialMesh` geometry alike, through the same
  buffers `renderer::shadow` instances — and a composite pass reads the
  silhouette back out of it, so a ball outlines as a circle and a mesh as
  its own polygon. Two undocumented corners are named in the code rather
  than guessed at: `LineThickness` is tagged `Hidden` in the API dump and
  has no published behaviour, so the outline is a constant width chosen to
  sit beside this renderer's existing cues; and where the scene is
  multisampled the composite reads one sample, so the highlight's own edge
  is aliased against the geometry it traces.
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
  - Text metrics match Studio: `TextSize` is the *line box* height (the
    docs: "line height equal to the `TextSize`"), the glyph em a fixed
    1/1.2 of it — measured against Studio captures of three families
    (Source Sans Pro, Gotham SSm, Fredoka One) to the pixel, which no font
    table combination reproduced. A family that lacks the requested
    weight shapes in its closest face rather than falling back to a
    system font (cosmic-text only matches an exact weight). Studio's
    synthetic bold for a weight the family lacks is not reproduced.
  - The overlay composites in encoded (sRGB) space, as Roblox does, through
    a non-sRGB view of the target: a `BackgroundTransparency` 0.5 frame
    over the scene halves the encoded pixel like Studio's, not the linear
    one.
  - A `Folder` (or any non-`GuiObject` instance) inside a GUI tree is the
    layout scope the `Folder` docs describe: its contents render against
    the nearest `GuiBase2d`, arranged by the folder's own `UILayout` if it
    has one and exempt from its siblings' layout.
  - `ScrollingFrame` (canvas, `CanvasPosition`, `AutomaticCanvasSize`, the
    scroll bars from their three images with every inset/position/
    direction property), `CanvasGroup` (`GroupTransparency`/`GroupColor3`
    over the subtree flattened to a texture, one blend for overlapping
    children), `ViewportFrame` (its parts under `CurrentCamera` — or the
    saved camera pose — with `Ambient`/`LightColor`/`LightDirection`, a
    single-light opaque+blended pass with no shadows or post, `ImageColor3`
    /`ImageTransparency`; meshes, unions and decals inside a frame draw as
    their fallback boxes), `UIPageLayout` (the current page laid out, no
    transition), `BillboardGui`/`SurfaceGui` `Brightness`/`LightInfluence`/
    `MaxDistance`/`SizeOffset`, `ScreenGui.ClipToDeviceSafeArea`,
    `ImageContent`.
  - Every property of `StarterGui` and of the 45 classes a place's GUI
    tree can hold was audited against the API dump and the docs (384
    properties): each is implemented, or documented as having no
    still-frame effect (`Active`, `Selectable`, `AutoButtonColor`, the
    selection/navigation family, `VideoFrame` playback…).
    `StarterGui.ShowDevelopmentGui` is honoured as the Studio view toggle
    the docs describe: `false` hides every screen and canvas under the
    service (a `ScreenGui` elsewhere — a copy under `Workspace`, say — is
    unaffected), toggling it in the editor re-plans live, and
    `rbxview --show-development-gui` overrides it for a player's-eye
    screenshot. The mouse wheel over a `ScrollingFrame` in the editor's
    viewport scrolls its canvas rather than the camera (viewing, not
    editing, so it never enters the undo stack). Known gaps:
    the deprecated `FrameStyle`/`ButtonStyle` skins (client assets),
    `TextDirection`/`OpenTypeFeatures`, `BillboardGui.ExtentsOffset*`.
    Several `UIStroke`s on one object draw in `ZIndex` order.
- [x] Free-flight camera (WASD + mouse look + wheel), exponentially-eased
  movement (mouse look itself stays unfiltered).
- [x] Orthographic camera mode — toggled from the Viewport dock
  (`rbxstudio`) or `rbxview --orthographic`. Flies with the same
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
- [x] **An orientation/axis indicator in the corner of the viewport**
  (top-right) — six small coloured, lettered dots (`Right`/`Left`/`Top`/
  `Bottom`/`Front`/`Back`, red/green/blue per axis, the same colours the
  Move/Rotate/Scale gizmo already uses), positioned by projecting the free
  camera's current basis (`rbx_viewer::Pose::basis`) rather than a literal
  3D cube mesh — the same flat 2D approach Blender's own gizmo actually
  draws with. Toggleable off from the Viewport dock
  (`Orientation Indicator`, next to `Orthographic`), on by default and
  persisted the same way. As flagged when this was picked up: a genuine
  rbx-native addition inspired by Blender/SketchUp/3ds Max conventions, not
  a claim that Studio itself has one. Click-to-snap-camera-to-a-face (real
  in those other tools) is deliberately not implemented — this is a static,
  informational indicator only.
- [x] **An FPS/frame-time readout**, matching real Studio's own
  performance-debugging surface rather than inventing a new one: Studio's
  `Window > Performance > Stats` toggles a debug stats overlay, and
  `Ctrl`+`F6` opens the MicroProfiler directly for a per-system frame-time
  breakdown. `rbxstudio`'s Viewport dock (a tab beside Output by default,
  so nothing is drawn over the scene) shows the render thread's
  last-measured fps and frame time beside the quality level, counted only
  while the dock is on screen, and reusing the same per-second numbers
  `workspace_view::stats` already computed to drive automatic quality
  scaling rather than a second timing mechanism. `rbxview`'s
  standalone title bar carries the same fps/frame-time reading now too,
  alongside the flight speed it already showed (it never displayed a
  quality level of its own to begin with, pinned or automatic) — always on
  rather than behind a toggle, since it is a separate binary with no menu
  to put one in. A small `FrameRate` counts redraws over the same rolling
  one-second window `rbxstudio`'s does, and the title is composed
  (`app::title`, `app::fps` in `rbx_viewer`) with the identical fps/ms
  formatting the corner label uses, once the first window closes.
- [x] **A `Model`'s aggregate bounding box** — a plain click resolves to
  the outermost enclosing `Model` (see Editor's Select bullet), which has
  no `CFrame`/`Size` of its own, so it is outlined by one world-axis
  -aligned box around every `BasePart` beneath it at any depth, nested
  `Model`s included; a container with nothing drawable under it outlines
  nothing, there being nothing to box. The Move gizmo stands at that box's
  centre and carries every part inside it by the same offset, through the
  same `transform::Targets` machinery multi-select already used, and
  `Alt`/`⌥`-click still reaches one specific part inside the model and
  gizmos it with its own oriented box. One derivation
  (`rbx_viewer::gizmo::bounds_of`) feeds the outline, the gizmo's centre
  and the Scale handles' box alike, and the Align tool's own **Selection
  Bounds** agrees with it on the world axes. Known divergence, since the
  docs are explicit: `Model:GetBoundingBox` orients Studio's box by the
  model's pivot (the `PrimaryPart`'s, or the `WorldPivot`), which matches
  world alignment only while that pivot is unrotated. The pivot itself is
  read now (the Properties panel's `Origin` row), but this box does not
  turn with it yet (see "What's planned" → Renderer's Pivot tools).
  The box is drawn through whatever stands in front of it, as Studio's is;
  the Viewport dock can ask for it to be depth-tested
  against the scene instead (`Hide Selection Box Behind Parts`, off by
  default and persisted the same way `Orthographic` is).
- [x] Legacy union/negate parts reconstruct the real constituent
  geometry via a from-scratch CSG boolean. The one piece not covered is
  `MeshData`/CSGMDL, which has its own bullet below and is deliberately
  not attempted.
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
- [x] **The `Handles`/`*HandleAdornment`/`Selection*` Instance family**
  (`Handles`, `ArcHandles`, `BoxHandleAdornment`, `SphereHandleAdornment`
  and siblings, `SelectionBox`, `SelectionSphere`) — real, current,
  documented classes a script or plugin instantiates to draw 3D handles
  and outlines directly in the viewport, independent of this project's own
  Move/Scale/Rotate gizmo. **Drawn**, from a place file: see "What's been
  implemented" → Renderer. Checked directly against
  `reference/engine/classes/Handles`/`BoxHandleAdornment`/
  `SphereHandleAdornment`/`SelectionBox` rather than assumed, and **not**
  modelled on how **Building Tools by F3X** draws its own tools — it
  isn't: F3X's actual source
  (`F3XTeam/RBX-Building-Tools`, `Libraries/Handles.lua`) renders plain 2D
  `ImageButton`s inside a `ScreenGui`, hand-projected from 3D to screen
  space, the same ordinary GUI machinery this project already renders
  (see "What's been implemented" → Renderer's GUI containers) — worth not
  conflating the two mechanisms just because both are called "handles."

### Editor (`rbx_studio`, binary `rbxstudio`)
- [x] Explorer: this project's own flat, from-scratch class icon kit
  (`assets/icons/default/dark`, 329 classes onto 142 of its 152 tiles —
  Roblox's 147-tile layout plus five of its own — spec'd in
  `assets/icons/README.md`) rasterized and painted per row
  (`class_icons.rs`), with Lucide glyphs as the fallback for anything the
  kit doesn't cover — Roblox's own sprite sheet is no longer downloaded or
  drawn anywhere in the editor. Default service filter with a "show all"
  toggle, selection with a viewport highlight, instance insert/delete.
- [x] Properties panel: real per-type widgets (checkbox, colour picker,
  enum dropdown, `BrickColor` palette, numeric fields), grouped into
  collapsible categories named as Roblox's own Properties panel names
  them, live —
  reflects DOM mutations from any source (script, undo/redo, the camera
  moving) without a manual refresh.
  - **The class's whole sheet**, built from the reflection database rather
    than from what the file happened to store, with anything the file left
    out filled from the class's defaults (see Format & parsing). Rows go by
    Studio's names (`Color`, `Size`, `Shape`) while an edit still lands
    under the name the renderer and the writer read (`Color3uint8`, `size`,
    `shape`).
  - **A multi-selection shows what it shares**: a value every instance
    agrees on reads normally, one they don't reads blank (per component,
    for a vector) or as a dash on a checkbox, and an edit applies to all of
    them as one undo step.
  - **Numeric values open like Studio's**: a `Vector3`, a `UDim2`, a
    `CFrame` is one name/value row showing the whole value (`0, 5, 0`,
    still typeable as one) with an expander that drops its components
    underneath — `Position` and `Orientation` each over their own three for
    a `CFrame`. A collapsed row builds no component fields at all. A
    bounded property (`Transparency`, `ClockTime`, a `GuiObject`'s
    `Rotation`) also gets a slider beside its field; the dump carries no
    bounds, so they are a named table (`properties::ranges`) that only
    decides how far the rail reaches.
  - **`BrickColor`**: the full 208-colour table from Roblox's docs, laid out
    as Studio's honeycomb picker and workable by keyboard. A part's
    `BrickColor` row reads the nearest palette colour off `Color` and writes
    a pick back to it, since `Color` is all Roblox saves.
  - **`Origin`**, which Studio lists under Transform: where a part's or
    model's pivot stands in the world, read as `GetPivot` reads it, and
    moved as `PivotTo` moves it when one is typed — a model's parts and its
    pivot together, as one undo step.
  - **Computed, read-only**: `Mass`, `CenterOfMass`,
    `CurrentPhysicalProperties` and the assembly's mass and centre, shown
    only where Roblox documents exactly how they are computed.
  - Rows are separated by hairlines, and property names, attribute names
    and tag chips share one left edge. The filter finds a `CFrame` row by
    its Position and Orientation fields as well as its name.
- [x] A `Variant` that may simply be absent (`OptionalCFrame`, the DOM's
  only such type — `Model.WorldPivotData`) edits through a present/absent
  checkbox above the ordinary `CFrame` editor, which is drawn only while
  there is a value to edit. Turning it off clears the value; turning it
  back on restores whatever it held rather than resetting to the origin.
  Not a Studio parity claim — real Studio never surfaces an optional
  `CFrame` at all, so the checkbox's caption says plainly what it means
  rather than borrowing a term from somewhere it isn't used.
- [x] Properties panel hides properties Studio itself never shows:
  `rbx_reflection`'s `PropertyDescriptor` carries the dump's per-property
  `Tags` and `Serialization` (`CanLoad`/`CanSave`), and the panel leaves
  out anything tagged `Hidden` or `Deprecated` — a deprecated property is
  an old spelling kept under a newer name (`className`, `Fire.size`) or
  one superseded or inert (`Sound.Pitch`, `FormFactorPart.FormFactor`),
  so its row would either duplicate the live one or do nothing. A part's
  position and rotation edit through its `CFrame` row's Position and
  Orientation fields and its `Origin` row. A property tagged `ReadOnly`,
  or one Studio never saves, shows with no edit widget — unless it is
  saved under another name: the dump reports `BasePart.Size` as
  `CanSave: false` only because a file holds it as `size`, so `Size`
  edits. `Tags` and `AttributesSerialize`, which the dump does not list at
  all, get no row either; the Attributes/Tags section is their editor.
- [x] Command Bar (Luau against the live DataModel) with an Output dock:
  run history, Clear, a success/error filter, click-to-recall a past
  command.
- [x] App-level warnings reach the Output dock without a Play session ever
  running: `OutputLog::push_warning`/`Feedback::Warning`, fed by every
  asset fetch/decode failure a place's initial load or any reload produces
  — both the plain fetch failure and the silent fall back to a default
  texture — along `Headless::drain_warnings` → the render thread's
  `Ready.warnings` → `WorkspaceView`'s `AssetWarnings` event → `Shell`.
  They surface continuously rather than on a flush, so nothing is left
  pending by the time a save happens. A warning is its own row kind
  (orange, alert icon) and its own **Warnings** bucket beside All/Output/
  Errors, which is what Studio's own window does — it "filters output by
  type, such as **Error** or **Warning**" (`studio/output.md`) — and each
  bucket holds exactly one kind, so a warning no longer doubles as
  `Output`. That includes the `ParticleEmitter`/`Beam`/`Trail`/`ImageLabel`
  textures the render passes only ever see an *answer* for: their failure
  is reduced to "no image" before a pass sees it, so the warning is carried
  out of `Loaded::resolve_effect_images` instead — the streaming loader
  already reported them through `Resident::poll`; the blocking one (the CLI
  viewer, and every test) was dropping them on the floor.
- [x] Menu bar (File/Edit/Model/View) wired to real actions where one
  exists; everything else an honestly-disabled placeholder rather than a
  button that looks functional and isn't.
- [x] The menu bar is reachable from the keyboard, which closes the last
  WCAG 2.1.1 (Keyboard, Level A) gap in this editor. **F10**, or a bare
  **Alt** tap, moves focus into it from wherever focus happens to be;
  Left/Right walk the titles and wrap; Enter, Space or Down opens the
  focused one; Escape closes an open menu, and Escape again leaves the bar
  and puts focus back exactly where it came from. It is deliberately *not*
  a Tab stop: Tab walks the editor's regions, and a fifth region everyone
  has to pass through on the way to the ribbon is not what the desktop
  convention asks for. An Alt *tap* is told apart from Alt-the-modifier —
  which this editor uses live, for the Ball/Cylinder Scale lock and for
  selection cycling — by a small state machine where anything at all
  arriving while Alt is held cancels the tap (`menu_bar::alt_tap`).
  This meant owning the bar rather than the toolkit's ready-made
  `AppMenuBar`, whose current title is a private field with no way in from
  outside; each dropdown is still the toolkit's own `PopupMenu`, keyboard
  contract and all. Two conveniences beyond what this needed are not
  there: access-key mnemonics (Alt+F for File), and Down preselecting the
  first item of the menu it opens — `PopupMenu`'s selected index is
  private, so Down opens the menu and a second Down steps into it.
- [x] Tags editor (`CollectionService`), in the same Properties panel
  section as the attributes above: existing tags as removable chips, and an
  add-tag field matching `CollectionService:AddTag`'s own semantics (adding
  an already-applied tag is a no-op, not an error; an empty tag is refused,
  since this crate's `\0`-joined wire format cannot tell an empty tag apart
  from none at all). The panel's filter box searches tag names alongside
  the reflected property rows, through the same `properties::matches`.
- [x] Undo/redo (`Ctrl+Z`/`Ctrl+Y`), bit-for-bit reversion verified.
- [x] Save (`Ctrl+S`, writes back in the file's original format, atomic
  write).
- [x] Dockable, rearrangeable panel layout (`shell::layout`, drawn by
  `shell::docks`) with persistence — layout position/size/docking state
  saved across restarts, plus persisted settings (quality, service
  visibility).
- [x] **Drag-to-rearrange docks** — real now, by the tab. A panel's home is
  data (`shell::layout`) rather than the order of three `.child()` calls:
  an edge holds a stack of docks, a dock holds tabs, and dragging a tab
  offers both — land on a strip to join it, land on a dock's half to make
  a new one beside it, at the size it will be. An edge holding nothing
  grows a ghost dock while a drag is in flight, easing open and shut, so
  an edge you emptied can be filled again. Drag a tab past the window and
  it tears out into a window of its own; a dock can be closed from its tab
  and reopened from the ribbon's Home tab or the View menu. The whole
  arrangement persists. The non-drag half is there too and is not
  decoration — "Move to Left/Right/Bottom", "Float" and "Close" on each
  dock's own menu, going through the same one transform, because the
  reference guidance treats drag-only rearrangement as a failure rather
  than a gap. Still open: a torn-out panel cannot be dragged back into the
  main window — GPUI's drag-and-drop is per-window — so it goes back by
  closing its window, which returns it to where it started.
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
    to a `Model` the way a plain click does.
    `Shift`/`Ctrl`/`Cmd`-click adds another top-level object to the
    selection instead of replacing it.
  - **Move** (`2`), **Scale** (`3`), **Rotate** (`4`) — colored axis
    draggers/handles/rings per axis; `Ctrl`/`Cmd`+`L` toggles world/local
    orientation, with an `L` indicator when local is active. While a Move
    or Scale handle is held only that handle is drawn, as in Studio, over
    the guides rather than under them. Move is also
    draggable by the part's own body ("cursor dragging"), which rests the
    part on whatever the cursor passes over — real geometry, not a
    bounding box — falling back to sliding flat across the view only when
    the cursor is over nothing.
  - **Snapping**: move/scale snap in studs, rotate snap in degrees, each
    with its own toolbar increment field and enable/disable checkbox;
    holding `Shift` suspends snapping for as long as it is held, the way
    Studio's draggers read it. A free drag lands the grabbed point on the
    face under the cursor, rounded onto that face's grid from its nearest
    corner and — with Snap to Parts — onto its edges and centre lines,
    squaring the selection onto an angled face (`Alt` keeps its
    orientation); a Move/Scale handle drag soft-snaps to nearby parts'
    faces along its axis. While cursor-dragging a part, `T`/`R` tilt/rotate
    it 90°, eased over Studio's 0.13 seconds.
  - **Dragger guides**, read off Studio's own DraggerFramework: a white
    ruler to the two nearest edges while hovering, a yellow one with minor
    and major ticks while dragging, a yellow line across the face when the
    part lines up with one of its edges or its centre, and, on a Move
    handle, the axis line with a dot wherever the selection's leading face,
    trailing face or pivot would meet a nearby part — which the drag takes
    over the grid step when it is the nearer. The handle trails a line back
    to where the drag began. Each guide and dragger setting is a Viewport
    dock toggle named after the Studio setting it mirrors (Show Hover
    Ruler, Show Target Snap, Show Dragged Point, Show Measurement, Snap to
    Parts, Align Dragged Objects).
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
  - **Placement**: the ribbon's Tools group — Select/Move/Scale/Rotate,
    the local-axis toggle, then the chevron that opens the snap increments
    and the Align popover.
  - **Still open**: the 5th "Transform" toolbar button visible in Studio's
    current toolbar (under "What's planned" → Renderer). A plain click
    landing on a `Model` now draws its aggregate box and gizmos the whole
    thing — see Renderer's own bullet above.

- [x] **Group/ungroup operations** — `Ctrl+G` (or Model ⟩ Group) wraps the
  current selection in one new `Model`, parented where the selection
  itself was; `Ctrl+Shift+G` (or Model ⟩ Ungroup) unwraps a selected
  `Model` back into its own parent and removes it. A selection spanning
  more than one parent refuses Group cleanly rather than picking one
  arbitrarily; ungrouping something that isn't a `Model`, or an empty one,
  is a clean no-op. Each is one undo step regardless of how many instances
  it moves. Out of scope, matching real Studio's own separate Pivot tools
  (see "What's planned" → Renderer): the new `Model` gets no computed
  `PrimaryPart` or pivot, just Roblox's own empty-pivot default.

- [x] **Light guides** — select a `SpotLight`, `PointLight` or
  `SurfaceLight` and the viewport draws how far it reaches, as Studio's
  "Show Light Guides" does: three great circles of `Range` around a point
  light, a spot's cone out to its spherical cap with the axis line running
  past the rim, and a surface light's frustum from its whole face. Drawn
  in the light's own `Color`, following a Range/Angle/Face edit live, and
  only for a selected, enabled light — never for the part it hangs on.
  Toggled from the Viewport dock. A `SurfaceLight` also lights exactly that
  frustum now, measured from the nearest point on its face rather than as
  a cone from the face's centre.

- [x] **Change Class** — from an Explorer row's context menu, the whole
  selection changes class in place (a `Part` into a `WedgePart`, a `Frame`
  into a `TextButton`, a `Script` into a `LocalScript`) as one undo step.
  The instance keeps its referent, so everything that pointed at it — a
  `Weld.Part0`, a `PrimaryPart`, the selection, an open script tab — still
  does, which a plugin that destroys and recreates the instance cannot
  offer. A property the new class has no room for is dropped (the picker
  says which before you pick), one still at the old class's default takes
  the new class's own (a stock `Part` becomes a 2 × 2 × 2 `TrussPart`),
  and tags, attributes and anything the API dump does not describe always
  survive. Related classes and this session's recent picks list first; a
  service keeps its class.

- [x] **Sun tool** (Model page) — places the sun, or from its Moon tile the
  moon, by pointing at the scene instead of typing a time and a latitude:
  drag it across the **Sky**; press a surface and it shines straight onto
  that **Face**; press an object and drag to where its **Shadow** should
  fall; or press a surface and it moves to where its **Glint** reaches the
  camera. Every step writes `TimeOfDay` and `GeographicLatitude` as a
  patch rather than a rebuild, and the whole drag is one undo. Face and
  Glint aim off the surface a part is drawn with — a wedge's slope, a
  ball's curve, a mesh's triangle — not its box. A stretch of sky no
  latitude inside ±90° reaches is held at its rim, and the readout says
  so. Not a Studio built-in: an rbx-native addition.

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
- [x] **Table-stakes script editor conveniences**, per
  `studio/script-editor.md`, none of them depending on `luau-lsp`.
  - **Multi-cursor editing**: Alt-click adds a cursor, Alt+Shift-drag
    (Ctrl+Alt-drag on Linux as well) selects a column block, and
    add-cursor-above/below is Ctrl+Alt+Up/Down on Windows, Shift+Alt on
    Linux and Cmd+Alt on macOS. All of it is GPUI Kit's own editor, which
    was multi-cursor already; nothing had to be retrofitted.
    Ctrl+D (Cmd+D) adds a cursor at the next match of the selection and
    Shift+Alt+L one at every match, Studio's own bindings: exact case,
    whole words when started from a bare cursor, wrapping to the top.
    Adding a selection from outside the widget needed an API upstream
    keeps private, so `gpui-base` is vendored with it (`vendor/README.md`).
  - **Find/Replace** in the current script: Ctrl+F, and Ctrl+H for
    replace, is the editor's own search bar with a match count, case
    toggle and next/previous.
  - **Find All / Replace All** over every open script (Ctrl+Shift+F):
    an overlay listing each match as its line and `Script:line`, jumping
    to one on click or Enter. Replace All goes through each tab's
    editor, so it lands on that tab's own undo stack and reaches
    `Source` through the usual debounced write.
  - **Go to Declaration**: Ctrl-hover underlines a name that resolves,
    and Ctrl-click or the right-click menu's Go to Definition jumps to
    where it was declared. It resolves locals, parameters and loop
    variables by block scope, and falls back to a `function` statement
    whose name ends in the same word, which covers `M.helper()` and
    `self:method()`. It reads the lexer's tokens with no parse tree
    (`script_editor::outline`), and works within one script only.
  - **Script Function Filter** (Alt+F): the same overlay, listing every
    named function in the current script (`function a.b:c`,
    `local function f`, `local f = function`) with its line, filtered as
    you type.
- [x] **A launcher: API key setup wizard, Home, Roblox publishing.** A
  bare `rbxstudio` opens the wizard on first launch (no key stored) and
  Home after that; `rbxstudio <file>` still opens the editor directly.
  - **Setup wizard**: walks the Creator Dashboard's real steps (deep
    link to its API Keys tab, a three-slide walk-through, the settings to
    use, with restricting by experience and IP/expiration recommended),
    then checks the pasted key against `api-keys/v1/introspect` and lists
    every permission it holds or lacks. Three are required —
    `universe-places:write`, `legacy-asset:manage`,
    `user.inventory-item:read` — and 22 are optional, each switching on
    one feature (`rbx_cloud::scopes`; `rbxcloud check` prints the same
    list). The key form cannot be prefilled from a link: the Dashboard
    reads nothing but `?activeTab=`.
  - **Key storage**: the OS credential store (Keychain, Credential
    Manager, Secret Service) through GPUI's credentials API, never a
    plaintext file; an old `api_key` file is moved in and deleted once
    the store reads it back. It is encrypted at rest; a program already
    running as the same user can still ask the store for it, which is
    why the wizard pushes IP restriction and expiry.
  - **Home**: New (Baseplate, Open a file), Recent (last 20, with the
    Roblox place each file came from), My Games (personal and group
    experiences with their icons). Private experiences come from the
    Inventory API's `CREATED_PLACE` listing — Open Cloud has no "list my
    universes" endpoint, and the Dashboard's own one is cookie-only —
    and any single one can be added by its place ID or link. Opening a
    game downloads it into a local copy without overwriting an existing
    one unasked.
  - **Roblox publishing**: the stored key's status and permissions,
    Check again, Replace key, Remove key.
- [x] **Throttled render loop while the window is unfocused.** Losing OS
  focus (`gpui`'s window activation) caps the viewport's render thread —
  not just the UI thread's own poll rate — to a user-chosen preset, 25 or
  30 fps (`pacing::UnfocusedFps`, next to the quality dropdown in the
  Viewport dock); the first focus or input event
  (`pacing::FocusPacing::mark_input`) restores the full display rate
  immediately, ahead of whatever window-activation event may still be in
  flight, so nothing feels sluggish coming back. Persisted the same way
  the quality level and projection mode already are.

- [x] **Colour-coded Explorer folders** — a `Folder` can carry a colour tag,
  edited through a synthetic "Explorer Colour" row the Properties panel
  adds only for a `Folder` (reusing the same `EditKind::Color` widget and
  `Name`-row precedent `properties.rs`/`properties/edit.rs` already had).
  Shown two ways on its Explorer row: the row's own icon is recolored to
  the tag (`class_icons::tint` flattens the already-rasterized bitmap to
  the tag colour, keeping each pixel's alpha, and `explorer::items` caches
  one tinted bitmap per colour per rebuild rather than per instance), and
  its hover/selected background and selection outline use the tag colour
  too, faded, instead of the theme's default blue (`shell/rows.rs` paints
  a tagged row's own chrome, bypassing the vendored `ListItem`'s hardcoded
  hover/selected colours — it has no per-instance override for either).
  Kept entirely out of the saved place file, per this bullet's own
  reasoning: a local, per-place store (`folder_colors.rs`, keyed by the
  place's file path and the folder's Explorer path) rather than an
  invented `Folder` property a real Studio session would trip over.
  `Folder`-only, not "any instance" — the hedge in this bullet's original
  wording, deliberately not chased. **Known limitation**: keyed by
  Explorer path rather than a stable id (a `Ref` regenerates on every
  load), so renaming or moving a tagged folder orphans its tag; a
  load-time prune keeps orphaned entries from accumulating forever, but
  does not carry the tag across the rename (see the `ponytail:` comment in
  `folder_colors.rs`).
- [x] The ribbon's Part insert menu inserts the shape it names. Block,
  Sphere and Cylinder all insert a `Part` and now write its `shape`
  property (`Enum.PartType`) instead of leaving all three to resolve as
  `ShapeKind::Box`; Wedge and Corner Wedge already passed their own
  classes but now get the same size/colour/material defaults as any
  other new part, since `insert_instance`'s defaults gate widened from
  the literal class `"Part"` to any `BasePart` subclass. One test per
  menu item asserts both the inserted instance's class and its resolved
  `ShapeKind`.
- [x] A scriptable command line for `rbxstudio` itself (`cli.rs`):
  `--select <target>[,<target>...]` selects instances once the place is
  open — a target being either an Explorer path anchored at a root
  (`Workspace.Model.Part`) or a bare name, which matches the first
  instance called that anywhere — `--run <script.luau>` runs a file
  against the place exactly as pasting it into the Command Bar would,
  `--verbose` narrates startup on stdout, and `--help` prints the lot.
  Enough to drive a GUI session from a CI job or a wrapper script without
  a human clicking through the Explorer first. The two older
  `RBX_STUDIO_SELECT`/`RBX_STUDIO_RUN` variables still work unchanged —
  they're how the rest of the screenshot aids are spelled — and a flag
  wins over its variable when both are set. Deliberately not matched from
  real Studio's own CLI: `--placeId`/`--universeId`/`--task`, which are
  tied to Roblox's published-place identifiers rather than to a local
  file, per this bullet's own reasoning when it was planned.
- [x] `NumberSequence`/`ColorSequence` — a real curve/gradient editor
  widget (keypoints along a timeline, draggable), not just the read-only
  text these used to fall back to. The row itself draws the sequence — a
  gradient ramp or the curve — and clicking it opens a graph in a second
  window of the editor's own (`crate::sequence_window`): fixed-size,
  floating above the main window, and wearing the editor's own title bar
  (`shell::chrome::panel_topbar`) rather than the platform's.
  Keypoints drag in both axes, a click on empty plot inserts one on the
  curve it split, a `NumberSequence`'s envelope band has its own draggable
  handle, and a `ColorSequence`'s stops are markers under the ramp with the
  panel's existing colour picker behind the swatch. The value axis fits
  itself rather than asking for Studio's "Max Size" number, and dragging a
  keypoint out through the top of the plot is what raises it. Roblox's own
  constructor rules are enforced (2–20 keypoints, non-descending time,
  first at 0 and last at 1), which matters because the renderer's
  `eval_number`/`eval_color` walk the list assuming exactly that.
  **The window keeps no copy of the value**: it rebuilds from the DOM every
  frame and commits through the same textual path a typed row takes, so an
  undo, a Command Bar script or any other write shows up in the graph
  immediately, the viewport repaints on every drag step, and a whole drag
  is still one undo entry.
- [x] `NumberSequence`/`ColorSequence` attributes are creatable. Both are
  in the Attributes section's type picker now, and their values edit
  through the graph above — the same per-type widget an ordinary property
  of that type gets, never a second set of editors. `rbx_dom::attributes`
  already read and wrote both; the gap was entirely on the editor's side.

- [x] **UI Editor — a dedicated UI-editing mode for `StarterGui`.** The
  Style Editor document is the **UI Editor** now, with two sub-tabs on the
  dock-tab pill: the new canvas (the default) and the style-sheet editor
  as it was (**Stylesheet**; View ⟩ Style Editor still lands there). The
  3D viewport never edits a GUI and is off screen while the canvas is up.
  - **The canvas draws one GUI alone** — the `ScreenGui`, `BillboardGui`
    or `SurfaceGui` the selection is in, enabled or not, on a part or not —
    through the viewport's own GUI layout and painter on the same render
    thread, over a flat backdrop with no scene pass. A `ScreenGui` is laid
    out at a simulated resolution: six device presets (desktop, laptop,
    tablet, phone landscape/portrait, small phone), a typed width×height,
    and a portrait ⇄ landscape turn; a `BillboardGui`/`SurfaceGui` at its
    own canvas size, the one its part gives it in the world. Pan with the
    wheel or the middle button, zoom with Ctrl+wheel or the toolbar, fit
    on demand.
  - **The Explorer lists only the UI** while the canvas is up — every
    `ScreenGui`/`BillboardGui`/`SurfaceGui` as a root with its subtree —
    and the Properties, Output and Viewport docks are left out of the
    layout for the room, at render time only: leaving the canvas shows
    exactly what was there. A Figma-style **design panel** stands in for
    Properties: Position (align to the parent, X/Y, an anchor grid that
    keeps the element where it is, rotation and a quarter turn), Layout
    (W/H with an aspect lock, auto layout flow — none, column, row, grid —
    with gap, padding and alignment, clip content), Appearance (show/hide,
    opacity, corner radius, all corners or each one), Fill (swatch, hex,
    opacity, a gradient), Stroke and Constraints (aspect, size and text
    size limits, scale). Every number's label scrubs, each edit is one
    undo step, and a value that lives on a modifier makes the `UICorner`,
    `UIStroke`, `UIPadding` or layout it needs on its first edit. Text,
    image, input, scrolling and the rest keep the Properties panel's own
    rows beneath.
  - **Figma-like editing**, all through the shared selection and the one
    undo history (each gesture one entry, written through the Properties
    panel's own commit): click and marquee select, drag to move, eight
    handles to resize and a knob to rotate — one element or the whole
    selection, carried as one by a frame round it (Shift keeps the aspect,
    and snaps a turn to 15°) — smart alignment guides against siblings, the
    parent and its padded box (Ctrl lets go of them), Alt for the distances
    to what the pointer is over, arrow nudging (Shift for 10 px), Delete,
    align left/centre/right/top/middle/bottom through the Align tool's own
    geometry, distribute evenly, and Ctrl+G into a `Frame` fitted round the
    selection that keeps each value's scale or offset. Every write honours
    the layout the renderer reports rather than a bare parent: the
    parent's `UIPadding`, the element's `UIScale`, an aspect constraint
    (kept through a resize), `SizeConstraint`, and a turned parent.
  - **A floating insert bar** arms a drawing tool (F frame, T text, B
    button, X text box, L image, G image button; V or Escape to put it
    down): drag the element out where it goes, or click for its own size,
    and it lands in the container under the pointer. Its `+` inserts a
    `ScreenGui`, layout or modifier through the Explorer's own insert, and
    an **Offset/Scale switch** picks which half of every `UDim` the canvas
    writes — moves, resizes, nudges, aligns, anchors and new elements alike.
  - **On the canvas, as in Sketch:** a W × H pill under the selection;
    corner radius handles (Alt for one corner); an auto layout's container,
    numbered children and gap and padding bands, each dragged to resize;
    a list or grid child dragged to a new place in it; double-click to
    type into a text element; the Explorer's menu on right-click (Group in
    a Frame, and Ungroup, which lets a `Frame` go where its children
    stand); Ctrl+[ / ] for paint order (Shift for all the way); Ctrl+0 to
    fit and Ctrl+1 to zoom to the selection; Space to pan; Shift to hold a
    move to one axis, draw a square; Alt to resize about the centre.
  - **The 3D view emulates the same screen.** The Viewport dock's Screen
    setting is the canvas's resolution: the scene letterboxes to the
    device's shape and the `ScreenGui` overlay is laid out at its size and
    drawn scaled into the frame, from startup; "Viewport size" turns it
    off.
  - **Make responsive** folds every `Position`/`Size` offset of the
    selection (or the whole screen) into its scale at the current
    resolution — against the parent's padded box, a `Size` along the axes
    its `SizeConstraint` names — and gives each pixel-sized box that no
    aspect constraint already shapes a `UIAspectRatioConstraint` at its
    shape, as one undo step.
- [x] **Align tool**, matching Studio's real Model-tab tool (checked
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
  no new dependency. Shipped: Min/Center/Max, X/Y/Z, World/Local, Selection
  Bounds/Active Object, a selected `Model` moving as one rigid body (the
  docs' "keeping the model intact"), and a compact popover on the
  transform toolbar rather than a full dialog.
- [x] **Align's live preview** — the docs' "dynamically previewing the
  point of alignment before confirming". Opening the popover draws a ghost
  box where each object would land, redrawn as the toggles and the
  selection change and cleared when it closes; one box per top-level
  object, a part keeping its own oriented box and a `Model` taking the box
  around everything beneath it (the same two answers the selection outline
  gives). The overlay itself (`renderer::preview`) takes plain world
  matrices and knows nothing about Align, so the next tool that wants one
  adds no pass. `RBX_STUDIO_ALIGN=...,preview` shows it without a click,
  the way that variable already stands in for the popover's own buttons.
- [x] **DOM editing from the Explorer row** — beyond the old plain
  insert/delete:
  - A `+` on the hovered row (`Ctrl+I` from the keyboard) opens a
    searchable class list that inserts straight under that row, without
    going through a menu. A class the parent cannot take is **greyed
    rather than missing**, so the constraint is visible: the rule is
    exactly the two refusals Roblox's own API dump states — `NotCreatable`
    (`Instance.new` refuses the class outright) and `Service` (a singleton
    the `DataModel` owns), since the dump carries no per-class table of
    legal parents to build anything wider on.
  - Right-click context menu on a row: **Cut**, **Copy**, **Duplicate**,
    **Paste Into**, **Rename**, **Insert Object…** (the same picker the
    `+` opens), **Group as Model**, **Ungroup**, **Delete**. Contextual
    the way creator-docs describes, but *greyed* rather than absent — each
    row is enabled by the same guard its own handler returns early on, so
    a service's menu shows the same shape with most of it unavailable.
  - **Rename** in the row itself, from the menu or `F2`, committing
    through the same `WeakDom::set_name` the Properties panel's `Name`
    field uses. A service is refused, the way it already is for a drag.
  - **Cut** is real (`Ctrl+X`, the Edit menu, the ribbon tile), built out
    of Copy and the removal path Delete already had. Deleting a *service*
    is now refused everywhere, rather than letting one keystroke produce a
    place file with no `Workspace`.
  - **Two insertion preferences** real Studio exposes next to the `+`
    icon's search field (`studio/explorer.md`), behind the same `⋯` and
    persisted: **increment names for new instances** (numbered names for
    same-type inserts/pastes/duplicates) and **expand hierarchy when
    selecting** (whether inserting/pasting/viewport-selecting an instance
    auto-expands the Explorer tree to reveal it, or only highlights the
    top-level parent).
- [x] **Copy/paste/duplicate instances** (`Ctrl+C`/`V`/`D`) — real now,
  from the keyboard, the Edit menu's Copy/Paste/Paste Into/Duplicate items
  and the ribbon's Copy, Paste and Duplicate tiles (Cut, never part of
  this bullet, landed with the Explorer row editing above). Copy is a deep,
  in-process clipboard, not the system one: descendants come along, a
  `Ref`/`Content::Object` property pointing at something copied along with
  it is remapped to point at the copy instead — `Class.Instance:Clone()`'s
  own documented rule — and the copy is independent of the original. Paste
  always lands in `Workspace`, matching creator-docs' `explorer.md`, never
  wherever the selection is; Duplicate lands beside the original in its own
  existing parent instead. A service can't be copied, pasted or duplicated,
  the same refusal Group/Ungroup already enforce. One undo step per
  operation. `Ctrl+Shift+V` is "Paste Into": the clipboard goes into each
  selected instance instead of `Workspace`, one copy per parent, as
  creator-docs describes for pasting "into multiple parents". A
  non-`Archivable` descendant is left out of the copy the way
  `Instance:Clone()` does (the root itself is always copied, and the copy is
  always `Archivable`). Still open: the right-click **Paste Options** ⟩
  **Paste Into At Original Location** the docs mention.
- [x] Drag-and-drop reparenting in the Explorer tree. Dragging a row
  onto another reparents onto it, the way creator-docs describes
  ("simply drag and drop them onto the new parent") — with a ghost under
  the cursor, the hovered row highlighted only while the drop is legal,
  the new parent expanded and revealed afterwards, and one undo step per
  drag. A drop is refused onto the dragged instance itself, into its own
  subtree, onto the parent it already has, and for a service. Escape
  abandons a drag in flight (`App::stop_active_drag`, from the
  window-level key handler), so nothing drops and no undo step is pushed —
  exercised in the running window, not just compiled. There is no drop
  *between* rows, which Studio does not offer either.
- [x] `Rect`, `PhysicalProperties`, `Font` — all three edit now, where all
  three used to be read-only text. `Rect` is four labeled fields; `Font`
  turned out to already be editable before this item was picked up (the
  roadmap text describing it was stale); `PhysicalProperties` was the one
  that needed a shape rather than a field list, being a real enum rather
  than a struct.
  It reuses `EditKind::Optional` — the checkbox-over-an-editor shape
  `OptionalCFrame` already had — because the two read the same way even
  though they mean different things: a `Default` carries no numbers at all
  (the engine derives them from the material), so the five fields appear
  only under a ticked **Custom** box, which is also how Studio presents it.
  That is why the checkbox now carries its own caption instead of the one
  fixed "Has value" wording. Unticking returns the value to `Default`
  rather than zeroing the numbers, and the fields under an unticked box are
  seeded from Roblox's `Plastic` defaults (`0.7 / 0.3 / 0.5 / 1 / 1`) so
  ticking it never commits a row of zeroes.
- [x] Attributes editor (custom `Instance` attributes, distinct from
  built-in properties) — a real, commonly-used modern Studio feature, not
  currently scoped anywhere. The Properties panel now has a dedicated
  Attributes section (below the reflected categories, matching where real
  Studio puts it): attributes are listed, added (name + a type picker),
  renamed, removed, and their values edited through the exact same
  per-type widgets an ordinary property of that type gets — a `Bool`
  becomes the panel's checkbox, a `Vector3` becomes three number fields,
  and so on — never a second set of editors. Name validation follows
  `Instance:SetAttribute`'s documented rules (alphanumeric plus
  `.`/`-`/`/`/`_`, ≤100 characters, no `RBX` prefix). `CFrame` is a
  creatable type now: `rbx_dom::attributes` reads and writes type `0x14` (a
  position and either a one-byte axis-aligned rotation id or nine raw
  floats), checked byte for byte against the two examples in `rojo-rbx/
  rbx-dom`'s attribute format documentation — and, since a blob is packed
  end to end, an instance whose attributes held a `CFrame` no longer loses
  every attribute stored after it.
- [x] **Native Argon integration, not Rojo — live sync.** Argon
  (`argon-rbx/argon`, Apache-2.0, open source) is the preferred target, and
  its live two-way sync protocol — previously undocumented — turned out to
  be readable straight from its own Studio plugin source
  (`argon-rbx/argon-roblox`, also Apache-2.0): HTTP+MsgPack against
  `argon serve`, long-polled. `crate::argon_client` implements it (the
  wire format, the background thread, the `WeakDom` apply/write-back path
  in `shell::argon_sync`), verified end to end against a real `argon serve`
  session. `ExecuteCode` (server-sent Luau) is decoded and always
  discarded — see that module's doc comment. Still open, and genuinely
  separate from the sync protocol: **file-tree ↔ DOM import/export and
  `sourcemap.json` generation** — reading/writing a project's
  `*.project.json`/`default.project.json` tree directly (for `luau-lsp`,
  and for opening an Argon project without a running server) is unrelated
  work against the same file-format Rojo also uses, not yet started.
- [x] **Improve Argon's Diff window to look like GitHub's.** The review
  prompt's Diff window (`shell::argon_diff_window`) now lists the batch
  under Additions / Updates / Removals with an added container's subtree
  beneath it, and shows the selected change on the right: properties
  before and after as chips, a script's `Source` as a unified diff (a
  Myers line diff in `argon_diff_window::diff`, hunks with three lines of
  context, expandable, +/− gutter markers, red/green rows, syntax colours
  from the active theme, cut off at Diff Lines Limit), and what an added
  or removed container holds. Resizable; under 760 px the list becomes a
  picker bar. Split view is not offered.
- [x] **Wally package manager, built in.** Wally (`UpliftGames/wally`,
  MPL-2.0) is the de facto Luau/Roblox package manager. The Wally dock
  (`shell::scripting_tools`, Script Editor tab only) now searches the real
  `api.wally.run` registry as you type, and installing a result resolves
  its *whole* dependency graph — a BFS matching Wally's own resolver shape
  (`crate::wally_client::resolve`), reusing an already-activated version
  when one satisfies a new requirement, erroring cleanly on an
  unsatisfiable one (a real stale dependency, `sleitnick/knit`'s
  `sleitnick/comm@^0.3`, is this codebase's own test fixture for that
  path) — then installs every resolved package into the DOM in the same
  on-disk shape real `wally install` produces: each package's content
  under `Packages/_Index/<scope>_<name>@<version>/<name>`, one alias
  `ModuleScript` per dependency edge beside it, and a top-level
  `Packages/<name>` alias for the package actually picked (`crate::
  shell::wally_sync`). One `WeakDom` insertion path serves both roles Wally
  itself splits: with a connected Argon session it reaches disk for free
  through the write-back sync above; without one, it's native. Two known
  gaps: version discovery is `package-search` filtered client-side, not a
  cloned registry index, so a non-default registry isn't supported; and
  there's no `wally.lock`, so two separate installs can pick different
  compatible versions of a shared dependency.
- [x] **Icon and theme packs — the editor's look stops being hardcoded.**
  The Explorer's class icons are now this project's own icon kit rather than
  Roblox's downloaded sheet (see "What's been implemented" above). Both
  variants, `assets/icons/default/dark` and `.../light`, are now embedded
  at compile time, and an **editor setting for dark/light icons** — a
  "Light Icons" checkbox in the Explorer panel's own overflow menu, next to
  "Show all services" — picks between them at runtime, dark by default,
  without a rebuild, persisted the same way `settings.rs`'s existing
  quality/service-visibility settings are.
  - **Installable icon packs** (`packs.rs`, `class_icons.rs`): a folder of
    SVGs under `icon_packs/<name>/` in the config directory, named by
    `ClassName` (`Part.svg`) or by the kit's tile slug
    (`humanoid-description.svg`, reaching every class that shares the
    tile), layered *over* the built-in kit — a pack of three icons is a
    pack, whatever it leaves out is still the kit's and then Lucide's. The
    Explorer's overflow menu lists what is installed and the choice
    persists in `appearance.json`. A drawing is scaled from its own size,
    not assumed 16x16, and one that will not parse falls through to the kit.
  - **Installable themes**: a toolkit `ThemeSet` JSON file under
    `themes/<name>.json`, named by `appearance.json`'s `theme`; its first
    dark theme (under a name the registry does not already hold — it
    ignores a duplicate) replaces the built-in at startup. Names read from
    `appearance.json` are refused unless they are one plain path segment.
- [x] **"Sober but alive" restyle of the gpui-kit layer (PR #87).** The
  chrome now follows the redesign reference (`gpui-ref/`, kept out of the
  repo) token for token: a three-tone surface ramp (`#0A0A0B` / `#121213` /
  `#191A1C`), three solid text tones, 6%/11% white hairlines, one accent
  (`#6C7FDB`) spent only on active/selected state, a small radius scale
  (3–8px), Manrope/JetBrains Mono, and the reference's 10–13px type sizes.
  Layout, icons, logo and behaviour are the app's own and unchanged, apart
  from the Output dock now spanning the full width under both side docks
  and the 3D view no longer letterboxed to the UI Editor's screen by
  default. Follow-ups still open:
  - [ ] Explorer rows at the reference's 9px chevron slot and 20px indent
    (ours: 12px and 12px). Everything else in the reference's Explorer
    row is in.
  - [ ] Property controls at the reference's full 130px. They sit at 116px
    so every `Workspace` name still reads whole at the default dock
    width; the two go together only once the dock is wider by default.
- [x] **Soften the editor's visual theme — calmer and lower-contrast,
  closer to real Studio but gentler.** Today's panels are high-contrast
  flat blocks: near-pure black/white backgrounds, hard 1px borders, sharp
  rectangular corners, tight padding, saturated colour used everywhere
  rather than reserved for anything in particular. Planned direction,
  taking inspiration (not a straight clone) from a community redesign
  concept's dock/panel layout and surface treatment — credit
  [u/1324764019 on the Roblox DevForum](https://www.roblox.com/users/1324764019/profile),
  [reference screenshot](https://devforum-uploads.s3.dualstack.us-east-2.amazonaws.com/uploads/original/4X/f/0/8/f08fc6d47d3aef25edecb00dd708f13e4f0553c1.png):
  - **Palette**: a muted 6-8 step neutral grey ramp replacing today's
    near-black/near-white panel backgrounds — nothing darker than
    roughly `#1a1a1a`, nothing lighter than roughly `#f0f0f0`.
  - **Panel separation**: hard 1px borders replaced by either a small
    background-luminance step between adjacent panels (2-4%) or a soft,
    low-opacity shadow (<15%) with no visible stroke.
  - **Corner radius**: a consistent small radius (4-6px) on buttons, icon
    containers and panel corners — no sharp rectangles.
  - **Padding**: toolbar buttons and list/tree rows (Explorer, Properties)
    grow roughly 30-50% over today's values to read as less dense.
  - **Accent colour**: saturated colour reserved for the selection
    highlight, the active tab indicator and functional icons; everything
    else desaturated.
  - **Typography**: regular/medium weight by default; bold reserved for
    section headers only.
  - **Dock/panel structure**: review the reference concept's
    floating/grouped-tab dock style and propose which pieces (tab
    grouping, panel grouping, drag handles) fit this project's existing
    dock layout — presented as options to choose from, not a mandated
    rebuild.
  - **Deliverable**: a theme/token file (colours, radii, spacing, font
    weights) the rest of the UI reads from — a concrete first instance of
    the theme format the item above calls for — plus a before/after
    screenshot of one representative panel (the Explorer, or a popup like
    Store/Upgrades) for review before it rolls out app-wide.

  Shipped: a real design-token module
  (`crates/rbx_studio/src/tokens.rs`) every piece of chrome reads from —
  surfaces, state washes, borders, a seven-step text ramp, one radius, the
  frame's measured dimensions and its type scale — with the palette also
  expressed as a `ThemeSet`/`ThemeConfig` JSON
  (`assets/themes/dark-soft.json`) so the toolkit's own widgets follow it
  without a rebuild, and a test that fails the moment the two disagree.

  The palette is not this bullet's original `#1a1a1a`-to-`#f0f0f0` ramp and
  the layout is not the DevForum concept's: partway through, the project's
  own Figma design landed (`RBX-NATIVE`, frame `RbxNative - Studio App`)
  and the editor was rebuilt against **that** instead, measured rather than
  interpreted. It is darker than this bullet asked for and answers the same
  complaints: soft state washes instead of hard borders, a single 3px
  radius, and exactly one saturated colour in the whole UI (the checkbox
  blue, which focus and selection borrow and nothing else may).

  The shell is now: a **title bar this editor draws itself** (client-side
  window decorations — logo, centred title, minimize/maximize/close,
  drag-to-move), the menu strip, **Row A** document tabs, **Row B** the
  ribbon's seven category tabs, **Row C** the ribbon, **Row D** a
  three-column workspace — Properties left, the open document over Output
  in the middle, Explorer right — each dock a tab strip over an inset body.
  Ribbon commands are 42px tiles and 78px stacks; the snap increments are a
  live readout that opens its own editor. Chrome icons are Lucide, the set
  the design is drawn with; the multi-colour `class_icons` kit stays where
  identity matters, in the Explorer. Contrast is asserted rather than
  eyeballed — every meaningful text token clears WCAG AA on every surface
  it can land on, and the disabled step is asserted from both sides.

  A second pass then grounded the whole thing in WCAG 2.1/2.2 and the
  WAI-ARIA APG rather than in taste. The editor is keyboard-operable end to
  end — Tab between regions, arrows within one, the full APG Tree View
  contract in the Explorer, Escape out of any menu — and three genuine
  keyboard traps were found and fixed by driving the window. Focus rings
  appear for keyboard focus only and clear 3:1 on every surface; selection
  and focus are no longer drawn the same way. There is a persisted UI scale
  (Ctrl+= / Ctrl+− / Ctrl+0, 0.5x-2.0x) over every font *and* every box, so
  text reaches 200% without losing layout. Controls are sized to the
  `InputsStyle` frame and clear WCAG's 24x24 target floor — the checkbox was
  10px. Each transform tool has its own pastel plus a border, so its state
  never depends on colour alone. Contrast, target sizes and the toolkit
  theme mirror are all asserted in tests.

  Dock sizes and the Output dock's collapsed state persist, with a Reset
  Layout command beside them, and the View menu carries Reduce Motion (which
  overrides the desktop preference read at startup) and Large Click Targets
  (WCAG 2.5.5's 44px floor in place of 2.5.8's 24px).

  `UX_GUIDELINES.md` §11 lists every deviation from the frame with its
  reason, and §1 states where the editor stands against the reference
  guidance's Stage 1/2/3 — failures included.
- [x] **Output window: the half of real Studio's filter/display feature
  set that does not need the sandbox**, checked against `studio/output.md`
  rather than assumed and built against what the Command Bar and app
  warnings already put in the dock today:
  A **Show Timestamp** toggle, in the Output panel's own overflow menu
  next to Explorer's and Viewport's toggles (`Shell::output_show_timestamps`,
  `shell/dock.rs`), prints a per-row timestamp in `HH:MM:SS.SSS`; rows now
  carry a per-kind color and icon in place of the old plain `✕`/`✓`
  marker — `print`/a successful run in the default text color with a
  check icon, `warn` in orange with an alert icon, `error` in red with an
  X icon (`OutputEntry::kind`/`RowKind`, `shell/output.rs`). **Free-text
  search over the log** is shipped too — a box in the Output tab's own
  title bar, beside the level filter, matching case-insensitively against
  both halves of what a row shows (the command and the result) and
  narrowing *within* the level filter rather than replacing it
  (`OutputLog::filtered`). The duplicate-display gap is closed: a Command
  Bar run's outcome used to show twice, once in `command_bar::Feedback`'s
  label and again as the Output dock's permanent row; the label now
  appears only while the dock is collapsed
  (`Feedback::shown_inline`), when it is the one place the result would
  otherwise be lost — which is why a `Ctrl+S` save, which used to report
  through the label alone, now logs a `Save` row too.
- [x] **The viewport no longer goes black, with the render thread's stats
  frozen, after a scripted full reload**
  (`RBX_STUDIO_RUN`/`RBX_STUDIO_EDIT`). Fixed by `ab24e6e`, five hours after
  the bug was filed here (`20cfbe6`). The two were never linked, so a later
  pass found it "not reproducible" with nothing to explain it. It was a
  latch, not a stall. The viewport infers that it is on screen from GPUI
  having repainted it, and a landed frame is ordinarily the only thing that
  repaints it. A rebuild stalls the frames for seconds, so a mounted panel
  looked like a tab switched away. It was declared hidden, the render thread
  stopped, and nothing ever repainted it again. `ab24e6e` measured this: 0
  frames against 75/s, and one "declared hidden" with no "visible again".
  Now the tick that would give up requests one repaint first
  (`workspace_view::presence`, with the stalled-reload case among its
  tests). That repaint still lands if the window is hidden or covered: GPUI
  leaves the window dirty and draws it once it is mapped again. Re-checked
  on 2026-09-26 on `marked.rbxl` with a scripted `Sky` insert, which drew on
  both the current build and one with the probe disabled. The editor now
  repaints for other reasons during a reload, such as Output entries, so the
  latch no longer shows on its own. The probe is what keeps it closed when
  nothing else repaints. `patch_parity`'s
  `a_rebuild_leaves_the_renderer_drawing` separately guards the renderer's
  side: a refused patch still draws what a reload draws.

### Platform
- [x] Linux (X11) — the daily-driven target.
- [x] Windows — asset cache and settings now fall back to
  `%LOCALAPPDATA%`/`%APPDATA%` when no `XDG_*`/`HOME` is set; the
  rendering/editor stack (`wgpu`, GPUI Kit) is cross-platform by
  construction. Never built on a real Windows machine yet — see
  [Platform: Windows](#platform-windows) below.
- [x] Windows-native texture fallback — when `setup.rbxcdn.com` cannot
  serve an `rbxasset://` file (offline, or absent from its packages),
  `rbx_assets` reads it off a Roblox/Studio install on the same machine:
  every `%LOCALAPPDATA%\Roblox\Versions\version-*` folder, newest first,
  looking in `content\`, then `PlatformContent\pc\`, then
  `PlatformContent\pc\textures\`. The last two are not optional: a real
  install keeps the default skybox panels (`sky\sky512_*.tex`) only under
  `PlatformContent\pc\textures\sky\` (checked against a real Studio
  install), so `content\textures\` alone would still have left a place with
  no `Sky` black when offline. The CDN stays the primary source; this is
  tried after it and the Sober fallback have both failed, and never writes
  to or installs anything. What it reads is deliberately not kept in the
  asset cache either: that cache is keyed by path alone, so a copy taken
  from an old version folder would otherwise become the permanent answer
  for that path once the CDN is back. The path comes from a place file, so
  unlike the zip-backed fallbacks it is refused outright when it contains
  `..`, `\`, `:`, an empty or `.` segment, a root or drive prefix, or a
  Windows device name (`NUL`, `CON`, `COM1`…), rather than handed to the
  filesystem.
- [x] A first real build on Windows, and CI coverage for it — every
  change now runs `cargo clippy -D warnings`, `cargo build` and
  `cargo test --workspace` on `windows-latest`
  (`.github/workflows/ci.yml`). The workspace compiles and its tests pass
  there. What that job cannot answer is anything about the editor
  *running*: it is headless, so no window, GPU surface or input path has
  been exercised on Windows. Launching the editor there is still open.
- [x] Daily API-Dump sync (`.github/workflows/sync-api-dump.yml`).
- [x] The viewer in a browser — `rbx_viewer` builds for
  `wasm32-unknown-unknown` (`scripts/build-web.sh`) and runs on WebGPU
  with the same renderer, streaming loader, legacy-union booleans,
  fallbacks, quality levels (`Automatic` included) and camera controller
  as `rbxview`, plus a read-only Explorer (Studio's service order and
  default filter, shared through `rbx_viewer::services`) and Studio's
  click selection (`pick::from_click`, now shared with `rbxstudio`).
  Switches for GUIs, local lights, shadows, bloom, colour correction,
  decals, materials, particles, beams, trails and the development GUI.
  Roblox's CDNs send no CORS headers, so a page cannot fetch assets
  itself: `rbxview --serve` hosts the page and resolves assets for it
  through the same cache, anonymous delivery, Open Cloud key and
  `rbxasset://` content as the desktop tools, on loopback only. What it
  shares with the desktop build is verified against it: a close-up of a
  union-heavy place differs from `rbxview --screenshot` by 0.24/255 on
  average, the overlay chips aside. Two things it needed that the native
  build had silently: WGSL's derivative-uniformity check, which a
  browser's compiler enforces and naga never did (turned off per module in
  `gpu::shader`, which is what native already got), and a fallback when a
  GUI shapes text before any font face exists (no system fonts in a
  browser). Rebuilding the material arrays after a pack streams in now
  copies every layer the old arrays already hold on the GPU instead of
  mip-mapping it again on the CPU — about a second per landing in the
  browser, and a real cut to every native swap-in too.
  Not there yet: WebGL2 (the renderer needs storage buffers and compute),
  threads (union booleans and decodes run on the page's one thread), and a
  size diet (the wasm is ~10 MB, mostly the embedded API dump).

## What's planned

### Script authoring — the biggest real gap
- [ ] 📋 **Script Analysis** (real Studio's static-analysis pass,
  in-editor squiggles plus a details window) is closer to genuinely
  duplicate work with the `luau-lsp` diagnostics below and probably
  shouldn't be built twice.
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
- [ ] 📋 **Managing script templates from inside the editor.** Authoring
  one today means a file manager and a text editor; there is no UI for
  adding, renaming or deleting a template. The user's extras also appear
  in the ribbon's Script menu but not the menu bar's Model menu, whose
  items are fixed actions rather than a list built at runtime.
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
- [ ] 📋 **An animated `ForceField` shimmer**, and the modern material's
  own `MeshPart.TextureID` source. The shell itself is drawn now (see
  "What's been implemented" → Renderer), but it does not move: a pattern
  that animates needs a clock in the material pass, and a `--screenshot`
  that differed from run to run would be worse than a still one — the same
  reason a `StyleRule` transition never applies here. Roblox's current
  `ForceField` material is documented as displaying the dark-to-light range
  of the `Class.MeshPart.TextureID` of the mesh it is applied to, which
  this renderer does not feed into the material pass at all; that is the
  other half, and it needs the mesh's own texture on the material path
  rather than a second guess at the texture-less look.
- [ ] 📋 **A 5th "Transform" toolbar button** appears in Studio's current
  toolbar (see the owner-provided screenshot) alongside the now-implemented
  Select/Move/Scale/Rotate (see "What's been implemented" → Editor), but
  creator-docs' `parts/index.md` only documents "Transform parts" as the
  *umbrella name* for Move+Scale+Rotate together, not a distinct 5th
  interactive tool — could not confirm what it does specifically from the
  docs alone. Needs confirming against a real Studio instance (the "Studio
  fallback" Vinegar/Wine workaround elsewhere in this roadmap is one way to
  do that) before implementing it, rather than guessing.
- [ ] 📋 **Gizmo papercuts and third-party-tool parity requests**, from
  real use of the Move/Scale/Rotate gizmos above. Checked against
  `Roblox/creator-docs` (`parts.md`'s Transform Parts section) and, where
  noted, **Building Tools by F3X** — a widely-used third-party Studio
  plugin, not native Studio — since some of what was asked for turns out
  to be F3X's own convention rather than something Studio itself does:
  - [x] **Scale handles that lock a Ball/Cylinder to a round
    cross-section**: dragging any Scale handle on a Ball or Cylinder used to
    grow only the one axis grabbed, same as any other part, which turned a
    sphere oval or a cylinder's round end into an ellipse. Native Studio's
    own docs give Scale no shape-specific behavior at all — `BasePart.Size`
    is three independent numbers regardless of `Shape` — so this isn't a
    Studio-parity gap so much as a genuinely useful addition modeled on
    F3X's real, open-source `Resize.lua`.
    **Ball and Cylinder are both done**: holding `Alt` while dragging a
    Ball's Scale handle now grows all three axes together (`Size +
    (d,d,d)`, keeping it a sphere); on a Cylinder, `Alt` locks whichever two
    axes form its round end together when either is the one grabbed —
    grabbing the length axis instead is unaffected either way, since
    nothing else is meant to grow alongside a cylinder's length. Which axis
    *is* the length wasn't guessed at or taken from F3X's own source: this
    project's own shape-resolution code (`rbx_viewer::scene::shape::
    part_type`) already fixes `Enum.PartType.Cylinder` to draw with its
    length along the part's local X and round in Y/Z, matching Roblox's
    real engine geometry, so the lock reuses that existing, already-tested
    fact rather than a second, independent determination of it.
    `Shift` was the modifier F3X itself uses and the one this bullet
    originally asked for, but it already means "invert the current snap
    state" on every other tool in this editor — `Alt` was picked instead
    because it is provably inert at the exact moment a Scale handle is
    grabbed (it only means "cycle selection" on a click that falls through
    to a *pick*, a branch a handle grab never reaches), not because it
    matches F3X. **Wedge/CornerWedge need nothing**: `Resize.lua` gates its
    shape-specific branch on `Part:IsA 'Part'` and sends every other class —
    `WedgePart`, `CornerWedgePart`, `MeshPart` — through the same plain
    per-axis resize, so there is no case to model, and native Studio's docs
    give Scale no shape-specific behavior either. Plain `Part`s and
    `MeshPart`s keep today's per-axis behavior regardless. — see
    `F3XTeam/RBX-Building-Tools`'s `Tools/Resize.lua`.
  - [x] **A live stud-count readout while a Move/Scale drag is in progress**
    (e.g. a floating "12" near the handle showing studs moved/grown so
    far) — genuinely useful, not documented as a specific Studio feature
    either way. A small label follows the cursor while a drag is held,
    reading the straight-line distance moved so far for a Move, or the
    dragged axis's growth (or shrink) in studs for a Scale — both to two
    decimal places, matching Studio's own numeric-field precision. Reads
    the same delta `gizmo.rs`'s own drag math already computes for the
    part itself (see `workspace_view::readout`), so there is nothing new
    to keep in sync. After a handle drag the label turns into Studio's
    measurement box: type a length and the selection moves by exactly
    that, as one undo step. A Rotate-angle readout was a natural follow-on
    but is out of this bullet's own scope and hasn't been added.
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
  - [x] **Selection outline thickness**: the outline (see `rbx_viewer::renderer::selection` and
    `renderer::outline`) is no longer a one-pixel `LineList` but a
    screen-space quad per edge, expanded in the vertex shader to a constant
    on-screen width (~3px selection blue, matching Studio's light-blue
    selection box; the hover cue rides the same path in amber).
  - [x] **Selection outline shape conformance**: a `Ball`, `Cylinder`,
    wedge or mesh outlined as its oriented bounding box rather than as its
    own silhouette. A part now draws through the very mask-and-composite
    pass `Highlight` does (`renderer::cue`), so it outlines as its own
    shape — a ball as a circle, a `MeshPart` as its own polygon — while a
    container, which has no shape of its own to trace, keeps the box
    around everything beneath it. The hover cue rides the same path in its
    own amber. Real Studio's own highlight is documented
    (`parts/models.md`) only as a light-blue outline with no
    shape-conformance spec published: this is not a claim of parity with
    it, and the code says so — it is the same answer this renderer already
    gives for the one outline effect that *is* specified as a silhouette.
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
  interactive tool, since real Studio exposes both. Landed so far: the
  Properties panel's `Origin` row reads a part's or model's pivot the way
  `GetPivot` does and moves the instance the way `PivotTo` does (see
  "What's been implemented" → Editor).
- [ ] 📋 **Explorer export and searchable-tree browsing** — the two halves
  of Explorer DOM editing that did not land with the row affordances
  above:
  - Export from the row's menu: services and the whole place to Roblox
    (Save/Publish, see below), to a local file; individual instances to
    `.obj` and `.gltf` — genuinely useful native additions since Studio
    itself has no built-in mesh export today. Roblox's own roadmap does
    list glTF export, pushed from Late 2025 to Late 2026 in its
    [fall 2026 update](https://devforum.roblox.com/t/creator-roadmap-2026-fall-update/4880208),
    so `.gltf` may become Studio parity rather than an addition. Check what
    it actually exports once it ships.
  - **Keep search results browsable without clearing the search field** —
    a real devforum request
    ([`view-descendants-of-matching-instances-in-explorer-search`](https://devforum.roblox.com/t/view-descendants-of-matching-instances-in-explorer-search/4862003),
    read in full, not just the title: today's real Studio forces you to
    clear the Explorer's search box before you can expand a matched
    result's children, so inspecting several matches in a row means
    search → select → clear → expand, repeated per result). Worth getting
    right from the start here rather than reproducing that friction: a
    filtered Explorer tree should stay expandable in place. The Explorer's
    search box is still inert today, so this means building the filter as
    well as keeping it browsable.
- [ ] 📋 **Multi-instance drag from a row outside the selection.** Pressing
  such a row collapses the selection to it before the drag starts, so a
  multi-instance drag only carries the whole selection when grabbed by its
  anchor row. The Explorer tree tracks one selected row and already
  behaves this way for a plain click, so this belongs with the fuller
  Explorer editing item above rather than being patched at the drag.
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
#### Properties panel — remaining type editors
- [ ] 📋 **Category order in the panel.** Categories are sorted
  alphabetically (`properties::group_by_category`), which doesn't read as
  sensibly grouped as real Studio's own panel does. Reported from real use
  and not yet checked against creator-docs or a real Studio instance
  (worth doing before assuming what "logical" ordering actually means
  there — the "Studio fallback" Vinegar/Wine workaround elsewhere in this
  document is one way to check). The two papercuts first filed with it —
  tight rows, and a numeric value clipped by a narrow field — went with
  the panel's rework: hairline seams between rows, and a numeric value
  shown whole on its own row with its components behind an expander.
- [ ] 📋 **A general pass on how Studio renders each type**, rather than
  a generic fallback: go through the API dump's actual type/category
  coverage (`assets/API-Dump.json`, kept current by the daily sync) rather
  than relying on memory for how each Roblox type is conventionally shown,
  the same discipline `AGENTS.md` asks for lighting/material claims. The
  two types this item first named are done — every `CFrame` row expands
  into `Position` and `Orientation`, and `BrickColor` has Studio's palette
  picker (see "What's been implemented" → Editor).
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

- [ ] 📋 **`AnimationConstraint` rigs, the Avatar Joint Upgrade.** Roblox
  no longer builds its R15 player characters from `Motor6D`s: with
  `StarterPlayer.AvatarJointUpgrade` on (the default for new experiences)
  a character spawns with `AnimationConstraint`s instead, which animate
  kinematically *and* take part in physical simulation (ragdolls, limb
  strength). `Motor6D` stays in the engine, but is frozen: new joint
  features land on `AnimationConstraint` only. Both classes are already in
  the embedded dump (`AnimationConstraint.Transform` included), so this is
  reachable data, and it changes the animation items above. Playback and
  the Animation Editor must drive `AnimationConstraint.Transform` as well
  as `Motor6D.Transform`, and find a rig's joints by either class (an
  upgraded rig has no `Motor6D` to find). Rig insertion should emit
  `AnimationConstraint` joints for R15 when `AvatarJointUpgrade` is on,
  and `Motor6D` when it is off. The Explorer, the joint gizmos and the
  "joint" icon treatment should treat the two as the same kind of thing.
  The simulation half (force-based limbs, ragdolls) is engine physics and
  stays out of reach, the same as any other physics (see
  [Explicitly impossible](#explicitly-impossible-without-robloxs-engine)).

#### CSG
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
- [x] **Docks tab into each other again.** A drop on a dock's tab strip
  joins it as a tab. The two split halves had been laid over the whole
  dock, strip included. Because they were painted later, they took the
  drop first, so every drop meant for the strip split the dock instead.
  They now cover the dock's content only. `Layout::apply` also misplaced
  two own-dock drops, which are now fixed: splitting a tab off below its
  own dock landed it above, and a lone tab dropped on its own strip
  joined the next dock.
- [ ] 📋 **The accessibility work the reference guidance calls Stage 2 and
  Stage 3, minus what already shipped.** Stage 1 is met and asserted in
  tests; these are the rest, each small enough to ride along with other
  work rather than needing its own PR:
  - **A separate editor/viewport font size**, independent of the UI scale —
    VS Code's split between `window.zoomLevel` and `editor.fontSize`.
    Nothing needs it yet; the moment the script editor grows, it will.
  - **Named dock layouts.** Sizes persist and Reset Layout exists; saving
    several under names (Blender's "workspaces") is the piece that does not.
  - **A high-contrast theme** targeting 7:1 body / 4.5:1 large text
    (WCAG 1.4.6). The palette is already a token module and the toolkit
    theme already mirrors it, so this is a second `ThemeSet` rather than a
    rework.
  - **44×44 targets on primary and destructive controls by default** (2.5.5),
    rather than only when Large Click Targets is on — Save, Delete, and
    Play/Stop once they exist.
  - **A command palette**, which the same guidance files under "recognition
    rather than recall" alongside keyboard-driven panel management.
  - **Keyboard focus shown separately from selection in the Explorer.** The
    toolkit's `TreeState` tracks a single `selected_ix` and nothing else, so
    the focused row and the selected rows cannot differ — which matters
    because the Explorer multi-selects. Needs the toolkit's tree replaced or
    extended.
- [ ] 📋 **Property editors for the two `Variant` types that still have
  none.** The Properties panel renders a value for every type the DOM can
  hold, but two of them are still read-only. Inventory, rationale and
  rough sizing live in
  [`agents/property-editors.md`](agents/property-editors.md); the bullets
  below are what is left after the `CFrame`/`Ray`/`Vector3int16`/`Faces`/
  `Axes`/`NumberRange`/`UDim` pass, the `OptionalCFrame` one, and
  `PhysicalProperties`, `Font` and `BrickColor` (see "What's been
  implemented" → Editor).

  Each is its own piece of work, so each gets its own PR:
  - **`Ref`** (`ObjectValue.Value`, `Weld.Part0`) shows the target's name
    and cannot be changed. Needs an instance picker — an Explorer target,
    or a pick-in-viewport mode.
  - **`Content`** (`Decal.Texture`, `MeshPart.MeshId`) needs an asset URI
    field, and its `Content::Object` case is a `Ref` picker again.

  Smaller, and not a missing editor: `Font` edits as three typed fields
  (family, weight, style) by choice — a weight's nine names are quicker
  typed than picked — so a family list is what would make it a picker,
  and that lives in `rbx_viewer`'s font package rather than the editor.

  **Deliberately excluded**, so nobody "fixes" them: `SharedString`,
  `UniqueId`, `SecurityCapabilities` and `Unknown` stay read-only. They are
  identities and opaque payloads — editing them by hand corrupts a file
  rather than editing it.
- [ ] 📋 **Effects (drop shadows) in the UI Editor's design panel.** Figma's
  Effects section, and Sketch's, is a drop shadow per element, which Roblox
  now does with `UIShadow`. The embedded API dump predates the class and the
  GUI renderer draws none, so both come first; the panel's `+` then makes one
  the way Stroke makes a `UIStroke`.
- [ ] 📋 **`ViewportFrame` authoring in the UI Editor.** The rest of the
  dedicated UI-editing mode for `StarterGui` has shipped (see "What's been
  implemented" → Editor → UI Editor); this is what is left of it.
  Setting one up in real Studio is notoriously painful: the `Camera` has to
  be created and parented by hand, its `CFrame` typed in or scripted, the
  model cloned under the frame, and every adjustment means re-running that
  dance with no live preview. Here a `ViewportFrame` selected on the canvas
  gets its own editing surface — its own window, or a large pop-out from the
  UI Editor, since it needs room a dock does not have — that renders the
  frame's contents exactly as the viewer's `ViewportFrame` support draws
  them, with a free-flight camera whose pose is written straight to the
  frame's `CurrentCamera` (created on the spot if the frame has none), an
  "insert from Workspace" action that clones a selected `Model`/`BasePart`
  under the frame, framing ("fit the model") buttons, and the frame's
  `Ambient`/`LightColor`/`LightDirection`/`ImageColor3`/`ImageTransparency`
  beside it with the result updating live. Every write goes through the undo
  history like the rest of the tab.
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
    (matching Studio's own output: correct joints — `AnimationConstraint`s or `Motor6D`s, see the Avatar Joint Upgrade item above — and
    `Class.Humanoid` so the rig is immediately animatable — ties directly
    into the Animation work above) — either built locally from known
    proportions/meshes, or, if a specific official asset id is the more
    faithful source for a given rig, imported directly as a real `.rbxm`
    the same way any other asset import works.
- [ ] 📋 **Wally "Recently published" list.** The Wally dock's Discover
  page shows the registry's featured packages (the list wally.run's own
  home page uses); a recently-published list would need a route the
  registry backend doesn't have (`UpliftGames/wally@f578078:
  wally-registry-backend/src/main.rs` exposes package-contents,
  package-metadata, package-search and publish only), so it waits on
  upstream.
- [ ] 📋 **Native Git integration** — a real panel in `rbxstudio` (diff view,
  stage/commit, branch switch), not relying on the user's own external git
  client. Not scoped in any detail yet.
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
- [ ] 📋 **Discord Rich Presence, switched on from Studio Settings** —
  an rbx-native addition, not Studio parity: real Studio has no built-in
  Discord presence, only third-party plugins and companion apps. A
  checkbox in the Studio Settings screen above (off by default) that
  shows the open place, what's being edited (a script's name, or the
  viewport) and elapsed time on the user's Discord profile, with a
  second toggle to hide place and script names for anyone working on
  something unannounced. Discord's local IPC is all it needs — the
  `discord-ipc-0` socket under `$XDG_RUNTIME_DIR` on Linux (Flatpak and
  Snap builds of Discord put it somewhere else, worth probing rather
  than assuming one path) and the `\\.\pipe\discord-ipc-0` named pipe on
  Windows — plus this project's own Discord application id. No Discord
  running should mean silently nothing, never an error.
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
- [ ] 📋 **More New templates on Home.** Home's New section has Baseplate
  only. Flat Terrain needs a `Terrain.SmoothGrid` writer, which nothing
  in the tree has yet; a terrain template without its voxels would be a
  Baseplate by another name. Real Studio's own list keeps growing, so
  treat it as a starting point. An account switcher (one key per Roblox
  account) would be an rbx-native addition, not Studio parity — real
  Studio has none.
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
- [ ] 📋 **A theme that reaches the whole editor, and a way to install
  one.** A theme file replaces the toolkit widgets' colours only: the
  chrome this editor draws itself (`tokens.rs`, hundreds of call sites)
  still reads compiled-in values, so palette, spacing and type scale are
  not data yet. There is also no in-editor theme chooser — `appearance.json`
  is edited by hand — no pack browser or installer, so a pack is copied in
  by hand, and while an icon pack is chosen at runtime a theme takes
  effect only on the next launch.
- [ ] 📋 **What the visual pass left behind**, beyond the items that
  already have their own bullets under "What's planned" → Editor (the
  remaining Stage 2/Stage 3 accessibility items and the property types
  still without an editor): hover feedback is instant — `gpui` has no
  CSS-style property transitions and cannot transform a `Div`;
  `gpui_base::transition` animates one value explicitly (the ghost dock's
  ease uses it), but putting it behind every hover state is a larger job —
  and keyboard arrow-navigation inside the hand-built menus
  (`shell::menu`) isn't wired.

### Play / Test workflow
- [ ] 📋 The sandbox-place design (private per-developer place, injected
  probe scripts, a tunnel back to the editor) is fully designed — see
  [Possible via a workaround](#play--test-a-private-sandbox--probe) — but
  not implemented in code yet.
- [ ] 📋 Wiring the Output dock to real script `print`/`warn`/`error` and
  session events (join/leave messages and the like) once a sandbox session
  is running — depends on the sandbox above existing first.
- [ ] 📋 **Output window: the sandbox-dependent half.** Filtering by
  **context** (`Client`/`Server`/`User Plugin`) only means something once
  the sandbox's client/server split exists to produce it, and the **Show
  Context** and **Show Source** (script name + line number) toggles are
  the same story — neither a Command Bar run nor an app warning carries a
  script/line origin today. `TestService.Message`'s blue/info kind needs
  the sandbox before anything can produce it. Whether logged tables show
  expanded by default is the one piece here that does not need the sandbox
  first.
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
  The current Studio UI is a **Network** button in the playtest toolbar
  (a Wi-Fi icon and a dropdown) opening a **Network simulation** panel:
  a **Preset** dropdown (or Custom), then **Inbound** and **Outbound**
  groups that each carry **Latency** (ms), **Packet loss** (a percent
  slider) and **Jitter** (ms), and **Apply** / **Save** / **Reset**
  buttons. That is the shape to copy. One honest limit, which the paragraph
  above glosses over: the relay only carries the probe's own traffic, not
  the replication between the real Roblox client and server that Studio's
  simulator throttles. So this would delay this editor's view of the
  session, and would not make the *game* feel lag. It cannot test a
  game's lag handling, and it matters more now that Server Authority
  (below) is built around exactly that.
- [ ] ⚠️ **Server Authority (Studio Beta, announced 2026)**, checked
  against the Creator Hub's `projects/server-authority` page. It is a
  workspace mode, not an editor feature: `Workspace.AuthorityMode = Server`
  turns on client prediction, misprediction detection, and rollback with
  resimulation, and sets five companion `Workspace` properties in one go
  (`NextGenerationReplication`, `PlayerScriptsUseInputActionSystem`,
  `SignalBehavior = Deferred`, `UseFixedSimulation`, `StreamingEnabled`).
  It adds `RunService:BindToSimulation()` and `SetPredictionMode()`, makes
  the Input Action System the way clients affect state, and syncs custom
  data on predicted instances through Attributes (64 per instance, names
  and string values of 50 characters at most). Effect on this project, in
  three parts:
  - **Editing is unaffected.** No place format or DOM change. The reachable
    part is small: once the daily API-dump sync brings the new `Workspace`
    properties in (only `SignalBehavior` and `StreamingEnabled` are in the
    embedded dump today), the Properties panel shows them, and setting
    `AuthorityMode` could flip the five companions together, the way
    Studio does. Worth a line in the Attributes editor's limits too.
  - **Running it is impossible.** Prediction, rollback and resimulation
    are engine behaviour. This is the same wall as physics, listed under
    "Explicitly impossible".
  - **The sandbox is affected.** A place using it needs its probe and
    mirror scripts to obey the same rules (shared `ModuleScript`s
    initialised from `ReplicatedStorage`, no reliance on `Heartbeat`
    ordering), and playtest results for such a place are only as good as
    the real client's. The Network Simulator item above is the one that
    would matter most for these places, and is the one the relay cannot
    fully serve.
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
- [ ] 📋 **Making those handles interactive.** The docs are explicit that
  a `Handles`/`ArcHandles`/`*HandleAdornment` listens for input only under
  a player's `PlayerGui` or the `CoreGui`, and firing `MouseButton1Down`/
  `MouseDrag`/`MouseButton1Up` at a script is the plugin API's business
  (the item above this one), not the renderer's — so what a place file
  holds is drawn, and nothing is draggable yet.
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
- [x] Mouse capture in the free-flight camera — a Win32 backend beside the
  X11 one in `pointer_lock::server`: `ShowCursor` hides, `SetCursorPos`
  warps back to the viewport centre after every move. No `ClipCursor`:
  GPUI already `SetCapture`s on button press, so moves keep arriving
  between warps. Type-checked for `x86_64-pc-windows-msvc` and built by
  the Windows CI job, but not yet driven on a real Windows desktop.

### Tooling / CI
- [x] A job that rebuilds against the current Studio version and
  compares parsed output to reference dumps, to catch a format drift
  before a user does — the nightly API-dump sync now runs
  `rbx_parser_cli`/`rbx_reflection`/`rbx_lua`'s tests with Studio's newest
  dump embedded before committing it, including a golden comparison of
  every fixture's `rbxdump` output against `assets/tests/dumps/`. A
  failure blocks the commit. Ceiling: the fixtures are old saves, so this
  catches reflection drift (renamed/dropped enums, defaults), not a new
  binary chunk that only a fresh Studio save would contain.
- [x] macOS build CI — a `macos-latest` job beside the Windows one:
  clippy, build and test. macOS is still not a supported target; the job
  only keeps the workspace from rotting there.

### From Roblox's own Creator Roadmap (2026 fall update)

Roblox's [Creator Roadmap fall 2026 update](https://devforum.roblox.com/t/creator-roadmap-2026-fall-update/4880208)
(read in full on 2026-09-22) added 72 items after RDC 2026. Every one of
them is **announced, not shipped**: the date after each is Roblox's own
target, and the same post pushed 32 earlier items back, some by more than a
year. None of this should be built against a guessed property name. Where an
item brings a new class, property or enum value, the work starts once it is
in the API dump and a real place file carrying it can be read. Until then
these bullets hold the idea so it isn't lost. They are grouped by the part of
this project each one would touch, not by Roblox's headings. The items that
give a Studio replacement nothing to build are listed at the end, so it is
clear they were considered and not missed.

#### Renderer
- [ ] 📋 **New primitive shapes: Capsule, Cone, Disc and rounded
  primitives** (Early 2027). These are new `Enum.PartType` values, and
  `scene::shape::part_type` sends any value it does not know to
  `ShapeKind::Box`. A place using one would draw as a block today without a
  word. Each shape needs its own mesh generator and has to go through the
  silhouette selection cue. Each also needs a decision on the Scale gizmo's
  `Alt` lock: a Capsule, a Cone and a Disc all have a round cross-section,
  like a Cylinder does. Which local axis is the length has to come from
  Roblox's own geometry once it ships, the way the Cylinder's did, and not
  from a guess.
- [ ] 📋 **An in-game orthographic `Camera` projection** (Late 2026), for
  2D, isometric and precision games. This is different from the editor's
  own Orthographic viewport toggle (see "What's been implemented" →
  Renderer), which is a view setting and never touches the file. This one is
  a property a place's `Camera` carries. Once it exists, the Properties
  panel shows it as an ordinary row, and anything that draws from the
  place's own camera honours it through the same
  `Camera::orthographic_projection` path.
- [ ] 📋 **Material layering and finer PBR response** (Mid 2027): PBR
  textures layered under custom masks, plus controls for specular response
  and more accurate scattering and reflection. This lands in the material
  pass that was calibrated against real captures (`rbx_materials`,
  `MaterialVariant`, `SurfaceAppearance`). It needs new captures from real
  Studio once it ships, the same discipline the lighting model was built
  under, and not a stretch of today's constants.
- [ ] 📋 **Emissive textures on UGC avatars, clothing and accessories**
  (Late 2026): a glow controlled by a texture. `SurfaceAppearance` is drawn
  today with no emissive term at all. Bloom already exists to carry the
  glow once the property does.
- [ ] 📋 **CSG on meshes** (moved from Late 2025 to Late 2026 in the same
  post). The from-scratch boolean (see CSG above) works on primitives. A
  `MeshPart` operand means feeding it `rbx_mesh` geometry instead. The open
  question is what a saved mesh union looks like in the file, which may be
  the same CSGMDL wall.

#### GUI (renderer and UI Editor)
- [ ] 📋 **Upgraded UI gradients** (Late 2026): radial and conic
  gradients, tile modes and scale controls. **Already drawn**: the GUI
  renderer reads `UIGradient.Type` (`Linear`/`Radial`/`Conical`),
  `TileMode` and `Scale` (see "What's been implemented" → Renderer). Two
  things are left. The UI Editor design panel's Fill gradient has no type or
  tile choice. And the property names should be checked against the dump
  once Roblox ships them, since the renderer reads them ahead of the
  embedded dump.
- [ ] 📋 **UI backdrop blur** (Early 2027): blur whatever is behind a GUI
  element, at a chosen strength or colour. The 3D `BlurEffect` pass already
  exists. A backdrop blur is that pass run over the region under the element
  before the element is composited on top, inside the GUI renderer.
- [ ] 📋 **Animated image containers and direct sprite sheet import** (both
  Late 2026). Containers group frames into named clips and play them as
  native 2D animation. Import brings a sprite sheet in with its loops and
  metadata attached. The GUI renderer already samples
  `ImageRectOffset`/`ImageRectSize`, which is one frame of a sheet.
  Playback runs into the same problem as the `ForceField` shimmer: it puts a
  clock in a pass that `--screenshot` needs to be repeatable. So the likely
  split is a still frame (the clip's first) in the viewer and live playback
  only in the UI Editor's preview. Import is an upload through `rbx_cloud`,
  like the 3D asset import item. The nearest existing relative is
  `ParticleEmitter`'s `Flipbook*` properties, which the particle renderer
  does not read yet either.
- [ ] 📋 **2D particles** (Late 2026): sparks, smoke and fireworks in screen
  space, on UI surfaces. The CPU particle simulation behind
  `ParticleEmitter` already exists in 3D. A GUI emitter would reuse it in
  screen space, with the same still-frame question as above.
- [ ] 📋 **Input action label** (Early 2027): UI that shows the right
  hotkey or button hint for the player's device. In the viewer and the UI
  Editor, the device picked by the Screen setting (the one both of them
  share) is the natural choice for which set of hints to draw.

#### Editor
- [ ] 📋 **Input action manager** (Late 2026): a visual editor for building
  and checking cross-platform control mappings. Unlike most of this list,
  what it edits already exists. The Input Action System's
  `InputContext`/`InputAction`/`InputBinding` are ordinary instances that
  Roblox has already shipped. That makes a contexts × actions × bindings
  table, one column per device, that writes real properties through the undo
  history, buildable now. It is the one item here that does not wait on
  Roblox. The embedded API dump predates those classes, so the dump needs a
  refresh first, the same as for `UIShadow`.
- [ ] 📋 **Branch and merge place files** (Early 2027), with conflict
  resolution at the property and script level. This project is better
  placed for this than most, because a place here is already a local file.
  The core is a three-way DOM merge: instances added, removed or reparented,
  properties changed, and a script's `Source` merged as text. Instances
  would be matched by `UniqueId`, which Roblox writes for exactly this kind
  of identity, rather than by file-local referents. The same merge is the
  heart of the Native Git integration item above, so it should be built once
  and used from both. Roblox's version is cloud-side; this one would be
  local.
- [ ] ⚠️ **Package overrides** (Late 2026): inspect, diff, selectively
  revert or publish deliberate edits to a package copy, from the Properties
  panel. The diff itself is local: a `PackageLink`'s subtree compared,
  property by property, against the package's published version. Getting
  that published version is the catch. It has to come from Roblox, and Open
  Cloud only serves the user's own assets, which is the same wall as
  Toolbox parity for everyone else's.
- [ ] 📋 **Project window** (Early 2027): game content organized into
  folders you can navigate, with readable paths, named asset references and
  automatic versioning. It overlaps the Argon/Rojo file-tree item (readable
  paths) and the asset manager below. This one should be decided against the
  shape Roblox actually ships, not against its one-line description.
- [ ] ⚠️ **Asset manager with a game inventory** (Late 2026): assets
  uploaded straight to a game's inventory instead of the account's. There is
  no asset manager panel here yet. Its upload path would be the 3D import
  item's Open Cloud client. The unknown is whether Open Cloud's Assets API
  gains a game inventory as a place to upload to.
- [ ] 📋 **Unified agentic permission** (Late 2026): one control over what
  Assistant, MCP, plugins and OCALE may access and do. This project plans
  both an MCP server (above) and a plugin API (see Plugins above). They should
  share one permission model from the start rather than each getting its
  own and being reconciled later.
- [ ] ⚠️ **Scene generation and a new texture generator** (Late 2026)
  belong under [AI content generation](#ai-content-generation) below. They
  have the same backend-agnostic shape, and a generated scene is, in the
  end, an ordinary instance tree written through the DOM.

#### Animation
- [ ] 📋 **Animation Graphs**: state-machine nodes and Luau expressions
  (Mid 2027), motion matching (Early 2027) and root motion (Mid 2027). None
  of the Animation Graph classes are in the embedded dump yet. The graph
  itself is authored data, so a node editor for it is reachable, and it
  could share its widget with the node-based scripting idea above. Running
  the graph is a different matter. Motion matching in particular is runtime
  behaviour that the local `KeyframeSequence` playback item could only ever
  approximate.
- [ ] 📋 **Avatar Schema changes**: procedural bones (Early 2027), higher
  mesh resolution (Mid 2027), more FACS controls including individual eyes
  (Mid 2027), and silhouette-preserving fit for clothing and accessories
  (Early 2027). These feed into Rig insertion and into the triangle budget
  the 3D import item checks, once Roblox publishes the new numbers.

#### Terrain
- [ ] 📋 **Terrain signed distance fields, virtual texturing, path splines,
  projected decals and scattering** (all Mid 2027). Every one of them
  depends on voxel storage (see Terrain above). SDF terrain may well
  *replace* the `SmoothGrid` format that this roadmap already calls its
  riskiest reverse-engineering item. That is a reason to see what format a
  Mid-2027 place file actually carries before starting on the old one. Path
  splines and scattering are tools for the Terrain Editor. Projected decals
  and virtual texturing are renderer work.

#### Play / Test
- [ ] ⚠️ **Multiple client views** (Late 2026), **Teleports in Studio** and
  **on-device testing** (both Early 2027), and **early testing** with up to
  10 friends (Late 2026). All four ride on the
  [sandbox design](#play--test-a-private-sandbox--probe).
  - Multiple clients means several real clients joined to the same sandbox
    server. Each mirror `LocalScript` already reports its own state.
  - Teleports need a second sandbox place in the same universe.
  - On-device testing works because the sandbox is already a real published
    place a phone can join. Debugging it from the editor goes through the
    probe's tunnel.
  - Early testing is the most directly useful. If a private game can grant
    friends access, testing as a team becomes a setting on the sandbox place
    instead of a new mechanism.
- [ ] 📋 **Client sessions & logs** (Late 2026) would feed the Output dock's
  sandbox-dependent half, if they can be reached over Open Cloud. **Audio
  debug tools** (Late 2026) have nothing to attach to: this project plays no
  audio at all today.

#### Nothing for a Studio replacement to build
- **Engine and cloud runtime**: the improved physics solver, efficient
  collision pipeline, collision summaries, native object interaction,
  improved navigation, Instance Streaming's adaptive radius and path
  pre-fetching, SLIM for NPCs and welded items, the 500-stud minimum draw
  distance, acoustic simulation, offline play, in-game creation persistence,
  push notifications, `QueueService`, compute functions, player data
  management, motion-sensor input and `OrderedDataStore` histograms. These
  run inside Roblox's engine or cloud (see
  [Explicitly impossible](#explicitly-impossible-without-robloxs-engine)).
  Whatever they leave in a place file, such as new properties or service
  instances, is read and round-tripped like everything else once it reaches
  the API dump. The Command Bar's Luau DataModel does not fake how they
  behave.
- **Platform, discovery, safety, monetization and Creator Hub**: name checks
  and promotional text, text-to-speech translation, Dutch support, quick
  words, badge improvements, every moderation and anti-cheat item, the Kids
  & Select changes, the Safety Callback API, the Wallet, passes surfaced
  outside the game, observability, the analytics agent and journey
  analytics. These live on Roblox's website and servers. Three of them touch
  work already planned here:
  - The **streamlined publishing flow** and the **pre-publish asset
    moderation signals** would show up in Save/Publish, if Open Cloud
    exposes them.
  - **Free trials for passes** is one more case for the monetization mocking
    layer below: a pass owned for exactly one session.

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
  real server authority possible from rbx-native. That covers Roblox's new
  Server Authority model too: client prediction, rollback and resimulation
  are engine behaviour, so only its settings can be edited here (see
  Play / Test).

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
