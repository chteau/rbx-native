# Changelog

## 2026-09-15

- **Orthographic camera mode.** The viewport's free-flight camera can now
  switch between perspective and parallel projection — toggled from the
  Viewport panel's overflow menu in `rbxstudio`, or `rbxview --orthographic`
  on the standalone viewer — reusing the existing WASD/mouse-look controller
  rather than a new interaction model. Building it surfaced a real bug along
  the way: the sky/star/sun background is drawn as near-unit-magnitude
  geometry that relies on the perspective divide to spread across the screen,
  which orthographic's constant `w` collapsed to a single point at screen
  centre, leaving pure black. Fixed by keeping the sky/star/sun background
  always perspective, decoupled from the main camera's own projection mode.
  Also threads the projection choice through shadow-map fitting
  (`Camera::frustum_corners`) and the depth-of-field pass's depth
  reconstruction (`post.wgsl`'s `view_distance`), both of which hardcoded the
  perspective-specific formula before this. — @chteau
- **A `Decal`/`Texture` now follows its part through the live-edit
  instance-patch path**, not just a full reload. The last of the three
  gaps the batch-move work surfaced: `Renderer::sync_instance` never
  touched the decal pass at all, and the decal pass itself had nowhere to
  touch — `FaceInstance` carried no referent, and `Textured` stored its
  batches as plain static buffers with no per-instance index. Gave it a
  `Keyed` index the same shape `filemesh` already uses, so a `CFrame`/
  `Size`/`Shape` edit re-derives and rewrites the part's Decal/Texture
  children in place instead of leaving them drawn at the old placement
  until the next reload. — @chteau

## 2026-09-14 (night) — getting ready to go public

- **Triplanar material projection.** Legacy `UnionOperation`/`NegateOperation`
  CSG output (see previous entry) has facets at arbitrary angles, and the
  renderer's material system picked one of six world-axis box projections
  per fragment — fine for an axis-aligned `Part`, but a facet near the
  boundary between two axis choices flipped projections between
  neighbouring triangles, and its axis-aligned tangent frame decoded the
  material's normal map far from the real geometric normal. Fixed with a
  real triplanar blend (`w = pow(|n|, 6)`, normalized; falls back to the
  original single-sample path when one axis dominates — a no-op, verified
  pixel-identical, on every ordinary `Part`). A second, smaller bug found
  in the same area: the tangent/bitangent frame was Gram-Schmidt'd
  independently against the normal, which keeps each perpendicular to the
  normal but not to each other once off-axis, shearing the decoded bump —
  fixed by deriving the bitangent from the tangent and normal directly.
  `DEMO_LIGHTING_MATERIALS.rbxl` pixel-diffed at 18/921,600 pixels
  (dithering-level) before/after — no regression on ordinary parts.
- **Windows fallback for the Open Cloud API key.** Same gap as the cache
  and settings paths fixed earlier tonight, found while writing the
  README's API-key documentation: `rbx_cloud::ApiKey::from_env_or_config`
  only checked `XDG_CONFIG_HOME`/`HOME`, no `%APPDATA%`. Added, same
  priority order as the other two (explicit override, then the OS
  convention, then `HOME` as a last resort).
- **Pre-public-release documentation and process, from scratch:**
  professional `README.md` (platform support table, build/test
  instructions, the 5 binaries, an Open Cloud API key how-to, a
  documentation map), `CONTRIBUTING.md` (branch-per-change convention,
  PR/issue workflow, an AI-agent-specific section), GitHub issue and PR
  templates, an MIT `LICENSE`, and a CI workflow
  (`.github/workflows/ci.yml`) that gates on Linux (fmt/clippy/tests) and
  builds+tests on Windows, on every branch.
- **A `ROADMAP.md` rewrite** into a TODO-checklist shape by category
  (format/scripting/renderer/editor/platform/tooling), split into what's
  done, what's planned — including a genuinely missing item found by
  checking the code rather than assuming: there is still no real script
  editor, `Script.Source` is just a Properties-panel text field — plus two
  new sections distinguishing what's flatly impossible without Roblox's
  own engine from what's reachable only via a deliberate workaround
  (the sandbox-place Play/Test design, and a monetization-testing mocking
  layer for gamepasses/developer products the user researched and asked
  to have documented).
- **An explicit, durable policy for autonomous AI contribution**:
  `agents/AGENTS.md` (moved out of the repository root, which still had
  tonight's own private working notes — machine-specific paths, an API key
  file location — that should never have gone public; the root now holds
  only a stub pointing at the real file) now explicitly authorizes an
  agent to pick anything from `ROADMAP.md`'s "What's planned", branch,
  implement, test, and open a PR without asking first, while forbidding it
  from ever editing `ROADMAP.md` itself. A shared, tool-agnostic procedure
  (`agents/workflows/roadmap-task.md`) covers the mechanics — checking for
  an existing branch before creating a duplicate, committing in small
  increments, merging `origin/main` back in periodically to avoid drifting
  into unresolvable conflicts, never force-pushing. A Claude Code skill
  (`.claude/skills/roadmap-task/`) and a `GEMINI.md` stub both point at the
  same procedure rather than duplicating it.

Verified: gate green, **1300 tests**. Nothing committed.

## 2026-09-14 (day, continuation 6) — rendering: Workspace scoping, materials, beams, plus two false positives ruled out

Six agents, all independently verified (gate + capture).

- **The viewport was rendering everything, not just `Workspace`** — confirmed and
  fixed. `ReplicatedStorage` alone, on `marked.rbxl`, contained 6663
  `Part` + 482 `UnionOperation` + 516 `WedgePart` + 381 `MeshPart`, all
  rendered incorrectly (vs ~292 elements actually in `Workspace`). New
  `scene::workspace_descendants` used by all entry points that
  construct visible geometry (parts, unions, meshes, trails,
  particles, textures, lights, local `Terrain`); global service lookup
  (`Lighting`, `MaterialService`, `Sky`) deliberately remains
  on entire DOM. Proof by positive control: a magenta Part injected
  into `ReplicatedStorage` via `rbxlua` correctly disappears from rendering after the fix.
- **Lighting too flat on materials — genuine constant drift bug.** `PLASTIC_SPEC_STRENGTH` (0.033) and `MATERIAL_SPEC_STRENGTH`
  (0.08) had diverged even though a comment claimed they were
  supposed to be equal. Since ~95% of parts in `marked.rbxl` (fountain,
  trees, bushes) are `Plastic`, it was this sole constant that
  flattened almost the entire scene. Unified to `0.2`, duplicate removed.
  Reflections now visible on the fountain rim, canopy, walls —
  confirmed by before/after capture. Saturation verified separately
  already correct (already fixed by corrections earlier tonight) —
  no spurious second bug invented.
- **Beam texture orientation** — genuine bug confirmed against
  official documentation and actual texture used by 8 Beams on `marked.rbxl`:
  the axis that should carry repetition along the beam (u vs v) was
  inverted. Fixed (UV + sampler addresses swapped); dotted lines now
  trace correctly around the full perimeter instead of appearing
  compressed at one end.
- **Free camera smoothing** — translation and scroll wheel smoothed by
  exponential decay independent of framerate (unit tested);
  mouse look remains instantaneous, as requested.
  Time constant = comfort choice (~180 ms), not a
  Roblox value — to be judged in practice. Tunable sensitivity added to ROADMAP for
  later.
- **Two false positives ruled out after investigation, no forced fix**:
  - Audit of Roblox Enums in Properties found no bug — the
    logic is already generic (tested on ~16,700 real instances,
    dropdowns confirmed for `Camera.CameraType`, `Humanoid.CollisionType`,
    `Model.LevelOfDetail`, etc. beyond just the known `Material` case).
  - SurfaceGui positioning was already correct (fixed earlier in the
    evening, before the task even launched) — verified algebraically
    on 6 faces + actual capture. What the user's capture showed (the `<MAPNAME>` / `Votes: 0` panel) is actually the gap
    **text** already documented and deliberately deferred (`TextLabel`/
    `TextButton`, no rendering dependency on chosen font) — not a
    positioning bug, confirmed by dumping the real instance.
- **"Rock" Unions still boxes — documented limitation, not a
  regression.** The 13 instances of asset `394314025` on `marked.rbxl`
  resolve correctly (no API key issue — anonymous client already works)
  but the asset itself decodes to 1 additive `Part` + 28
  `NegateOperation`; resolver retrieves additive primitives only by design (no true CSG subtraction), so a
  "carved" block gets a solid box, as expected — not new.
- **Edit lag (~10 s) — partially improved.** Profiled: complete reload of `marked.rbxl` takes ~0.9-1.15 s cold (35% CPU
  scene reconstruction, 55-60% GPU re-upload) — not the ~10 s reported,
  which likely comes from elsewhere (network, quality, another unprobed path).
  Workspace scoping fix already mechanically reduces
  each reload by 28× fewer parts and 91× fewer duplicated union plane entries
  on this fixture. True incremental patch per
  instance (instead of complete rebuild) remains to be done — not attempted tonight to avoid
  file collision with materials agent.

Verified by me independently after the 6 agents finished: green gate,
**1247 tests**, no forgotten scratch files, fresh capture confirming
material reflections and overall correct functioning.

## 2026-09-14 (day, continuation 5) — GUI pass: menu, dock chrome, live camera, typed Properties, Output

Four agents, on disjoint file areas then sequenced where
they overlapped (`shell.rs`/`shell/dock.rs`), all independently verified
(gate + capture) before being marked complete.

- **File/Edit/Model/View menu bar** — built on `AppMenuBar` from gpui-component (no custom component). Save/Undo/Redo/Insert/Delete wired to
  real existing handlers (no duplicated logic); everything else (New/Open/Save As/Publish, Cut/Copy/Paste, View) honestly disabled placeholder
  (`MenuItem::disabled(true)`), no false functionality that looks working.
- **Viewport camera → `Workspace.CurrentCamera`** — free flight pose
  is now directly reflected in CFrame/FieldOfView of the DOM,
  throttled at 200 ms and deduplicated (not every frame). Written outside
  undo system (`History::push` never called on this path) —
  hovering the scene must never pollute the undo stack. Ctrl+S
  thus persists the camera as we left it, like in Roblox Studio.
- **Dock chrome** — Viewport tab now displays the filename (`DEMO_LIGHTING_MATERIALS.rbxl`) instead of a false duplicated internal tab; Explorer and Properties no longer have double headers; "Show all
  services" moved to Explorer burger menu (checkbox); Properties frozen
  height fixed (list now truly fills the resized dock); tab bar recolored gray/smaller
  to contrast with dock interior. Confirmed: this version of
  `gpui-component` exposes no mechanism to detach a dock to
  separate window — nothing to fix, already enforced by design.
- **Properties: widgets by type + categories** — `rbx_reflection` now
  exposes `Category` for each property (missing until now, present
  in raw dump). Panel groups properties into collapsible sections
  (Appearance, Transform, etc.), with one widget per real type:
  checkbox (Bool), color picker (Color3/Color3uint8), dropdown (Enum), numeric
  X/Y/Z fields for Vector3 and CFrame position.
  `NumberSequence`/`ColorSequence`/`Rect`/`PhysicalProperties`/
  `Font` remain read-only (deferred, as planned). `BrickColor`
  edited by raw palette index — no table of ~140 Roblox
  colors exists in the repo, risky to fabricate one was not worth it.
- **"Output" dock** — new tab stacked with Properties: history of
  Luau executions from the Command Bar (max 200 entries), Clear button, All/Output/Errors filter (two honest levels —
  `rbx_lua::Runtime::run` only distinguishes success/error, no false "warnings" category),
  recall a past command by clicking its entry (up/down arrows in the Luau field are
  already taken by component text editing, so no concurrent keyboard shortcut).
- **Portability bug found and fixed along the way** (dock Output):
  a `use gpui_kit::*;` at file head brought `gpui::test` into
  wildcard in the test module, shadowing `std::test` — applied to a test without GPUI arguments, it triggered a macro expansion that
  crashed `rustc` (SIGSEGV) once the recursion limit was lifted while digging into the problem. Fixed with explicit imports rather than a
  global `recursion_limit` hack.

Verified: green gate, **1234 tests** (up from 1198 before tonight). Final capture sent to user, everything confirmed consistent despite 4 concurrent agents on dense GUI session.

## 2026-09-14 (day, continuation 4) — sunlit wall: two more leads ruled out, one new opened

Follow-up to previous entry. A second dedicated agent, fed a new
reference capture showing contrast between faces differently
oriented to the sun, measured that contrast at ours (~2.4-2.7×) vs
reference (~1.9-2.4×) — same order of magnitude, not a directional contrast deficit. It instead isolated a precise numerical gap
(ambient read on GPU/screen ≈0.407 vs ≈0.258 calculated by hand) without
having time to locate it, and honestly reported it without guessing a fix.

I verified that point myself directly (temporary instrumentation of
`Lighting::assemble`, removed immediately after): **the real CPU value
is 0.258, identical to hand calculation.** The 0.407 gap found by
the agent was thus an artifact of its measurement method (recalculate a
linear value from a capture pixel already tone-mapped/encoded), not
a code bug — this lead is closed.

I then verified a tonemap hypothesis (a non-linear Reinhard curve
could distort channel ratios): `post.misc.x` (which
selects Reinhard) is enabled only by a `ColorGradingEffect` with
`TonemapperPreset = Retro`, absent from `marked.rbxl` — MARKED uses simple clamp, which does not distort ratios below 1.0. Lead ruled out too.

**New lead opened, not verified**: the wall in question bears its own
`MeshPart.TextureID` (not the shared 43-material pack — the `Plastic` branch of the shader uses `base_albedo`, not pack texture),
described as "a small atlas of solid tints". Mip generator
(`texture::mip_chain`, naive box filter over entire image, no knowledge of regions) would mathematically blend neighboring tints
from such an atlas once a low enough mip level is selected for that
face — standard mipmapping behavior, not a bug per se, but could
explain a per-channel offset if the sampled swatch is small in
pixels. Unverified: actual texture file dimensions, mip level
effectively chosen at this render distance, colors of neighboring swatches. Left as-is for tonight — check with user for next steps.

## 2026-09-14 (day, continuation 3) — sunlit wall too dark: honest investigation, no fix

Lead opened by user: the canyon wall directly exposed to
the sun remains too dark (measured ~121 at ours vs ~168 at Roblox
Studio, same camera). A dedicated agent dug in and **did not apply a fix**, by deliberate choice rather than lack of effort — what it found
makes the initial hypothesis (`SUN_BASE` under-calibrated, cf. previous entry) false,
proven algebraically, not just empirically:

- Measure by channel on 4 clean wall samples: target R/G/B ≈
  168/122/77, ours ≈ 117/93/78. **Blue already matches almost exactly**
  — only red and green fall short.
- Any *colorless* boost (including raising `SUN_BASE`) would push
  blue beyond its correct target to barely fill the red gap —
  verified empirically (forcing `EnvironmentDiffuseScale = 1` adds
  ~+20/+23/+22 uniformly to all three channels, which cannot satisfy
  two different target ratios at once).
- Ruled out by real bisection: `Terrain` (never drawn, not voxel here),
  missing asset (`29242300`, unrelated sign decal),
  vertex color of mesh (real bug found in passing — `rbx_mesh` parses it but `filemesh/vertex.rs` never uploads it to GPU — but the wall in
  question has white vertices, so not the cause here),
  `ColorCorrectionEffect.TintColor` (neutral).

Remaining lead for next time, unverified: a hue/texture offset of the wall's
material (likely `Rock` or similar) rather than the lighting model itself — an offset that depends on channel like this one resembles
more a mishandled albedo texture or material tint in color space
(linear vs sRGB) than a scalar light formula. Not handled tonight. Gate unchanged (1198, nothing modified).

## 2026-09-14 (day, continuation 2) — the real fidelity fix on MARKED

The reported gap ("way too much fog, colors darker and less vivid")
remained substantial after the first Atmosphere fix (see below) —
rightly so: part of the perceived gap was a release binary not
rebuilt since shader edit (`.wgsl` files are compiled inline via
`include_str!`), but once rebuilt, two genuine additional bugs emerged
from property-by-property bisection, camera locked to the exact
Studio CFrame provided by the user:

- **`ColorCorrectionEffect` was graded in linear space instead of
  display space.** A `+0.1` Brightness added before `*Srgb` encoding raised black to sRGB 89 — all walls/shadows turned gray.
  Roblox grades the already tone-mapped frame. Fix: tone map first, gamma 2.2 conversion, grade, back to linear (`renderer/post.wgsl`). This was the dominant source of gray veil.
- **`OutdoorAmbient` not clamped to `Ambient`**, contrary to
  official docs ("*the effective OutdoorAmbient value is clamped to be greater
  than or equal to Ambient in all channels*", verified verbatim via `gh api`
  on `Roblox/creator-docs`). On MARKED, `Ambient` (139) exceeds
  `OutdoorAmbient` (70) — shadowed zones were ~2.5× too dark.
  Fix in `lighting.rs`, with dedicated test.
- `ATMOSPHERE_DENSITY_SCALE` tweaked a second time (0.00265 → 0.0013) once the two bugs above were fixed — residual haze on distant walls remained too pronounced. Honest empirical compromise documented:
  DEMO_LIGHTING_MATERIALS ends up very slightly under-hazed in return
  (Studio 114/131/153 vs ours 104/116/138 on its distant plane, vs
  99/110/129 after this adjustment) — a non-linear response to `Density` would
  probably be needed to tune both fixtures perfectly, left as-is for lack
  of a third real calibration point.

Verified: green gate, **1198 tests**, camera-locked capture compared to
reference Studio screen — grass/water/paving/walls now close at
first glance; remains a brightness offset on wall directly
exposed to sun (168 at Roblox vs 121 at ours), which is a distinct
topic (direct lighting/sun orientation) rather than
atmosphere, not handled here.

## 2026-09-14 (day, continuation) — post-FX fidelity on a real place, and cleanup before open-source

- **Fix**: rendering of `marked.rbxl` (a real rich place, not a
  test fixture) was noticeably more blurry/washed-out than real Roblox Studio.
  Diagnosed by bisection (scripted copies via `rbxlua`, property by
  property) rather than by eye: dominant cause was neither `Haze` nor
  `DepthOfFieldEffect` (both initial suspects, which proved to have almost no
  measurable effect here), but `Atmosphere.Offset = 0`
  on this place (0.25 on the only already-calibrated fixture) — never tested
  against real screenshot before. `ATMOSPHERE_DENSITY_SCALE` halved in
  consequence (`renderer/atmosphere.wgsl`), documented as an
  empirical compromise between the two now-known calibration points,
  not a verified formula (Roblox publishes none).
- **Bug found alongside**: `EnvironmentSpecularScale` did gate the
  roughness-dependent environment specular term, but not the fade of
  `Reflectance` sampling the same environment probe — a
  `BasePart.Reflectance > 0` thus reflected like a mirror even when the place
  had `EnvironmentSpecularScale = 0`. Fixed (`renderer/lighting.wgsl`).
- Verified: green gate, fresh captures of both fixtures compared to
  Studio screenshots (canyon wall recovers its warm tint instead of washed-out gray; already-faithful fixture remains unchanged).
- **Cleanup before making repo public**: overly long comments
  compressed to essentials (WHY, not full history) on a dozen
  files; any mention in comments of maintainer's private fixtures (`DEMO_LIGHTING_MATERIALS`, `marked.rbxl`, `testrust_*`) removed
  or generalized — a future contributor has no way to obtain these
  files. A whole module bore their name (`textures/sky/seams/marked.rs`)
  → renamed `real_capture.rs`.
- **Portability bug found during this pass** (not just cosmetic):
  `crates/rbx_binary/tests/shared_string_round_trip.rs` embedded
  `marked.rbxl` via `include_bytes!` on an absolute path specific to this
  machine — the crate could not compile its tests anywhere else, including CI. Replaced with runtime read behind
  `#[ignore]` + environment variable `RBX_ROUND_TRIP_FIXTURE`, on the
  same pattern already in place for `RBX_MESH_FIXTURES`/`RBX_UNION_FIXTURE`/
  `RBX_SKY_FIXTURE`. Similar test in `scene/union/tests.rs` (hard path,
  but already `#[ignore]` so no compile breakage) aligned to same variable; its name, which also contained
  `marked_rbxl`, renamed.
- Test count reference: **1197** (not 1198) from now on — the one-unit
  drop comes solely from the above test moved to `#[ignore]` opt-in, not a regression.

## 2026-09-14 (day) — the viewport no longer freezes at rest

- **Fix**: the 3D view in `rbxstudio` only redrew on camera
  movement (« camera at rest = no render »), which also froze all animated content unrelated to camera — particles,
  `Trail`, `Clouds` — as soon as the user stopped interacting.
- The render thread (`workspace_view/pump.rs`) now draws continuously,
  paced to screen refresh, as long as the panel is actually
  visible — camera at rest or not — and goes completely idle (no
  render, no GPU read) only when it is not.
- Visibility detected via a new signal (`WorkspaceView::advance`/
  `painted`): the dock (`gpui_component::dock`) only mounts the active tab
  of a group, so a hidden Viewport tab never calls `render`
  — that is what triggers idle, with a delay of about one
  frame budget to absorb normal jitter between a frame ready and
  its actual paint by GPUI.
- Verified in real conditions: `RBX_STUDIO_STATS=1` (enabled by default in
  debug) shows 75/75 fps continuous on `DEMO_LIGHTING_MATERIALS.rbxl` with
  no input since launch, whereas before the fix no stats line
  appeared at rest (0 frames drawn). Gate unchanged,
  1198 tests.
- **Known limitation, not resolved**: nothing yet detects a
  *minimized* window specifically (no occlusion API exposed by the GPUI version used) — only switch to another dock tab is
  covered. Behavior on minimize/collapse depends on compositor.

## Night of 2026-09-13 to 2026-09-14 — from « viewer » to functional editor

Thirty tasks delivered autonomously by a fleet of subagents (common rules in `AGENTS.md`), each independently verified (clean gate +
captures/tests) before marked complete. Result: **1198 tests,
clean gate** (`fmt` / `clippy -D warnings` / tests).

### Format foundation
- **Binary write round-trip** (`rbx_binary::serialize`), symmetric to
  existing reader, with `SSTR` table of shared strings (MD5 deduplication). Exact round-trip verified on 4 real fixtures, including
  `marked.rbxl` (16k+ instances).
- **XML `.rbxlx`/`.rbxmx` format** (`rbx_xml`), read AND write,
  exact round-trip.
- **`rbxdump --roundtrip`** — new continuous verification tool that
  found and got two real regressions fixed tonight (see
  « Fixes » below).

### Scripting
- **`rbx_lua`** (new crate): Luau VM sandboxed (`mlua`), in-house
  DataModel (`Instance`/`game`/`workspace`), datatypes Vector3/CFrame/Color3/
  UDim2/NumberSequence/ColorSequence/Rect/PhysicalProperties/Font/Content,
  runnable in CLI (`rbxlua`) and from the editor (**Command Bar**).

### Rendering (`rbx_viewer`)
- DDS decoder (BC1/2/3) + default Roblox sky.
- `TrussPart` rendered as truss (rails + braces per `Style`).
- `UnionOperation`/`NegateOperation` legacy rendered with their **real
  original parts** (fallback via `AssetId`, ~13 rocks retrieved on
  `marked.rbxl`) rather than an empty box.
- Cast shadows for `SpotLight`/`SurfaceLight`.
- **Particles** (`ParticleEmitter`, CPU simulation + billboards,
  `NumberSequence`/`ColorSequence`).
- **`Beam`** (textured ribbons between attachments).
- **`Trail`** (implemented and tested, correctly inert as long as no
  animation moves an attachment).
- **GUI**: `ScreenGui`/`Frame`/`ImageLabel` (2D overlay), `BillboardGui`,
  `SurfaceGui` (text not handled, font render dependency
  deliberately left out).
- Procedural `Clouds` on skybox.

### Editor (`rbx_studio`)
- Explorer with **real Roblox icons** (downloaded at runtime,
  never committed) and filter of non-« browsable » services by default.
- **Selection** with highlight box in viewport.
- **Editable** Properties panel (scalar types + Vector3/Color3/UDim2/
  Enum/CFrame position).
- Instance insert/delete (Ctrl+Shift+P/F, Del).
- Rearrangeable panels (`gpui_component::dock`).
- **Save** (Ctrl+S, rewrites in original format, binary or
  XML, atomic write).
- **Undo/Redo** (Ctrl+Z/Y, `WeakDom` snapshot stack, reversal
  verified bit-for-bit).

### Texture fallback
- `rbx_assets` can fall back to Android Sober APK (if installed)
  when `content-textures2/3.zip` lacks a requested file —
  never default source, never auto-install without
  explicit caller request.

### Fixes
- **DepthOfFieldEffect**: `InFocusRadius` was treated as a diameter
  (`/ 2.0`) while official Roblox docs specify it is a radius
  on each side of `FocusDistance`. Fixed in shader and its Rust mirror,
  3 tests recalculated.
- **BlurEffect**: "first activated wins" selection replaced with "largest
  `Size` wins", conforming to official docs (only effect documented to
  detail this behavior).
