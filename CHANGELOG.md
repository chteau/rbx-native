# Changelog

## 2026-09-16

- **An edit is a patch of the instances it touched, never a rebuild.**
  Roblox's engine never reloads its scene: the DataModel is the live
  picture, and a property write is an event applied to that one instance.
  The editor now works the same way. `Headless::apply_changes` takes the
  `Change` log a mutation produced — every `Added`, `Removed`, `Property`
  and `Parent` the DOM recorded — folds it by instance, looks each one up
  in the DOM as it stands *now*, and patches only those: a part's box or
  mesh is re-derived and its opaque/blended record, shadow caster and
  outline rewritten, moved between batches, added or dropped (the box-part
  removal that never existed now does); the decals, lights, emitters and
  attachments hung off it follow; a `Model` reparented reaches its whole
  subtree, in or out of `Workspace`; a light, an effect list or a GUI
  canvas set is re-planned once per batch however many members changed.
  Because the log is read as *which* instances to look at rather than
  *what* happened to them, undo hands over the very log its mutation
  produced against the restored DOM — the `Added` of an insert, applied
  after the undo, finds the part gone and takes it out — so undo and redo
  of anything take the same path as the edit. On the editor side that
  closes every reload that was left: the Explorer's insert, delete and
  multi-instance drag-drop, a Command Bar script touching any number of
  instances, a Properties edit on a class the old classifier did not know
  (every `Sky` edit aside), a `Script`'s `Source` on save, and undo/redo of
  each; `shell::edit`'s `classify_edit`, `shell::command`'s one-instance
  classifier and `reload_viewport` itself are gone, and the render thread
  takes one `Command::Changes` — one DOM clone, one pass — where it took a
  command per instance. The GPU side batches too: an instance buffer marks
  the slots an edit touched and uploads the span once before the next
  frame (`renderer::slots::Slots::flush`), where each patched instance used
  to cost its own staging buffer. What still rebuilds the whole scene is
  the closed list in `rbx_viewer::Rebuild`, one variant per reason: a `Sky`
  edit (its six panels prefilter the environment probe — a few
  milliseconds, kept on purpose), a `MaterialVariant`/`MaterialService`
  edit (the material catalog is defined from the service), an asset this
  renderer never uploaded (a mesh, texture, material pack or
  `SurfaceAppearance` set — fetching one is a load-time path today), and a
  failed-CSG union drawn as its fallback pieces (they share the union's
  referent). Measured with `scripts/bench.sh` on `marked.rbxl` (16 742
  instances, assets on, 1280×720, RTX 4070, medians of 50, first frame
  readable / call returned): one part's `CFrame` 1.04 ms / 0.04 ms, insert
  1.07 / 0.03, undo insert 1.07 / 0.03, delete 1.02 / 0.03, undo delete
  1.05 / 0.03, 100 parts moved in one script 3.56 / 2.28 (7.53 ms before
  the coalesced upload; the rest is the one canvas re-plan a
  `SurfaceGui` among them costs), undo of that 3.63 / 2.34 — against a
  full reload of 30.1 ms on the same run — all of them phases of the
  harness now, not a one-off. For every kind of change — insert, delete, move, a 100-part
  move, a reparent inside `Workspace`, a reparent out of it, a script
  touching several instances, and the undo and redo of each — the patched
  frame is pixel-identical (AE 0, uploads drained) to a cold rebuild of the
  same DOM, on both `TestPlace.rbxl` and `marked.rbxl`
  (`crates/rbx_viewer/tests/patch_parity.rs`, `--ignored`, needs a GPU).
  `read_place` now hands back a DOM with an empty change log: a parser
  builds the tree through the same calls an edit uses, and the log of its
  own construction is not an edit. — @chteau

- **A full reload no longer starts over.** `Headless::reload` — what a
  Command Bar script, an undo the fast paths cannot classify, or any edit
  they refuse falls back to — used to be a cold load in all but name: it
  threw the whole `Offscreen` away and opened a brand-new wgpu instance,
  adapter, device and queue, compiled every pipeline again, and, before any
  of that, decoded every decal image, material pack, file mesh and union
  asset from the disk cache again and carved every union's boolean again,
  even though the edit had changed none of them. Two things now outlive a
  reload. `Headless` keeps a `load::Resident` of everything its assets
  decoded to, keyed by asset reference, so a reload decodes only what the
  place never showed before — failures included, whose warnings are answered
  again each time so the Output dock reads as it did; the union booleans are
  kept the same way, and the decoded images ride along behind `Arc`s rather
  than being copied into each scene. And the renderer is rebuilt in place
  (`renderer/rebuild.rs`): only what the scene decides — instance buffers,
  shadow casters, effect lists, GUI canvases, the selection's placements —
  is redone, while every asset-keyed upload stays under a key that says it
  would be uploaded identically (the material arrays by the catalog's
  resolved maps, the environment probe and skybox by the six panel assets,
  the sun and moon by image and angular size, decals, mesh textures,
  `SurfaceAppearance` sets and file-mesh geometry by asset and skin). A
  `Sky` edit still rebuilds the probe; a material on one part still does not
  touch the pack. Measured with `scripts/bench.sh` (1280×720, first frame
  readable, median of 25 reloads, same machine as `BENCHMARKS.md`):
  `marked.rbxl`'s full reload went from 1092 ms to 30.6 ms (p95 46.6 ms)
  with assets on and from 404 ms to 13.8 ms without; `TestPlace.rbxl` from
  568 ms to 1.7 ms and 280 ms to 1.4 ms. With a part moved between every
  reload (a throwaway harness, 1500×900, 12 iterations) `marked.rbxl` went
  from 1009 ms to 34 ms median, 1125 ms to 56 ms p95. Frames after a reload
  that moves parts, changes materials, edits the `Sky` or edits `Lighting`
  are pixel-identical to a cold load of the same DOM on both fixtures. What
  is left of a reload is `Scene::from_dom` itself, ~15 ms on 16k instances.
  — @chteau

- **Undo/redo of a Scale drag no longer reloads the scene.** The fast path
  below classified an undo step as "exactly one property write on one
  instance", and a Scale drag never is one: Studio's Scale tool holds the
  face opposite the grabbed one still, so every step writes `size` *and*
  `CFrame` on the part together (see `shell::drag`). Every Scale-then-undo —
  of a single part, too — therefore fell back to a full `reload_viewport`,
  which on a large place made undo look broken. `shell::command`'s
  classifier now answers "one instance, and every property written on it"
  instead of "one write", and the new `Shell::reflect_changes` patches each
  of those properties in place — the same per-property loop the live drag
  already ran each frame — for undo/redo and a Command Bar script alike. A
  multi-part drag, an instance create/delete, or a script touching a second
  instance still reloads, deliberately. `RBX_STUDIO_RESIZE=<dx>,<dy>,<dz>`
  joins `RBX_STUDIO_DRAG` as the debug aid that stands in for a Scale drag,
  so a `RBX_STUDIO_UNDO=1` run can prove it. — @chteau

- **Undo/redo fast path.** `Ctrl+Z`/`Ctrl+Y` used to reload the whole scene
  on every step, however small the reverted edit — undoing a single
  `Transparency` change cost exactly as much as undoing an instance delete.
  `shell/history.rs` now pairs each pushed `WeakDom` snapshot with the
  `Change` log the mutation right after it produced, and reads it back
  through `shell::command`'s own `single_change` classifier — the same one
  a Command Bar script's viewport reflection already uses — so undoing or
  redoing a single property write or reparent patches the GPU state in
  place instead. An instance create/delete, a multi-instance drag, or
  anything else `single_change` can't classify still falls back to a full
  reload, correctly. — @chteau

- **A real script editor.** Double-clicking a `Script`, `LocalScript` or
  `ModuleScript` in the Explorer now opens it in a new Script Editor dock
  panel — tabbed, one tab per script, each closable — instead of leaving its
  code in a one-line Properties field. Luau syntax highlighting comes from a
  hand-written lexer behind GPUI Kit's own `InputHighlighter` seam rather than
  a tree-sitter grammar: no new dependency, and Luau's type annotations,
  `continue`/`export`/`type`, compound assignment, `0b` literals, digit
  separators and backtick interpolation all lex correctly where a Lua 5.1
  grammar sees a parse error and stops colouring the rest of the file. Two
  things a flat token stream cannot decide get a pass of their own on top: a
  type annotation's names are told apart from an expression's (so
  `local n: Vector3` colours `Vector3`, while the colon in `obj:method()` is
  left alone), and the `{...}` holes inside a backtick string are lexed as the
  Luau expressions they are, by re-entering the lexer on them. Edits
  land on `Source` through the same `set_property` every other property edit
  uses, 400 ms after typing stops, so a typing burst is one undo step; Ctrl+S
  and undo/redo flush any pending write first. `Source` became read-only in
  the Properties panel for scripts — that panel's commit path trims what it
  writes, which was quietly eating a script's trailing newline. The saved dock
  layout is now `dock_layout_v2.json`, since a layout written before this
  panel existed would have hidden it rather than failed. — @chteau