- **Bloom**: `Size = 0` did not fully disable blur (1px residue); fixed to return null intensity.
- **XML `Unknown`-type**: an unknown-type blob was silently corrupted on re-read (hardcoded type_id). Replaced with honest rejection (`XmlError::Unsupported`) for any unproven case with loss —
  verified against `xml.md` spec from `rojo-rbx/rbx-dom`.
- **Missing binary `SSTR`**: `rbx_binary::serialize` wrote no
  shared string table, breaking round-trip on real files
  mixing `SharedString` and regular strings on same property.

### Compliance / Legal
- Added **non-affiliation disclaimer** at top of `README.md`.
- `class_icons.json` (Roblox icons) **removed from repo**: icons
  now fetched at runtime from already-credited `Roblox-Client-Tracker`
  mirror, cached locally — no proprietary assets committed.

### Roadmap
- Added **« Native Windows compatibility »** lead.
- Added **« Clean render capture with ray tracing »**
  lead (viewport screenshot without UI, offline path tracer) — documented,
  deliberately deferred to very end.
- `ROADMAP.md` fully updated to reflect real post-night state.

### Not done / Deferred
- Interactive gizmos for translate/rotate/scale in viewport.
- GUI text (`TextLabel`/`TextButton`/`TextBox`).
- `Light.Shadows` for `PointLight`.
- Live publish via Open Cloud (Save locally is done; publish is not —
  action deemed risky for unsupervised run).
- Real CSG (decode `MeshData`/CSGMDL format, proprietary).
- Terrain, rig/animation, shimmer ForceField, refraction Glass.