- **Dock layout persistence.** Dock panel position, size, and docking state
  (floating vs. docked) now persist across restarts. Uses `DockAreaState`
  from `gpui_component::dock` to capture the full layout on
  `DockEvent::LayoutChanged`, persisting to `~/.config/rbx-native/dock_layout.json`.
  Verified: rearrange panels, restart, layout is restored. Panels cannot
  currently be closed (they're marked non-closable), so panel visibility
  persistence is a follow-up feature blocked by making panels closable first. — @chteau
- **Drag-and-drop reparenting in the Explorer.** Dragging a row onto another
  moves the instance under it, which is the whole of what Studio offers here —
  creator-docs' Explorer page says only "to change the parent of one or more
  children (reparent), simply drag and drop them onto the new parent", and
  there is nothing to drop *between* two rows because a place has no
  user-orderable sibling order to rearrange in the first place. A ghost follows
  the cursor, the row under it lights up only while the drop is actually legal,
  and the new parent is expanded and scrolled into view afterwards so a drop
  into a collapsed branch does not read as a delete.
  `WeakDom::set_parent` validates nothing, and parenting an instance under its
  own descendant would cut that whole subtree loose from every root while
  leaving the cycle intact — nothing would ever draw it again. So the rules are
  the feature: a drop is refused onto the dragged instance itself, into its own
  subtree, onto the parent it already has, and for a service (Roblox creates
  one of each under the DataModel and Studio will not move them). One rule
  answers both the highlight and the drop, so an illegal target never lights up
  and nothing slips past the one that does. One drag is one `Ctrl+Z` however
  many instances it carried, and a single instance takes the cheap viewport
  path a script's `part.Parent = model` already uses rather than a scene
  rebuild. `WeakDom` grew a `parent` accessor for it: the reverse edge was
  already maintained and simply unreadable, and walking a chain by depth is
  what makes the ancestor check cheap enough to run per row per frame.
  Dragging a row that is *not* in the current selection collapses the selection
  to that row before the drag begins, so a multi-instance drag carries the
  whole selection only when grabbed by its anchor — the tree widget tracks one
  selected row and already behaves that way for a plain click; untangling it
  belongs with the fuller Explorer editing work. — @chteau
- **`GuiObject.Rotation` now renders.** A `Frame`/`ImageLabel`/text
  element's background, border and image all turn together about the
  element's own centre — never its `AnchorPoint`, which Roblox's own docs
  say can't be done — and `ClipsDescendants` is skipped wherever the element
  or an ancestor has a non-zero `Rotation`, matching the primary source's
  own description of the two properties as incompatible. Nested rotation
  (a rotated element's children swinging around with it, the way
  `GuiBase2d.AbsoluteRotation` implies real Studio composes it) is still
  open. — @chteau
- **Fixed the CPU (and GPU upload) spike loading a real place.** Profiling a
  synthetic CSG-heavy place (this repository ships no real one) showed
  resolving 40 legacy `UnionOperation`/`NegateOperation` booleans took ~6s
  single-threaded, dwarfing the ~0.1s an equivalent number of texture mip
  chains took to generate — the from-scratch BSP boolean was the dominant
  cost, though that mip-chain number only measured CPU-side generation, not
  the GPU upload itself, which turned out to be its own real spike.
  `union::resolve` now evaluates each distinct asset's boolean across a
  bounded worker pool sized to CPU parallelism, the same synthetic place
  resolving in ~0.8-1.1s afterward. Separately, every `Decal`/`Texture`
  image used to upload its full mip chain to the GPU in one uninterrupted
  burst before the renderer was usable at all; `Renderer::draw` now spreads
  that upload across a bounded number of images per frame instead
  (`texture::PER_FRAME`), each slot starting from a cheap placeholder until
  its turn comes up. Measured directly on a real GPU: uploading 60 synthetic
  1024x1024 images in one burst took ~245-295ms, versus every single frame
  staying under ~33ms once spread across 8 of them. The one-shot `rbxview
  --screenshot` path drains any remaining upload immediately instead, since
  it has no next frame to spread the rest across. — @chteau

- **Performance is a measured quantity now: `scripts/bench.sh` and
  `BENCHMARKS.md`.** Until today the only reload timing that existed anywhere
  was the hand profile written into this file — a number nobody could re-run,
  so nobody could tell whether a change had helped. `crates/rbx_viewer/examples/bench`
  times cold load, full reload, single-instance patch and steady-state frame
  cost at each quality level, through `Headless`'s public API only, and prints a
  table plus a JSON file carrying every sample, the adapter, the commit and the
  iteration counts. Two numbers per operation, never added together: when the
  call returned, and when the first frame it queued was actually readable —
  a reload returns with its GPU work still in flight, and a wall clock around
  the call alone would flatter it by 26-49ms. Median and p95, never a mean.
  The recorded baseline reproduces the old hand profile (1.09s to a readable
  frame on `marked.rbxl`, against ~0.9-1.15s profiled by hand) and decomposes
  it: ~245ms is `Offscreen::new` opening a fresh wgpu device and rebuilding
  every pipeline, ~132ms is the 16 742-instance scene itself, and ~665ms is
  re-resolving textures, materials and meshes the previous scene already had
  resident. That last part is also the only unstable one — bimodal, 15%
  run-to-run — so the baseline is recorded both ways and `--no-textures`
  (6.6% run-to-run) is what a before/after comparison should use. Deliberately
  not wired into `check.sh`: a GPU benchmark is not a CI gate. — @chteau

## 2026-09-15

- **Viewport selection and a Move gizmo.** Clicking in the 3D view now
  selects, and a transform toolbar sits under the menu bar where Studio's
  does. A plain click picks the nearest drawn part under the cursor and
  selects the outermost model it belongs to (so clicking one wall of a house
  selects the house); `Alt`/`⌥`-click performs creator-docs' own *selection
  cycling* instead, stepping one raw part at a time to whatever stands
  behind the current one — the mechanism real Studio uses to reach a child
  of a model without leaving the viewport. With the Move tool active
  (shortcut `2`), a part is dragged either by a coloured axis arrow or by
  its own body, and `Ctrl`/`Cmd`+`L` re-orients the draggers between world
  and the part's own frame, with an `L` indicator while local is on. The
  whole gesture is one undo step: a history snapshot clones the entire DOM,
  so pushing one per mouse move would empty a fifty-deep stack in under a
  second.
  The geometry behind it lives in `rbx_viewer` (`pick`, `gizmo`) so both
  halves read the same definition — the renderer builds the arrows the user
  sees from exactly the functions the editor hit-tests the cursor against,
  which is what stops "what you can grab" and "what you can see" from
  drifting apart across the thread boundary between them. Scale, Rotate and
  the whole snapping story (increment fields, `Shift`-to-invert, soft-snap
  onto nearby surfaces, `T`/`R`'s 90° tilts) are still open; Scale and
  Rotate are shown as disabled toolbar buttons rather than live ones that do
  nothing, the same convention the menu bar already uses. Studio's fifth
  "Transform" button stays out until what it actually does can be confirmed
  against a real Studio rather than guessed. — @chteau
- **The viewport no longer goes blank after a Command Bar script.** Whether
  the 3D view is still on screen was inferred from whether GPUI had repainted
  it since the last tick, which is only sound while something keeps causing
  repaints — and the only thing that ordinarily does is a finished frame
  landing. A scene rebuild takes seconds, during which no frames land, so the
  panel looked exactly like a dock tab switched away: it was declared hidden,
  the render thread was told to stop drawing, and that stopped the very frames
  whose absence was the sole evidence for it. The state sustained itself, and
  the view stayed blank until some unrelated notification happened to repaint
  the window. The tick that would have given up now asks for one repaint
  instead of concluding anything: a mounted panel answers and stays visible, a
  genuinely hidden one cannot and is dropped on the next tick exactly as
  before. — @chteau
- **Renderer state survives a rebuild.** `Headless::reload` builds a whole new
  renderer from the new DOM, and everything the editor had asked for rather
  than the file — the projection mode, the selection outline, the transform
  gizmo — went with the old one, leaving the viewport visibly wrong with
  nothing to say why. They are gathered into one value the renderer's
  constructor now *requires*, so a rebuild cannot start blank: there is no way
  to build one without saying what view it is for. — @chteau
- **Snapping for the Move tool.** The transform toolbar now carries the snap
  increments Studio's does: a move/scale field in studs and a rotate field in
  degrees, each with its own enable/disable checkbox beside it rather than one
  shared flag — two pairs and not three, because creator-docs gives Move and
  Scale a single studs increment between them (`Shift`+`2` jumps to "the
  move/scale increment input", `Alt`+`R` to "the rotate increment input"), and
  the rotate pair is drawn disabled until there is a Rotate tool for it to act
  on. Holding `Shift` mid-drag *inverts* whichever state the checkbox is in,
  for that drag only, so it frees a snapped drag as readily as it snaps a free
  one. What rounds is the travel since the handle was grabbed, not the part's
  world position: the docs never say where a grid is anchored, and rounding
  the travel is what stops a part that already stood off-grid from jumping the
  moment it is picked up. With no grid in force, a drag by the part's own body
  instead "soft snaps" its grab point onto the surfaces, edges and corners of
  parts it passes near — the docs give the two as alternatives, not as things
  that stack ("if snapping is **disabled**, the part will soft snap to
  surfaces and edges of nearby parts"), and publish no threshold, so how near
  "near" is scales with the draggers' own screen-relative size here. `T` and
  `R` during such a drag turn the part 90° about the point it is held by: `T`
  tilts it towards the camera, `R` turns it about the normal of the surface
  under it. — @chteau

- **Orthographic camera mode.** The viewport's free-flight camera can now
  switch between perspective and parallel projection — toggled from the
  Viewport panel's overflow menu in `rbxstudio`, or `rbxview --orthographic`
  on the standalone viewer. Two real bugs surfaced testing this against an
  actual place file before it shipped, both fixed along the way rather than
  left for later:
  - The sky/star/sun background is drawn as near-unit-magnitude geometry
    that relies on the perspective divide to spread across the screen, which
    orthographic's constant `w` collapsed to a single point at screen
    centre, leaving pure black. Fixed by keeping the sky/star/sun background
    always perspective, decoupled from the main camera's own projection
    mode.
  - The view volume's zoom was first derived from the free camera's distance
    to the scene, recomputed every frame — better than a one-time snapshot,
    but still broke down on a level with several spread-out clusters of
    geometry (a real report: floating islands scattered across a big map),
    where "distance to the scene" has no relation to how close the camera
    actually is to whatever it's looking at. Replaced with an explicit
    `Pose::ortho_scale` the mouse wheel controls directly while orthographic
    is on (dollying the eye instead, perspective's own role for that wheel
    gesture, does nothing visible under a parallel projection) — zoom is now
    independent of camera position entirely, so it neither depends on the
    rest of the level's layout nor risks flying the eye through geometry
    with no size cue.
  Also threads the projection choice through shadow-map fitting
  (`Camera::frustum_corners`) and the depth-of-field pass's depth
  reconstruction (`post.wgsl`'s `view_distance`), both of which hardcoded the
  perspective-specific formula before this, and scales the orthographic
  depth range with the current zoom level rather than a fixed constant — a
  fixed one large enough to never clip destroyed float32 depth precision for
  close-up work, caught by a test rather than a screenshot. That range runs
  as far *behind* the eye plane as in front, the way other orthographic
  -capable tools clip around the view's focus rather than at the camera:
  the eye's position has no optical meaning under a parallel projection, and
  clipping just in front of it sliced clean through anything the free camera
  had flown into or alongside (a third real report against the same place:
  a hillside cut flat where the camera stood inside its bounds). — @chteau
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
- **A cursor-dragged part now rests on whatever the cursor passes over.**
  Dragging a selected part by its body slid it along one flat plane, fixed
  through the grab point for the whole gesture, so dragging it over a
  platform kept it at its original height. Each move now casts the cursor
  ray against the rest of the scene and rests the part on the face it
  meets — on top of a platform, against the side of a wall — falling back
  to the flat plane only when the cursor is over nothing, which is the
  surface half of the soft-snapping `creator-docs` describes for cursor
  dragging (the edge half is still open). The part being dragged is left
  out of its own raycast, or it would climb onto itself a stud per frame;
  and every answer is a function of the cursor alone, with the part's last
  position never fed back in, so a cursor held still on the edge between
  two surfaces gives one answer rather than flickering between two. The
  geometry lives in a new `settle` module in `rbx_studio`, tested without a
  window: settling onto a raised part, falling back over open space, the
  self-exclusion, and a crossing between surfaces jumping by exactly the
  height difference. — @chteau
- **Clicking in the viewport now picks against the shape that is actually
  drawn, not the box around it.** `pick::parts_along` tested every part as
  its oriented bounding box, so the empty corner beside a ball, the air
  above a wedge's slope and the whole hollow of a rock-shaped `MeshPart`
  all selected the part — and a thin slab standing just inside a ball's
  box but outside its sphere lost the nearest-first ordering to the ball.
  Each part now resolves through the same `scene::shape::resolve` the
  renderer draws it with (so a `SpecialMesh` sphere is the same ellipsoid
  on both sides): a `Ball` is a sphere, `Part.Cylinder` a capped cylinder
  along X and `CylinderMesh` one along Y, `WedgePart`/`CornerWedgePart`
  their slope half-spaces derived from `shapes::wedge`/`corner_wedge`'s own
  vertices, and every convex solid shares one entry/exit-span test in the
  part's local space. A `MeshPart`/`SpecialMesh` FileMesh is tested against
  the triangles of its downloaded mesh through the same `Fit` the renderer
  places it with: `Headless::pick_meshes` hands out a handle onto the
  render thread's parsed meshes (behind `Arc`s, no copy), the pump reports
  it once per scene build, and `Shell` picks with it. Whatever never
  downloaded still picks as the fallback box it is drawn as; a `TrussPart`
  deliberately stays its full box, and a `UnionOperation` too. The
  per-shape tests include a sweep of a few thousand rays per solid, under
  the identity and an arbitrary rotation+scale, checked against the actual
  `shapes` meshes with a triangle test — which is what pins the slope
  planes and cylinder axes to the geometry rather than to a reading of it.
  — @chteau
- **Scale and Rotate gizmos.** The two toolbar buttons PR #8 left disabled are
  live, on their documented shortcuts `3` and `4`. **Scale** puts a block on
  the end of each axis arm; dragging one resizes the part along that axis with
  the opposite face held still, so the grabbed face tracks the cursor and the
  centre travels by half the growth — written as a `Size` and a `CFrame` edit
  together, clamped to the 0.001–2048 range `BasePart.Size` documents.
  **Rotate** draws a ring per axis that turns the part about its centre;
  because a ring angle only exists up to a full turn, the drag sums the step
  between successive samples rather than measuring back to the grab, which is
  what lets it be carried past 180° without snapping round the other way. Both
  obey `Ctrl`/`Cmd`+`L` the way Move already did, and both measure against the
  frame the gesture *started* in — in local orientation the handles turn with
  the part as it goes, and measuring against those live would cancel out the
  very rotation being applied. `rbx_viewer`'s `Handles` gained the ring
  geometry beside the arm geometry it already had, so all three tools are hit
  tested and drawn from one definition, and `Gizmo` now carries which tool it
  is rather than implying Move. Writing a rotation needed a `CFrame` the
  Properties panel's parser could express: it now reads nine numbers as a
  rotation and twelve as a whole placement, in the order Roblox's own
  `CFrame.new(x, y, z, R00 … R22)` takes them, alongside the three it already
  read as a position — so a viewport drag and a typed value still go through
  exactly one write path. Snapping is still open for all three tools.
  — @chteau
- **Viewport multi-select.** `Shift`/`Ctrl`/`Cmd`-click in the 3D view now
  adds another top-level object to the selection instead of replacing it —
  `Alt`/`⌥`-click's own selection cycling is untouched. The selection went
  from at most one instance to an ordered set: the Explorer highlights
  every selected row, the outline draws around all of them, and the Move
  gizmo still appears exactly once, on the first ("anchor") selected part —
  the same part `rbx_viewer::renderer::selection::Selection::anchor`
  already picked for a lone selection, now just reused rather than
  reinvented for a group. Dragging any handle, or any selected part's own
  body, moves every selected part together by the gizmo's own measured
  offset, so the group's layout relative to itself never changes; the
  whole drag is still the one undo step it always was. Group/ungroup
  operations remain their own separate, unimplemented roadmap item.
  `RBX_STUDIO_SELECT` now takes a comma-separated name list (a single name
  behaves exactly as before) and a new `RBX_STUDIO_DRAG=<dx>,<dy>,<dz>`
  moves the current selection by that offset — debugging aids for
  screenshotting the outline/gizmo over a group and a group drag, since
  neither a modified click nor a mouse drag can be sent to the editor on
  its own behalf. — @chteau

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
