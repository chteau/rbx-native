# Changelog

## 2026-09-23

- **The Argon dock is rebuilt to the redesign.** The connection sits on
  the left: the Argon mark, the CLI's version, and a status badge for
  Disconnected, Connecting, Connected, Error and a pending review; under
  it one line of context, then the `host : port` field, a "?" that opens
  a "Getting started" popover, and the Connect / Disconnect / Dismiss /
  Accept and Cancel buttons. The plugin's settings fill the right as
  cards under Connection, Sync, Two-Way Sync and Workflow headers, with a
  Global / Game / Place switch (Game and Place only once a published
  project is connected) and Restore defaults. The dock lays out for its
  own width: two columns above 900 px, stacked below it, one column of
  cards under 588 px, a wrapped action row under 388 px, so it reads the
  same docked at the bottom, at the side, or floated. While disconnected
  the dock says whether the Argon CLI is installed (its path in a
  tooltip) or links to Argon's install page, and the settings column
  keeps its scroll thumb in view. The Argon and Wally dock menus are now
  called "Argon dock options" and "Wally dock options". — @chteau

- **Auto Connect stays quiet without Argon.** When the Argon CLI isn't
  installed, opening a place no longer tries to connect and fails; the
  dock stays Disconnected and Output notes that Auto Connect was skipped.
  Connect still works by hand, for a server in WSL or on another
  machine. — @chteau

- **Connecting to Argon no longer duplicates the place.** Before, every
  instance the server sent was added on top of what the place already
  had. Now, on connect, the place and the server's project are matched up
  by name and class: instances the server knows are updated in place,
  and instances it doesn't know are removed, unless Keep Unknowns is on
  or the project marks them as kept. Initial Sync Priority decides who
  wins: Server (the default) changes the place to match the files, Client
  changes the files to match the place, None connects without changing
  either. Override Packages off keeps server changes out of anything
  under a `PackageLink`. When the connect would change more instances
  than Changes Threshold allows, the dock asks first, as it does for a
  live batch. — @chteau
- **The rest of Argon's settings do what they do in the plugin.** Auto
  Reconnect retries five seconds after a dropped connection; HTTPS
  connects over TLS; Two-Way Sync (off by default, as in the plugin)
  decides whether edits go back to the files at all, with Only Code Mode
  and Syncback Properties choosing what goes; Display Prompts and Changes
  Threshold decide when a batch asks first, and the threshold is now
  "more than", as in the plugin, instead of "five or more"; Open In
  Editor sends a synced script to your OS editor instead of opening it
  here; Log Level filters what Argon writes to the Output dock; Diff
  Lines Limit caps how much of a script the diff window shows. — @chteau
- **Argon's own settings, per level.** The Argon dock's settings model is
  now the plugin's: the same 15 settings with the same defaults, resolved
  Place → Game → Global → default, stored in `settings.json` under `argon`
  with only the overrides. The Game and Place levels are identified by the
  connected project's own game and place IDs (an unpublished project has
  neither). The first setting wired up is Auto Connect, on by default as in
  the plugin: opening a place now tries the remembered Argon address at
  once, so a machine without `argon serve` running sees the dock's error
  state instead of Disconnected until it's dismissed. — @chteau

- **The snap popover follows the redesign board.** Opening the ribbon's
  snap pills now shows a 248px surface with one section per unit: a
  title row with the unit and a switch, then a single-field stepper with
  its − and + at either end and the value centred in mono. A switched-off
  section goes flat and out of the Tab order, and both pills take the
  accent hairline while the popover is open. Same increments, same
  shortcuts, same IDs. The pills themselves grow to the board's 26px with a
  13px icon and a 6px gap, and a switched-on toggle's knob is pure white
  everywhere in the editor, as the palette specifies. — @chteau

- **The editor ships its own fonts.** Manrope and JetBrains Mono (both OFL,
  licences in `assets/fonts/`) are embedded and registered at startup, so
  the chrome no longer falls back to the platform font on a machine that
  doesn't have them installed. — @chteau

- **The editor's chrome is restyled: sober, near-black, one accent.** Every
  surface, border, text colour, radius and type size in `tokens.rs` (and the
  toolkit theme mirroring it, `assets/themes/dark-soft.json`) now follows
  the redesign reference: three surface tones, three text tones, faint
  white hairlines, and `#6C7FDB` reserved for active and selected state.
  Booleans in Properties are pill toggles now, not checkboxes. Section
  headers are small uppercase labels, rows sit 7px apart without separator
  lines, and a toggle is pinned to the right edge of its row. The ribbon's transform tools sit in
  one bordered card, the snap values are bordered pills, document tabs
  float with an accent underline, and a dock with a single tab shows a
  plain title instead of a tab pill. The Output dock spans the full window
  width under both side docks, its four level filters are one segmented
  control, and its search box fills the strip. The Command Bar gets a `>`
  prompt and a `Luau` badge. The 3D view also stops being letterboxed to the
  UI Editor's canvas resolution unless the UI Editor is actually open.
  Properties also gains the reference's breadcrumb (class icon, name,
  parent) under its filter, controls parked at the row's right edge with
  the name taking the rest of the row, and every field, search box and the
  Command Bar show an accent border while focused. Icons, logo, layout and
  behaviour are otherwise unchanged. — @chteau

## 2026-09-22

- **The Argon review prompt has a real Diff window, and the connection
  address is remembered.** A "Diff" button on the batch review prompt opens
  a second window (`shell::argon_diff_window`, the same fixed-size floating
  shape as the `ColorSequence`/`NumberSequence` graph, own title bar and
  all) listing every addition, update and removal in the pending batch —
  an addition's own name/class and nested-descendant count, an update's
  changed properties as "before → after", a removal's name/class. It holds
  no copy of the batch: every frame it re-reads whatever review is
  currently pending, so it closes itself the moment Accept or Cancel (still
  the dock's own buttons) resolves it. Also: the address Connect last
  actually succeeded with is now saved to `Settings` and reused on the next
  launch, instead of always starting back at `localhost:8000`. — @chteau

- **The Argon dock talks to the real thing.** Connect now opens an actual
  HTTP+MsgPack connection to a locally-running `argon serve`
  (`crate::argon_client`, the same protocol `argon-rbx/argon`'s own Studio
  plugin speaks), long-polling for changes in the background and applying
  them to the DOM as one undo step — additions, property/name/class
  updates and removals, with a review prompt ("N additions, N updates, N
  removals") before a large batch lands, the same threshold Argon's own
  plugin uses. Local edits sync back out too: an instance added, changed or
  removed in `rbxstudio` while connected is pushed to the server on a short
  debounce. Verified end to end against a real `argon serve` session.
  `ExecuteCode` messages (server-sent Luau) are decoded and always
  discarded — running code a network message asked for is a boundary this
  client doesn't cross, unlike the reference plugin. Two known gaps: a
  reparent doesn't sync back out (Argon's own wire protocol has no field
  for it), and an update's omitted properties are left alone rather than
  reset to their class default. — @chteau

- **The Wally dock searches and installs for real.** Typing in the dock's
  search field now hits the real `api.wally.run` registry (debounced) and
  lists results; picking one resolves its whole dependency graph — a BFS
  matching Wally's own resolver (`crate::wally_client::resolve`), erroring
  cleanly on an unsatisfiable requirement rather than installing the wrong
  version — and installs every resolved package into the DOM as one undo
  step, in Wally's own real on-disk shape: each package's content under
  `Packages/_Index/<scope>_<name>@<version>/<name>`, a sibling alias
  `ModuleScript` per dependency edge, and a top-level alias only for the
  package actually picked. No separate "install via Argon" path exists —
  the same DOM insertion reaches a connected Argon session automatically
  through the write-back sync above, and is native when one isn't
  connected. Two known gaps: no `wally.lock` (two installs can pick
  different compatible versions of a shared dependency), and version
  discovery is search-based, so a non-default registry isn't supported.
  — @chteau

- **The Script Editor has its own docks: Argon and Wally.** Two new bottom
  docks, tabbed beside Output, show up only while the Script Editor is the
  open document — and the Viewport dock steps aside while they're there,
  since the 3D view isn't on screen either way. The Argon dock is shaped
  after Argon's own Studio plugin: it actually checks whether the `argon`
  CLI is on PATH and shows the real version if so, and its address field is
  a real, editable one — but Connect stays inert, the same way an unbuilt
  ribbon command stays visible and greyed rather than disappearing, since
  Argon's live sync protocol isn't public yet (see ROADMAP.md). Wally's
  dock is the same shape (an Add Package action, inert for the same reason
  — its resolver is its own effort). — @chteau

- **The 3D view shows a GUI at the screen it was built for.** A `ScreenGui`
  laid out on a 1920×1080 canvas used to be laid out again at whatever size
  the viewport panel was, so it never looked the way it was built. The
  Viewport dock has a Screen setting now, the same one as the UI Editor's
  resolution: pick a device, type a size or turn it, and the scene is
  letterboxed to that screen's shape with the GUI laid out at its size, as
  Studio's device emulator shows it. The viewport follows the canvas's
  resolution from the start, 1920×1080 until another is chosen; "Viewport
  size" goes back to the panel's own. — @chteau

- **The UI Editor's canvas works like Figma now, and keeps up with a drag.**
  Its sidebar leads with a design panel — position with align-to-parent and
  an anchor grid, W/H with an aspect lock, auto layout, opacity, corner
  radius (per corner too), fill with hex and a gradient, stroke, and
  constraints — where every number's label scrubs and a value Roblox keeps
  on a modifier makes the `UICorner`, `UIStroke`, `UIPadding` or layout it
  needs as you edit it. The insert bar draws: pick a tool (or press F, T,
  B, X, L, G) and drag the element out where it goes; a switch beside it
  picks whether the canvas writes Offset or Scale. The canvas shows the
  selection's size, rounds corners from handles, shows an auto layout's
  gaps and padding as bands you drag and reorders a list by dragging a
  child, edits text in place on a double-click, and takes Sketch's keys —
  paint order, fit and zoom to selection, Space to pan, Shift and Alt on
  every drag. A drag step on a text-heavy screen went from 27 ms to 4 ms:
  laying the screen out again after each edit no longer re-shapes every
  label (see BENCHMARKS.md). Open Explorer rows now stay open across an
  edit. — @chteau

- **The Style Editor is the UI Editor, with a canvas.** Editing a
  `ScreenGui` meant picking its descendants out of the Explorer and typing
  `UDim2`s into Properties, with the 3D view the only picture. The document
  now opens on a Figma-style canvas beside the unchanged style sheets
  (its Stylesheet sub-tab): select a `ScreenGui` or anything in it and that
  screen alone is drawn — by the viewport's own GUI renderer, with no scene
  behind it — at a device preset, a typed size or turned on its side. Click
  or marquee to select, drag, resize and rotate one element or the whole
  selection at once, nudge with the arrows, snap to siblings and the parent
  along smart guides, hold Alt for distances, align, distribute, group into
  a fitted `Frame`, insert elements from a floating bar, and make a screen
  responsive in one click (offsets folded into scale, fixed shapes kept by
  aspect ratio) — every edit one Ctrl+Z, written the way a typed Properties
  value is, and measured the way the renderer lays it out: inside the
  parent's `UIPadding`, through `UIScale`, aspect and size constraints. A
  part's `SurfaceGui` or a `BillboardGui` goes on the canvas too, at its
  own canvas size. While it is up the Explorer lists only
  the UI, and the Properties, Output and Viewport docks step aside for a
  property sidebar built from the same rows; leaving it puts them back as
  they were. A new `GuiObject` inserted anywhere is now a visible box rather
  than a 0×0 one. — @chteau

- **Properties shows a part's `Origin`, and typing one moves it there.**
  Studio lists where a part or model's pivot stands in the world under
  Transform; it was missing here, so filtering for "origin" or "position"
  found nothing. The row reads `GetPivot` — a part's `CFrame` times its
  `PivotOffset`, a model's primary part or `WorldPivot` — and an edit moves
  the instance the way `PivotTo` does, carrying a model's parts and its
  pivot together as one undo step, with a rotation left untouched kept
  byte-for-byte. The filter now also finds a `CFrame` row by its Position
  and Orientation fields. On a US keyboard, `Shift`+`2` reaches the move
  increment field again: GPUI reports it as the `@` it types. — @chteau

- **The docks move.** A panel's home used to be the order of three
  `.child()` calls in the render code, so there was nothing a gesture could
  change. It is data now, and the tab is the handle: drag a dock's tab to
  another edge, onto another dock's strip to become a tab of it, or onto a
  dock's half to split the edge and make a new dock there — the half that
  lights up is the size the new dock will be, because it *is* that half. An
  edge holding nothing grows a ghost dock while a drag is in flight, easing
  open and shut, so an edge you emptied can be filled again. Drag a tab past
  the window's own edge and the dock tears out into a window of its own,
  rendering the same editor rather than a copy of it; close that window and
  the panel goes back where it started. A dock closes from the × on its tab
  and comes back from the ribbon's Home tab or the View menu, whose three
  entries had been placeholders since the menu was written. Everything —
  which panel is on which edge, in which dock, showing which tab, how big
  each edge is, what is floating and what is shut — survives a restart.
  "Move to Left/Right/Bottom", "Float" and "Close" are on every dock's own
  menu too, and that is not decoration: the accessibility guidance this
  project follows treats drag-only rearrangement as a failure rather than a
  gap, so the drag is the fast path and the menu is the one that has to
  exist. Both go through one transform, so they cannot disagree. — @chteau

- **A dragged part squares itself onto what it lands on.** Dropping onto a
  wedge's slope, a ball or a cylinder's side turns the selection onto that
  face the way Studio's `snapRotationToPrimaryDirection` does, keeping the
  grab-time orientation otherwise (`Alt` holds it as is), and every one of
  those surfaces now gets its own frame for the grid snap and guides — a
  ball's latitude and longitude, a cylinder's cap and side, a mesh's face
  from the triangles under the cursor. After a handle drag the distance
  label turns into Studio's measurement box: type a length and the
  selection moves by exactly that, as one undo step. The handle trails a
  line back to where the drag began, every guide is Studio's one pixel
  wide — the line pass now draws after the tone map, so a hairline keeps
  its colour — and a key press over the viewport no longer wipes the hover
  ruler and cue until the mouse moves. — @chteau

- **A `SurfaceLight` lights its whole face.** It shone as a cone from the
  face's centre, so its patch was half the width of the frustum its own
  guide draws; the shader now measures from the nearest point on the face,
  and the lit area is the guide's. — @chteau

- **`BrickColor`, and the numbers Studio computes, in Properties.** The
  full 208-colour table from Roblox's docs backs a `BrickColor` row that
  reads the nearest palette colour off `Color` and writes a pick back to
  it, and `rbx_lua`'s `BrickColor` uses the same table now, so the two
  cannot disagree. `Mass`, `CenterOfMass`, `CurrentPhysicalProperties` and
  the assembly's mass and centre show read-only, computed only where
  Roblox documents exactly how. A file that stores `Color` or `Size` under
  its canonical name renders them — names are normalised once, at load —
  and a script reading a property the file never stored gets Roblox's
  default instead of nothing. — @chteau

- **The Viewport dock works from the keyboard and at any UI scale.** Its
  settings are one Tab stop walked with the arrow keys, the way Properties
  is, every select shows a focus ring, and the dock keeps its proportions
  at 0.5× and 2×. — @chteau

- **A binary save no longer zeroes what an instance left unset.** When one
  Part of a place stored a property and another did not, the writer filled
  the gap with the type's zero — a Part inserted from the Explorer saved
  with `CanCollide` off and no size, and one copied Part's `archivable`
  was enough to make Studio drop every other Part on its next save. The
  gap is filled with the class's own default now, infinite ones included,
  and a load never renames a property to a name Studio cannot read back
  (a package link keeps `PackageIdSerialize`). Move and Scale show only
  the handle being dragged, as Studio does, and draw over the guides
  rather than under them. — @chteau

## 2026-09-21

- **`Highlight` draws.** The class a script reaches for to call attention to
  something had nothing behind it here at all — parenting one to a model did
  nothing. It now draws what Roblox documents: a silhouette outline around
  the adornee and a solid interior over it, each with its own colour and
  transparency, and both `DepthMode`s, so a highlight either shows through
  whatever stands in front of it or stops where the object stops being the
  nearest surface. The silhouette is the object's own, not the box around
  it — a ball outlines as a circle, a wedge as a triangle, a downloaded mesh
  as its own polygon — because the covered geometry is re-drawn, position
  only, into a mask the frame is then painted from, through the same buffers
  the shadow pass already instances. A place with no highlight in it compiles
  none of the six pipelines and allocates no mask. — @chteau

- **Tags stopped showing up twice in Properties.** The panel listed every
  property the instance carried and left out only the ones the reflection
  dump tags `Hidden` — but `Tags` and `AttributesSerialize` are not in the
  dump at all, so both got an ordinary row showing their packed bytes,
  sitting a few rows above the Attributes/Tags section that is their real
  editor. Two editors for one value, one of them useless. Neither gets a
  row now, which is also what Studio does. — @chteau

- **The insert picker's classes carry their own icons, and the kit grew
  five drawings to cover them.** A list of several hundred class names with
  nothing but text beside them is a list you read rather than scan, so each
  row now draws the same identity icon the Explorer tree gives an instance
  of that class — through the same `explorer::resolve_icon`, so the two can
  never disagree. A row whose parent cannot take the class fades its icon
  along with its label, since a kit tile carries its own colours and
  greying the words alone left the row looking half-disabled. Rasterized
  tiles are memoized now: the picker rebuilds its list on every keystroke,
  and re-rendering an SVG per row per frame is not something that survives
  contact with a real place. Five classes a creator reaches for had no tile
  in Roblox's own metadata and so had no tile here — `StyleSheet`,
  `StyleRule`, `StyleLink`/`StyleDerive`, `IntersectOperation` and
  `BodyColors` — and now have one each, drawn to the kit's own spec in both
  the dark and light variants; twenty-two more are pointed at the family
  tile they belong to (a `FileMesh` is a mesh, a `KeyframeSequence` is an
  animation). What still falls back to a Lucide glyph is the long tail:
  `ReflectionMetadata*`, the `Studio*` plugin objects, the `DataStore*`
  option bags and the debugger's own instances. — @chteau

- **The Explorer edits the place from its own rows.** Inserting used to
  mean two keyboard shortcuts or a trip to the ribbon, and renaming meant
  finding the `Name` field in Properties. Hovering a row now reveals a `+`
  (`Ctrl+I` from the keyboard) that opens a searchable list of every class
  the API dump marks browsable, inserting straight under that row; a class
  that parent cannot take is greyed rather than missing, so you learn the
  constraint from the list instead of from its absence. Right-clicking a
  row opens Cut, Copy, Duplicate, Paste Into, Rename, Insert Object…,
  Group as Model, Ungroup and Delete, each greyed by exactly the guard its
  own handler returns early on. `F2` renames in the row. Cut is real now
  rather than a greyed placeholder, in the menu, the Edit menu, the ribbon
  and `Ctrl+X` — and, in the same spirit, deleting a *service* is refused
  everywhere instead of quietly producing a place with no `Workspace`.
  Real Studio's two insertion preferences ride along behind the picker's
  own `⋯`, persisted: numbered names for new instances, and whether
  selecting expands the tree to reveal what was selected. — @chteau

- **Bounded properties get a slider.** A `Transparency`, a `ClockTime`, a
  `GuiObject`'s `Rotation` — anything whose value has a real floor and
  ceiling — now shows a rail beside its number field, and the two edit the
  same value: drag the rail or type the number. The reflection dump carries
  no bounds, so which properties have one and how finely each steps is a
  named table (`properties::ranges`), and a range there only decides how far
  the rail reaches — the field still takes anything the parser does, so a
  frame really can be rotated 400°. A drag commits every step, so the
  viewport follows the rail rather than waiting for it to be let go, and the
  whole gesture still undoes in one. This is the first slider in the editor;
  it is built from `gpui_base`'s unstyled parts and skinned as a field box,
  because the toolkit's finished one sizes itself in `rem` and would ignore
  the UI scale. — @chteau

- **Numeric properties open like Studio's, and the panel has one left
  edge.** A `Vector3`, a `UDim2`, a `CFrame` and every other multi-number
  value used to take the row's whole width and lay its components out side
  by side — three fields sharing a 150px column, each too narrow for a
  digit, under a name on its own line. Each one is now an ordinary
  name/value row showing the value whole (`0, 5, 0`, the spelling a script
  would use, and still typeable as one), with an expander that drops its
  components underneath: `X`/`Y`/`Z` for a `Vector3`, `Position` and
  `Orientation` each over their own three for a `CFrame`. Collapsed is the
  default, and a collapsed row does not build its component fields at all —
  a `BasePart` selection is five such rows, so that is most of what the
  panel used to lay out and paint every frame. Rows are separated by a
  hairline instead of a gap, and property names, attribute names and tag
  chips all start in the same column the expander chevrons leave for them.
  — @chteau

- **An image a Properties edit first names now actually arrives.** Pointing
  an `ImageLabel`/`ImageButton` at an asset the place had never shown
  re-planned the GUI with the new reference but never asked the loader for
  it, and a reference nobody asks for never lands — so the label drew the
  placeholder until the next reload. A patched GUI now asks for its images
  the way it already asks for its fonts. — @chteau
- **A `Decal`/`Texture` edit is no longer quietly undone by the next asset
  that lands.** Editing one of the faces themselves — a new `Texture`, a
  different `Face` or tint, one added or deleted — patched the renderer but
  left the *decor plan* as the file was read, and the plan is what every
  later landing re-assembles the decals from. The very image the edit asked
  for was enough to put the old one back. The plan is now re-read from the
  DOM for the part a face edit touches, as it already was for a part that
  moves. — @chteau

- **`NumberSequence` and `ColorSequence` have a real editor, and are
  creatable as attributes.** Both types used to render as read-only text,
  which is also why neither could be added as an attribute: the Attributes
  section routes a value through the exact per-type widget an ordinary
  property gets, so a type with no editor had no row anyone could change.
  A sequence row now *draws* its value — a gradient ramp, or the curve —
  and clicking it opens a graph in a second window of the editor's own:
  fixed-size, floating above the main window, moved by the editor's own
  title bar rather than the platform's. Keypoints drag, a click on empty
  plot inserts one on the curve it split, dragging one out through the top
  raises the value axis (there is no "Max Size" box to fill in first), a
  `NumberSequence`'s envelope band has its own handle, and a
  `ColorSequence`'s stops are markers under the ramp with the panel's
  existing colour picker behind the swatch. Roblox's own constructor rules
  are enforced (2–20 keypoints, non-descending time, first at 0 and last
  at 1) because the renderer's `eval_number`/`eval_color` walk the list
  assuming exactly that. The window keeps no copy of the value: it
  rebuilds from the DOM every frame and commits through the same textual
  path a typed row takes, so the viewport repaints on every drag step, an
  undo or a script shows up in the graph immediately, and a whole drag is
  still one undo entry. — @chteau

- **The rest of the renderer's API-dump gaps.** A sweep of
  `assets/API-Dump.json`'s renderer-relevant classes against what
  `rbx_viewer` actually draws turned up four that drew nothing at all, and
  they draw now. `Fire`, `Smoke` and `Sparkles` — documented as
  preconfigured particle emitters — are read into the same emitter
  definition a `ParticleEmitter` produces, a `Fire` as the two emitters its
  docs describe; every emitter also reads `TimeScale` now and can hang off
  an `Attachment` rather than only a `BasePart`. `AdGui` shows its own
  `FallbackImage` on the face it adorns, which is what Roblox documents an
  ad surface as showing when no ad is available. An `Explosion` and a
  `DynamicMesh` are recognized as having nothing a place file can draw —
  one is a one-shot that plays when it is parented, the other carries no
  properties at all — rather than left unaccounted for. — @chteau
- **The `Handles`/`*HandleAdornment`/`Selection*` family draws.**
  `SelectionBox`, `SelectionSphere`, `SurfaceSelection`, `Handles`,
  `ArcHandles` and the six handle shapes are resolved into world-space
  primitives on the CPU (`scene::adornment`) and drawn as unlit geometry
  inside the scene pass (`renderer::adornment`) — depth-tested by default,
  over everything while `AlwaysOnTop`, with `ZIndex` ordering those among
  themselves. What the docs leave unpublished (a handle's proportions, the
  axis a cone points down) is marked in the code as this renderer's own
  choice rather than presented as parity. Input is not part of this: the
  docs are explicit that a handle listens only under a `PlayerGui` or the
  `CoreGui`, which is the plugin API's business. — @chteau
- **A selected part is outlined by its own shape.** The Explorer's
  selection and the hover cue traced the oriented bounding box of whatever
  they covered, which is the wrong shape for a `Ball`, a wedge or a
  `MeshPart`. A part now draws through the same mask-and-composite pass a
  `Highlight` does, so it outlines as its own silhouette; a container has
  no shape of its own to trace and keeps the box around everything beneath
  it. — @chteau
- **`PointLight` casts a shadow.** It was the one local light that never
  did: a point light has no axis to point a single shadow map down. It
  takes six 90-degree faces into a depth array of its own now, and the
  shader picks the face a fragment belongs to from the major axis of the
  light-to-fragment direction — the same projection-and-compare the cone
  lights already use, rather than a second set of cube-map conventions to
  get wrong. Sized so it pays its way: the faces are half a cone light's
  map across, a quality level allows a quarter as many point lights as
  cone ones, and the six passes are redrawn only when the cubes would hold
  something different rather than every frame. — @chteau
- **`Glass` refracts, and a `ForceField` is a shell rather than a tint.**
  The scene pass ends when the opaque half is done and everything that
  blends over it goes in a second pass, which is what lets a pane read the
  frame behind itself and displace it along its own mapped normal. The
  copy that makes it possible is only allocated, and only taken, in a
  place that actually holds glass. A `ForceField` part keeps its tinted
  shell and gains a lattice of cells and a rim that brightens edge-on;
  Roblox publishes no pattern for the texture-less look, so the code says
  plainly that the rendition is this renderer's own — and that it does not
  animate, because a shimmer with a clock in it would make every
  `--screenshot` differ from the last. — @chteau
- **The Align tool previews where things will land.** Opening its popover
  now draws a ghost box at each object's aligned placement — the docs'
  "dynamically previewing the point of alignment before confirming" —
  redrawn as the toggles and the selection change and cleared when it
  closes. The overlay takes plain world matrices and knows nothing about
  Align, so the next tool that needs one adds no pass. — @chteau
- **Change Class.** Right-click an Explorer row and a `Part` can become a
  `WedgePart`, a `Frame` a `TextButton`, a `Script` a `LocalScript` — the
  whole selection at once, in one undo step. The instance changes class in
  place rather than being rebuilt, so it keeps its referent, and everything
  that pointed at it — a `Weld.Part0`, a `PrimaryPart`, the selection, an
  open script tab — still does, which a plugin that destroys and recreates
  the instance cannot offer. A property the new class has no room for is
  dropped, one still at the old class's default takes the new class's own,
  and tags, attributes and anything else the API dump does not describe
  always survive; the picker says what a class would drop before you pick
  it. It is the insert picker with another purpose: related classes and
  this session's recent picks come first, and the search ranks a prefix
  over word starts over scattered letters, so `tl` finds `TextLabel`. A
  service's class stays put, and the Output panel says so. — @chteau
- **The sun goes where you point.** A Sun tool on the Model page places the
  sun — or, from its Moon tile, the moon — by pointing at the scene instead
  of typing a time and a latitude. Four gestures: drag it across the Sky;
  press a surface and it shines straight onto that Face; press an object
  and drag to where its Shadow should fall; or press a surface and it moves
  to where its Glint reaches the camera. Every step writes `TimeOfDay` and
  `GeographicLatitude` and is patched rather than rebuilt, so the sky and
  the shadows follow the cursor, the Properties panel follows too, and the
  whole drag is one undo. The inverse of the sun model lives beside the
  formula the renderer lights with, so the two cannot drift; a cone of the
  sky no latitude inside ±90° reaches is held at its rim, and the readout
  says so rather than writing a latitude past the pole. A line through the
  anchor toward the light — and in Shadow a marker where the shadow lands —
  is drawn through the same overlay the Align preview uses. — @chteau
- **Change Class and the Sun tool read the real shapes and defaults.** A
  value still at the old class's stock takes the new class's own stock,
  read from each class's recorded defaults rather than one table shared by
  every part: a stock `Part` becomes a 2 × 2 × 2 `TrussPart`, a stock
  `PointLight` a `SpotLight` reaching 16 studs. A default recorded in
  another type than the file keeps its key in is left alone rather than
  written wrong. The Sun tool's Face and Glint aim off the surface a part is
  drawn with — a wedge's slope, a ball's or a cylinder's curve, the triangle
  of a downloaded mesh — rather than the box around it, through a
  `pick::surface_hit` that resolves shapes the way a click does. — @chteau

- **Studio's light guides.** Select a `SpotLight`, `PointLight` or
  `SurfaceLight` and the viewport draws how far it reaches, the way Studio's
  "Show Light Guides" does: three great circles of `Range` around a point
  light, a spot's cone out to its spherical cap with the axis line running
  past the rim, and a surface light's frustum from its whole face. The lines
  take the light's own `Color`, follow a Range/Angle/Face edit live without
  a reload, and — as Studio's own announcement says — appear only for a
  selected, enabled light, never for the part it hangs on. They go through
  a new editor line pass that the dragger guides share, and the toggle lives
  in the Viewport dock. — @chteau

- **A dragged part lands where Studio lands it, and shows why.** A free
  mouse drag now snaps to the Move increment the way Studio's draggers do —
  along the face under the cursor, from that face's nearest corner, not from
  the world origin — and Shift suspends the snap while it is held rather
  than inverting it. The guides are read off Studio's own DraggerFramework:
  a white ruler to the two nearest edges while hovering, a yellow one with
  minor and major ticks while dragging, a yellow line across the face when
  the part lines up with one of its edges or its centre, and, on a Move
  handle, the axis line with a dot wherever the selection's leading face,
  trailing face or pivot would meet a nearby part — which the drag takes
  over the grid step when it is the nearer of the two. Each guide is a
  Viewport-dock toggle named after the Studio setting it mirrors. The
  selection outline also stops staying behind at a moved part's old
  position: its mask was rewritten on every move and never uploaded.
  — @chteau

- **Properties lists what Studio lists, and a multi-selection shares it.**
  The panel used to show only what the file happened to store, so a
  hand-written Part had six rows. It now builds the class's whole sheet from
  the reflection database, fills what the file left out from each class's
  defaults (recorded beside the API dump from rbx-dom's MIT-licensed
  database, since Roblox's own dump carries none for inherited properties),
  and shows `Color`, `Size` and `Shape` rather than the `Color3uint8`,
  `size` and `shape` a file saves — while an edit still lands under the name
  the renderer and the writer read. Select several instances and it shows
  what they share: a value they agree on reads normally, one they don't
  reads blank (per component, for a vector) or as a dash on a checkbox, and
  an edit applies to all of them as one undo step. — @chteau

- **The viewport's settings got out of its way.** The quality dropdown, its
  "…" menu and the quality label floated over the scene; they live in a
  Viewport dock now, a tab beside Output by default, with the live frame
  rate beside them — counted only while that dock is on screen, so closing
  it costs nothing. A panel coming back to an edge joins the dock already
  there as a tab instead of splitting it, and the View menu brings a hidden
  tab forward rather than closing it. Properties and Output lost their dead
  space: names share one left edge with their section headers, the name
  column is 20px narrower, the bottom dock fills its edge instead of
  centring in it, and Output's search and filters sit at the right end of
  its strip. — @chteau

- **Fire, Smoke and freshly inserted lights draw what they should.** A
  `Fire` or `Smoke` saves its `Size`, `Heat`, `Opacity` and `RiseVelocity`
  as `size_xml`, `heat_xml` and so on, and the viewer only ever read the
  plain names — so every real file's values, and every edit to them, were
  ignored. It reads the saved names now. A light inserted from the Explorer
  stores nothing, and a `SpotLight` or `SurfaceLight` without a `Face` was
  simply skipped; every missing light property now falls back to the class
  default Roblox itself uses, which also means an unstored `Shadows` is off,
  as the Properties panel already said. — @chteau

## 2026-09-20

- **A `CFrame` attribute no longer takes every attribute after it down with
  it.** The attribute blob's decoder gave up on type `0x14`, and because
  entries are packed end to end one unknown id leaves no way to find the
  next name, so a place with a `CFrame` attribute showed *none* of that
  instance's attributes past it — and re-saved the blob unread. It is
  decoded and encoded now, checked byte for byte against the two examples
  `rojo-rbx/rbx-dom`'s attribute format prints, and `CFrame` is a type the
  Attributes section can create and edit through the same twelve-number
  field a property gets. The axis-aligned rotation table it needed already
  existed twice in `rbx_binary`, once per direction with a comment saying
  they had to agree; it lives once in `rbx_dom::rotation` now and both use
  it. — @jleeclient

- **Paste Into, and live Copy/Paste/Duplicate on the ribbon.** `Ctrl+Shift+V`
  (and Edit ⟩ Paste Into) pastes the clipboard into the *selection* rather
  than `Workspace`, and into each selected instance when there are several —
  the docs' "convenient way to paste the same clipboard items into multiple
  parents" — one undo step for the lot. Plain `Ctrl+V` used to swallow the
  shifted chord; it does not now, and the whole modifier set is matched, so
  `Ctrl+Alt+V` (AltGr on some layouts) is no paste and `Ctrl+Shift+D` is no
  duplicate. The ribbon's Copy, Paste and Duplicate
  tiles were disabled placeholders; they run the same code the keys do. The
  roadmap listed skipping a non-`Archivable` descendant as still open, but
  that was already done and tested; only the bullet was stale. — @jleeclient

- **Starter scripts can be your own.** A `script_templates` folder in the
  config directory (`Script/`, `LocalScript/`, `ModuleScript/`, one `.luau`
  file per template) adds each file to the ribbon's Script menu under its
  file name, and a `Default.luau` in a class's folder replaces the built-in
  starter every new script of that class gets. Read once at startup; a file
  that is unreadable, not UTF-8 or past 256 KiB is skipped rather than
  stopping the editor. — @jleeclient

- **Icon packs and themes can be installed.** An icon pack is a folder of
  SVGs under `icon_packs/` in the config directory, named by class
  (`Part.svg`) or by the kit's tile (`humanoid-description.svg`); it is
  layered over the built-in kit, so a pack of three icons is a pack, and the
  Explorer's overflow menu lists whatever is installed. A theme is a
  toolkit `ThemeSet` JSON file under `themes/`, named in `appearance.json`.
  Drawings are scaled from their own size rather than assumed 16x16, and one
  that will not parse falls through to the kit. A theme reaches the
  toolkit's widgets only: the chrome this editor draws itself still reads
  compiled-in tokens. — @jleeclient

- **A Command Bar result is not shown twice.** Every run put its outcome in
  the label above the input *and* as a permanent row in the Output dock. The
  label now shows only while the Output dock is collapsed, when it is the one
  place the result would otherwise be lost. That rests on every outcome being
  a row in the dock, which a save was not: `Ctrl+S` reported through the label
  alone, so a failed save would have gone silent under default settings. A
  save is logged as a `Save` row now, and a test pins it. — @jleeclient

- **Escape abandons an Explorer drag.** Letting go after a mistaken pickup
  reparented whatever it was over; Escape stops the drag first, so nothing
  drops and no undo step is pushed. — @jleeclient

- **The menu bar answers F10, and a bare Alt tap.** It was the last thing
  in the editor a keyboard could not reach at all — a WCAG 2.1.1 (Keyboard,
  Level A) failure, and the one the previous pass named as still open.
  Either key moves focus into it from wherever focus was; Left/Right walk
  File/Edit/Model/View and wrap, Enter, Space or Down opens one, and Escape
  closes the menu and then leaves the bar, putting focus back exactly where
  it came from. It is not a Tab stop, on purpose: Tab walks the editor's
  regions, and nobody wants a fifth one in the way of the ribbon. Telling
  an Alt *tap* apart from Alt-the-modifier — which this editor uses live,
  to lock a Ball's Scale handle round and to cycle the selection — is a
  small state machine: anything at all arriving while Alt is held cancels
  the tap. The toolkit's ready-made menu bar keeps its current title
  private with no way in, so the bar itself is ours now; the dropdowns are
  still the toolkit's, keyboard contract and all. — @chteau

- **The Output dock has a Warnings filter.** An app-level warning — an
  asset that will not resolve, a texture that fell back to a default — was
  showing up under both *All* and *Output* and had no bucket of its own, so
  there was no way to ask what was only a warning. There is one now, beside
  All/Output/Errors, which is what Studio's own window does; each bucket
  holds exactly one kind of row, so *Output* is a run's output again. The
  emitter, beam, trail and `ImageLabel` textures were the one case that
  could still fail silently: a render pass only ever sees "no image", never
  why, and the blocking loader was dropping the reason instead of handing
  it back. It hands it back now. — @chteau

- **The Properties filter box searching attributes and tags is now
  covered.** That gap was closed a day earlier without tests or a roadmap
  line to show for it; both sections narrow through a helper of their own
  now, with the cases that matter asserted. — @chteau

- **`PhysicalProperties` is editable.** A part's density, friction,
  elasticity and the two weights were read-only text — the last of the three
  types the roadmap listed that way. They edit as five labelled fields now,
  under a **Custom** checkbox: unticked, the physics come from the material
  the way `Default` means them to; ticked, the five numbers are yours. The
  box switches between the enum's two forms rather than editing numbers, so
  unticking hands the physics back to the material instead of zeroing them,
  and the fields under an unticked box read Roblox's own `Plastic` defaults
  so ticking never commits a row of zeroes. — @chteau

- **Dropdowns, colour swatches and the snap increments can be reached with
  Tab.** Every enum property, every `Color3` and both snap-increment fields
  were mouse-only — a WCAG 2.1.1 (Keyboard, Level A) failure, and the one
  the guidelines named as the most serious thing still open. They are Tab
  stops now, in the same reading order as everything around them. The fix
  turned out not to need the focusable wrapper it had been scoped as: each
  widget's state already exposes the focus handle its own keyboard path
  uses, so the window's Tab order just records it. The menu bar is still
  out, and wants F10/Alt rather than a Tab stop. — @chteau

- **The Output log is searchable.** A box in the Output tab's title bar,
  next to the level filter, hides every row that does not mention what you
  typed — matched against both the command and the result beside it, and
  case-insensitively, because nobody searching a log for an error types it
  the way the error did. It narrows *within* the level filter rather than
  replacing it: Errors plus a query still means errors. — @chteau

- **An absent `OptionalCFrame` can be given a value, and a present one
  cleared.** `Model.WorldPivotData` is the only property the DOM can hold
  that is legitimately *nothing* rather than wrong, and the Properties
  panel had no way to say so in either direction: absent, the row was
  read-only text reading `none`; present, there was nowhere to clear it.
  Both are one checkbox now, above the same `CFrame` editor the value
  already used, which draws only while there is a value to edit. Clearing
  and restoring keeps the frame rather than quietly moving it to the
  origin — the box only says whether there is one. — @chteau

- **A Windows install of Roblox is now a texture fallback.** When
  `setup.rbxcdn.com` can't serve an `rbxasset://` file — offline, or a file
  its packages lack — the asset resolver reads it from the Roblox or Studio
  install already on the machine, under
  `%LOCALAPPDATA%\Roblox\Versions\`. The CDN is still asked first and
  nothing is ever written to that folder. A real install keeps the default
  skybox panels under `PlatformContent\pc\textures\` rather than `content\`,
  so both are searched; without that, a place with no `Sky` rendered a
  black background offline. What it reads is not kept in the asset cache,
  so a stale copy from an older version folder can't become the permanent
  answer once the CDN is back. The path in an `rbxasset://` reference comes
  from a place file, so one that could step outside the install folder
  (`..`, a drive letter, a backslash) or name a Windows device (`NUL`,
  `CON`) is refused rather than read. — @jleeclient

- **`cargo clippy` is clean on Windows again.** Three pieces of X11-only
  code in `rbx_studio` were being compiled on every target: the RandR
  refresh-rate arithmetic and the check that decides whether the pointer
  lock may talk to X, both used only by code that exists on Linux, and the
  `u32::try_from` on an Xlib window id. The first two now carry the same
  `target_os = "linux"` gate as their callers, tests included. The id is a
  C `unsigned long`, so the conversion is a real range check where that is
  64 bits and a same-type one clippy rejects where it is 32; it goes
  through a helper generic over `TryInto<u32>` instead, which is one code
  path on every target. Nothing changes on Linux. The Windows CI job runs
  clippy now as well: the Linux run could not see any of this, because
  what clippy reports here depends on the target, so a lint that only
  fails on Windows had nothing to catch it until someone ran the gate by
  hand there. — @jleeclient

- **`publish-screenshot.ps1` parses on Windows PowerShell 5.1.** It had one
  em dash in an executable string. 5.1 reads a script with no byte-order
  mark as Windows-1252, where the last byte of that dash is a curly double
  quote, so the string ended early and the parser reported four errors
  instead of publishing anything — the script's own header called it
  untested on Windows, and it was. It is ASCII there now, with a comment
  saying why. The other three `.ps1` files have non-ASCII only in
  comments, which are harmless, and parse clean. Checked with PowerShell's
  own parser over all four rather than by eye. — @jleeclient

- **The ribbon greys Copy, Paste and Duplicate when they would do nothing.**
  All three were drawn live whatever the editor held, so with an empty
  clipboard or an empty selection they hovered, took keyboard focus and
  then returned straight out of their handler's own guard — a control that
  looks clickable and silently isn't is worse than one that says why. Each
  now greys with the reason on hover and drops out of the ribbon's arrow
  order, the same treatment the not-implemented tiles already get. The test
  is the interesting part: availability asks `clipboard::has_copyable`,
  which *is* the guard `copy_selected` and `duplicate_selected` return on,
  so the button and the command cannot drift — and a selection holding
  nothing but services reads as empty, which a plain "is anything
  selected?" would have got wrong. Group/Ungroup have the same flaw and a
  same treatment: `group::has_groupable` is `common_parent` returning
  `Some`, and `has_ungroupable` is "one `Model` with something in it among
  the selection", which is exactly what `ungroup_selected` acts on and
  ignores the rest of. — @chteau

- **The Windows row in `README.md` was two claims out of date.** It said
  the project "has never been built or run on a real Windows machine" long
  after CI started running clippy, a build and the full test suite on
  `windows-latest` for every change, and it pointed at a roadmap section
  ("Compatibilité Windows native") that does not exist. It now separates
  what CI covers — it compiles and the tests pass — from what nothing
  covers: the editor has still never been *launched* there, because CI is
  headless. `publish-screenshot.ps1`'s header claimed it was untested on
  Windows after being run end to end on Windows 11; that line is what let
  its parse bug sit, so it now says what was actually run. — @chteau

- **The roadmap says Windows is built and tested, because it is.** "A
  first real build on Windows, and CI coverage for it" had been sitting
  unchecked since the `windows-latest` job landed, and the Tooling/CI
  bullet still claimed Windows had no job at all. The first is checked off
  and says what the job does and does not cover; the second is now about
  macOS, which is the only platform the claim was still true of. Neither
  is marked in-progress — a bullet that is half true is two bullets. —
  @chteau

- **No more in-progress bullets in the roadmap.** Nine were marked
  `[x] 🚧` — each one both a claim that something shipped and a note that
  it hadn't, so the planned list was never the whole of what was left and
  every checked box had to be read to the end to learn which half it was.
  Each is now two: an `[x]` for what actually landed and a `[ ] 📋` for
  what is still open, named so it reads as a real item rather than a
  footnote on a finished one. Nothing changed about what is done; the
  shape of the claim did. `agents/AGENTS.md`, its workflow and the
  `roadmap-task` skill all told agents to *create* that marker, so they
  now describe the split instead — otherwise the next one would have put
  it straight back. Along the way the Explorer-drag bullet stopped saying
  Escape-to-cancel "has not been exercised in the running window", which
  #68's own screenshots had already disproved. — @chteau

## 2026-09-19

- **A collapsed property category reads as a tile.** The category headers
  in the Properties panel carry their own fill now, one step above the
  dock and rounded like everything else that has a surface, and the gap
  between two of them shrank from the panel's largest to its smallest.
  That is not a loosening of the proximity ladder: the ladder's big gap
  now sits under a category's *last row*, where there is a group to close,
  and a run of collapsed headers is spaced like the stack of tiles it
  looks like. — @chteau

- **Numeric property fields are draggable.** Pull a field's *label*
  sideways to scrub its value — the gesture Studio, Blender, Unity and
  Figma all bind the same way, and on the label rather than the field so a
  plain click still means "put the caret here and type". Shift coarsens by
  ten, Alt refines by ten.
  How far a pixel moves the value is a property of the **field**, not of the
  row, which is the part worth knowing: a `UDim` is a decimal scale beside
  an *integer* offset, so dragging the offset steps through whole units
  while the scale moves in hundredths. Every composite type now carries a
  per-field kind rather than just a label.
  That audit found a real bug: `Vector3int16` was cast with `as i16`, which
  truncates toward zero — so a dragged `3.7` would have landed on `3` — and
  wraps silently outside the range, turning `40000` into `-25536`. It now
  rounds and saturates, with a test for both.
  The drag writes through the field's own input rather than straight to the
  DOM, so it takes exactly the path typing does, including that rounding.
  A `Font`'s family name is a field like any other to look at and cannot be
  dragged at all.
- **A `CFrame` is editable as a Position and an Orientation**, the way
  Roblox's own panel splits it, instead of showing its position and hiding
  its rotation matrix behind nothing at all. The matrix is converted to and
  from three degrees in Roblox's `Y`-`X`-`Z` order.
  The lossy direction needed a rule, because nine numbers do not fit in
  three: the panel's fields always submit all six, so an edit compares the
  angles against what the row was *showing* and leaves the stored matrix
  **byte-identical** unless they actually changed. Without that, nudging a
  part's X position would quietly rewrite its rotation through degrees and
  back — a trip that at gimbal lock can land on a different orientation
  entirely.
  Six other types stopped being read-only text along the way: `Ray` and
  `Vector3int16` as numeric fields, `Faces` and `Axes` as named checkboxes
  (a bit set is six yes/no answers, not one value with sixty-four
  spellings), `NumberRange` and `UDim` as labelled pairs instead of one
  comma-separated string. A composite value now drops to its own line under
  the property's name, because six numbers never fitted in a 140px column.
  `agents/property-editors.md` lists the ten types still without an editor
  and says why each was left.
- **Darkened the palette and gave dropdowns a surface of their own.** Every
  surface came down a step, and the whole ramp is now **neutral** grey
  rather than the cool grey it started as: a blue cast on a tool whose
  entire job is showing somebody else's colours puts a thumb on the scale
  for every material and texture judged against it —
  and a select now sits one level above the text fields around it, because
  the two do different things and were previously told apart only by
  noticing the chevron. The step between surfaces is asserted as a
  *luminance difference* rather than a contrast ratio: this near black a
  ratio is dominated by WCAG's own `+0.05` term and scores two surfaces 20
  grey levels apart about the same as two 4 apart.
  A select also sat 2.5-4.5px high in its field box, depending on its
  padding — the toolkit top-aligns a select's row inside whatever height it
  is given. Measured against a plain text field in the same panel and
  compensated, so the two now differ by the same half pixel.
- Property category headers are bold and the brightest step in the ramp, so
  a heading no longer reads as one more property. Buttons are this project's
  own now rather than the toolkit's: a filled surface and the one radius,
  because an outlined pill beside a row of borderless filled fields looked
  imported from somewhere else. The Command Bar sits on the docks' surface
  instead of the window's black ground, which is what makes the field inside
  it read as the same input every property row uses.
- **Rebuilt the Tab order on this window's own registry, after GPUI's
  turned out to be unusable here.** Two limitations, both found by driving
  the real window rather than by reading the code: focus cannot escape a
  group of stops that share a `tab_index`, and every toolkit control that
  never asked for one sits at 0 — so focus that wandered into that bucket
  stayed there, 49 presses in both directions. `tab_group` is no better: it
  silently drops its children's stops entirely. And the obvious workaround —
  call `focus_next` in a loop and check where it landed — cannot work
  either, because GPUI defers a focus change to the end of the frame, so
  the check reads the previous value every time.
  Tab and Shift+Tab are now a key binding in this window's own context
  (the toolkit binds them as an *action*, which resolves before any key
  listener, so a capture-phase handler never saw them at all), and they walk
  an ordered list this shell builds each render. The cycle is closed,
  reversible and has no traps. The Explorer's tree finally has a keyboard
  door: it had a complete, carefully-reasoned APG contract that no Tab press
  could reach, and focusing it now also places the cursor on its first node.
  The Properties panel became one stop with Up/Down inside it, instead of
  the 32 presses it used to take to get from there back to the ribbon.
- **Gave the editor some colour and made the docks visible.** Every dock had
  been painted the frame's black on a black ground, so three docks and the
  window behind them read as one undifferentiated field. There is now a
  surface per layer — ground, dock, panel, button — each a measured step
  above the one below, with a cool cast, and the step is asserted in tests.
  The open document's tab gained an accent rule: the frame conveys "this one
  is open" with a wash measuring 1.036:1, which is five grey levels out of
  255 and invisible.
- **Replaced the viewport's orientation indicator** with the design's own:
  a translucent sphere with the three world axes' orbital rings passing
  through it and a pale cube at the centre. Each ring is split at the
  horizon and its far half dimmed, which is what makes it read as passing
  *through* the sphere rather than lying on it. Still a flat projection, not
  a 3D pass.
- Checkboxes came down from the frame's 26px to 15 across two rounds of
  review, while their *click target* stayed at 26 — WCAG 2.5.8 is explicit
  that the icon may be smaller than the target. Property labels went back to
  left-aligned; right alignment binds a label to its field more tightly but
  leaves a ragged edge that makes a long list harder to scan. The command
  bar, the viewport's quality dropdown and the Output buttons now use the
  editor's own field and control sizes instead of the toolkit's steps. The
  black slots between docks are gone, and so are the discs behind the
  Explorer's chevrons.
- **Fixed what the accessibility review found.** Composite property values
  (a `CFrame`'s numbers) rendered as empty 16px cells and now wrap. A
  read-only property's value was drawn at 1.7:1 and is now a readable step.
  Target sizes are asserted at every UI scale rather than only at 1.0x,
  where none of them could fail. The ribbon scrolls instead of losing
  buttons at 2x, and a dock can no longer take more than 28% of the window.
  The focus ring is inset on controls that sit flush against a container
  edge, where an outset one was clipped to two sides of four — about half
  the area WCAG 2.4.13 asks for. Double-clicking the title bar works from
  the first try: the window move now starts on a drag, not on the first
  press of the double click. — @chteau

- **Made the editor keyboard-operable, and grounded the design in WCAG
  2.1/2.2 rather than in taste.** Tab now moves between regions and arrows
  move within one — the ribbon, each tab strip and the Explorer are one Tab
  stop each, per WAI-ARIA's Toolbar, Tabs and Tree View patterns. The
  Explorer implements the tree contract in full: Right expands then
  descends, Left collapses then climbs to the parent, Home/End, type-ahead,
  and no wrapping at the ends. Escape closes any open menu.
  Three real keyboard traps were found by driving the window rather than
  reading the code, and all three are fixed: Tab was swallowed by the
  Command Bar's input, so focus could never leave it; Tab was swallowed
  again by the title-bar buttons, because GPUI does not advance between two
  stops sharing a `tab_index`; and a roving group's indices were silently
  ignored, because `InteractiveElement::tab_index` does nothing once
  `track_focus` is in play. Every tab stop now takes a unique index from one
  counter, handed out in paint order.
  Focus rings only appear for keyboard focus (GPUI's `focus_visible`), which
  removes the hard blue rectangle a mouse click used to leave on every
  button it touched, and they are now a 2px outset ring clearing 3:1 against
  every surface — WCAG 2.4.13's measurable floor. Selected and focused are
  no longer drawn the same way: selection is a wash, focus is an outline.
- **Added a UI scale** (Ctrl+= / Ctrl+− / Ctrl+0, 0.5×–2.0×, persisted),
  modelled on Blender's Resolution Scale: one multiplier over every font
  size *and* the boxes they sit in, since text that grows while its row
  doesn't is not a larger UI but a clipped one. This is how a native app
  meets WCAG 1.4.4's 200% resize, and every size token is now a function
  rather than a constant so nothing can escape it.
  Body text went from the design frame's 9px to 14px. That is the one place
  the frame is deliberately overruled — its hierarchy is kept, only the base
  size moved.
- **Sized the controls to the `InputsStyle` Figma frame**, which supersedes
  guesswork: 31px fields, 3px radius, no border, 8px padding, and a 26px
  checkbox. The checkbox was 10px, which is under a fifth of WCAG 2.5.8's
  24×24 target area; panel icon buttons were 20px. Both now clear the floor,
  and the floor is asserted in tests rather than eyeballed. The frame draws
  an unticked checkbox with no border at all — 1.11:1 against a black dock —
  so that one gets an outline this project chose.
- **Gave each transform tool its own pastel** while it is the active one,
  with a 1.5px border in the same colour so the state survives grayscale and
  every kind of colour blindness (WCAG 1.4.1: colour is never the only cue).
  Ribbon icons lift on hover, and the OS "reduce motion" preference is read
  at startup and honoured — GPUI owns the flag but never learns the
  platform's setting on its own.
- **Loosened the Properties panel**, whose rows were touching. The four gaps
  are now a ratio rather than four numbers — label-to-input < row-to-row <
  header-to-first-row < between-categories — so the panel reads as groups
  (Gestalt proximity) instead of one block. Labels are right-aligned, which
  is the tightest label-to-field binding for a dense numeric inspector.
- **Double-clicking the title bar maximizes or restores**, through the
  platform's own call rather than by recomputing bounds. Both gestures hang
  off the same press, which is the debounce: the second press of a double
  click never starts a compositor window-move. Noted: that call toggles on
  X11, Wayland and macOS but only maximizes on Windows.
- Drag-to-rearrange docks is **not** implemented: it needs a layout tree in
  place of three hardcoded slots, and the architecture note is in
  `agents/dock-rearrangement.md` rather than a half-built version in the
  shell. — @chteau

- **Fixed the Part insert menu always inserting a block.** Block, Sphere
  and Cylinder all inserted a bare `Part` and never wrote its `shape`
  property, so the renderer resolved all three to `ShapeKind::Box`; Wedge
  and Corner Wedge passed their own classes but got none of a new part's
  size, colour or material defaults, since those were gated on the
  literal class `"Part"`. `shape` (`Enum.PartType`) now travels alongside
  the class for the three that share it, and the defaults gate widened to
  any `BasePart` subclass. One test per menu item asserts both the
  inserted instance's class and its resolved `ShapeKind`. — @chteau
- **`rbxview`'s title bar now shows fps/frame time**, alongside the flight
  speed it already showed, closing the one leftover on the FPS/frame-time
  readout item. Shares the same one-line format `rbxstudio`'s corner label
  uses (`rbx_viewer::fps_readout`) rather than a second copy of it. —
  @chteau
- **Copy, Paste and Duplicate work now** (`Ctrl+C`/`V`/`D`, and the Edit
  menu's items — Cut stays a placeholder). The clipboard is this window's
  own, not the system one, and a copy is a genuine deep clone: descendants
  come along, an internal reference is remapped to the copy exactly as
  `Instance:Clone()` documents, and mutating the copy can't reach the
  original. Paste always targets `Workspace`, matching creator-docs;
  Duplicate stays in the original's own parent. Services refuse to be
  copied, pasted or duplicated, the same way Group/Ungroup already refuse
  them. A non-`Archivable` descendant (and its own subtree) is excluded
  from what gets copied, and the copy is always `Archivable` regardless of
  the original, matching `Instance.Archivable`'s own documented rule. —
  @chteau
- **The selection box can be hidden behind the parts in front of it.** A
  selected object's outline has always been drawn with the depth test off,
  so it shows through whatever stands between it and the camera — Studio's
  own behaviour, and the reason a `Model` selected behind a wall is visible
  at all. That stays the default; a new item in the Viewport panel's
  overflow menu, next to Orthographic and Stats, asks for the other
  behaviour instead, and the choice survives a relaunch. Two pipelines that
  differ in one depth-compare, picked between at draw time, rather than one
  rebuilt whenever the menu item is clicked — there is no device in hand at
  that point. The choice rides in `rbx_viewer`'s `View` alongside the
  projection mode and the selection itself, so a scene rebuild cannot
  quietly drop it.
  The `Model` aggregate bounding box this branch set out to add turned out
  to already exist — it shipped with the Model/Folder/Tool outline work —
  so what landed for it here is the coverage it was missing: a `Model`
  nested inside a `Model` is one box and one drag over every part at every
  depth, `Alt`-click still reaches a single part inside a model with its
  own oriented box rather than the model's aggregate, and the Align tool's
  **Selection Bounds** agrees with that box on the world axes. Three doc
  comments left describing the older behaviour — one still calling a
  `Model`'s aggregate bounds a TODO — now describe the code as it stands,
  and the box records its one documented divergence from Studio, whose own
  `Model:GetBoundingBox` orients the box by the model's pivot. — @chteau
- **Attributes and Tags editor.** The Properties panel grows a section
  below the reflected categories for custom `Instance` attributes and
  `CollectionService` tags — neither had any editor before. Attributes
  are listed, added (name plus a type picker), renamed, removed, and
  their values edited through the exact same per-type widgets an ordinary
  property gets, never a parallel set; tags are chips you add and remove,
  matching `AddTag`'s own idempotent-add behaviour. Name validation
  follows `Instance:SetAttribute`'s documented rules. Both sections search
  through the panel's own filter box, the same way an ordinary property
  row does. `CFrame` is not offered as a creatable attribute type (no
  verified wire format for it in `rbx_dom::attributes`), and
  `NumberSequence`/`ColorSequence` aren't either, matching this project's
  separate policy of giving each of those eight `Variant` types its own
  PR. — @chteau
- **The graphics-quality dropdown is reachable from the keyboard.** It was
  one of the controls behind this project's most serious open
  accessibility gap: `SelectState`'s own `focus_handle` — the exact one
  `Select::focus` already uses — is now recorded directly in the window's
  own Tab order (`shell::roving::TabOrder`), no wrapper element and no
  forwarding subscription needed, since focus lands on the real thing
  already. `Color3`/enum property rows, the snap increments, `ColorPicker`
  and the menu bar remain mouse-only; each needs its own follow-up, laid
  out in the roadmap bullet this partly closes. — @chteau
- **Holding `Alt` while dragging a Ball's Scale handle keeps it round.**
  Dragging any Scale handle used to grow only the one axis grabbed, same as
  any other part — fine for a block, but it turns a sphere oval. `Alt` now
  locks the drag to grow all three axes together (`Size + (d,d,d)`),
  modeled on Building Tools by F3X's own `Resize.lua` rather than native
  Studio, which gives Scale no shape-specific behavior at all. `Alt` was
  picked over F3X's own `Shift` because `Shift` already means "invert the
  current snap state" here on every tool; `Alt` is provably free at the
  moment a handle is grabbed. `Cylinder`'s and Wedge/CornerWedge's own
  cases are still open. — @chteau
- **`Alt`-dragging a Cylinder's round handle keeps its end circular.** The
  same lock the Ball got now covers `Cylinder`: grab either of the two axes
  forming its round end and both grow together; grab its length axis
  instead and nothing extra happens, since nothing else is meant to grow
  alongside a cylinder's length. Which axis is which wasn't guessed at —
  `rbx_viewer::scene::shape::part_type` already fixes `Enum.PartType.
  Cylinder` to draw with its length on local X, round in Y/Z, matching
  Roblox's real engine geometry, and the lock reuses that existing fact
  rather than determining it a second time. Wedge/CornerWedge's own case
  is still open. — @chteau

- **`rbxstudio` has a real command line.** Driving the editor from a script
  or a CI job used to mean exporting `RBX_STUDIO_SELECT`/`RBX_STUDIO_RUN`
  and hoping: variables no `--help` mentions, that a wrapper has to set on
  a child process, and that say nothing back when a name in them matches
  nothing. Those two capabilities are now arguments — `--select`, `--run` —
  alongside `--verbose` and a `--help` that prints the lot.
  Three things are genuinely new rather than renamed. `--select` takes an
  Explorer path (`Workspace.Model.Part`) as well as a bare name, so a
  scripted launch can name the part it means instead of the first one that
  happens to share a name; the path is tried first and falls back to the
  name search, which is what keeps an instance whose own name contains a
  dot reachable. `--run` takes a `.luau` *file* rather than inline source,
  read before the window and the GPU exist so a path typo is an exit code
  instead of a line in the Output dock on the first frame. And a `--select`
  target that matches nothing is now reported on stderr, verbose or not,
  since a script driving the editor cannot see a silent no-op for itself.
  Both variables keep working unchanged — they are still how the rest of
  the screenshot aids are spelled — and a flag simply wins over its
  variable when both are set. — @chteau

## 2026-09-18

- **Rebuilt the editor's chrome against the project's own Figma design.**
  Every surface, radius, dimension and type size in `tokens.rs` is now
  measured off the `RbxNative - Studio App` frame rather than interpreted
  from a reference screenshot: a near-black palette with soft white-alpha
  state washes instead of borders, one 3px radius, and exactly one
  saturated colour in the whole UI — the checkbox blue, which keyboard
  focus and row selection borrow and nothing else may. The toolkit's theme
  file mirrors that palette, and a test now fails the moment the two
  disagree.
  The editor draws its own title bar (client-side window decorations: logo,
  centred title, minimize/maximize/close, drag-to-move and double-click to
  maximize), over a 24px menu strip, fixed-width document tabs, seven
  ribbon category pages, and a three-column workspace whose docks are a tab
  strip over an inset body. Ribbon commands are 42px tiles and 78px stacks;
  the snap increments became a live readout that opens its own editor,
  since an increment is checked far more often than it is changed.
  Chrome icons are Lucide — the set the design itself is drawn with, so
  this project's hand-made outline kit went away; the multi-colour
  class-icon kit stays in the Explorer, where the colour is the identity.
  Contrast stays asserted rather than eyeballed: every meaningful text
  token clears WCAG AA on every surface it can land on, and the disabled
  step is asserted from both sides so it can't drift into invisibility.
  The frame's 9px body text ships as drawn and is flagged in
  `UX_GUIDELINES.md` §10 as the one open question rather than silently
  corrected. — @chteau
- **Rebuilt the editor's UI on a real design-token system.** Every colour,
  radius, spacing step, elevation, easing curve and text style now comes
  from one module (`tokens.rs`), with the palette mirrored into the
  toolkit's own theme file so stock widgets follow it too. The discipline
  (fixed radius scale, base-4 spacing, "shadow-as-border" layered
  elevation) is borrowed from Vercel's Geist; the dark, rounder, softer
  look deliberately isn't. Contrast is now a test rather than a judgement
  call — primary text clears 11.5:1 on every surface, secondary 5.0:1, and
  the accent-on-accent active-tab case 4.6:1.
  The shell was rebuilt around it: document tabs directly under the menu
  bar, the ribbon's category tabs under those, the ribbon under those, and
  a three-column workspace (Properties, document over Output, Explorer)
  with real resize handles, a collapsible Output dock, per-panel corner
  radii and directional elevation. Explorer rows grew hierarchy guides.
  Every piece of chrome is drawn from this project's own 24px outline icon
  kit, which is what lets an icon tint itself per state; the multi-colour
  class-icon kit stays in the Explorer, where the colour is the identity.
  Replacing the toolkit's `DockArea` with that fixed shell is what made the
  layout expressible — it also means panels can no longer be dragged to
  rearrange, and the saved dock layout is gone; `UX_GUIDELINES.md` §10
  records that along with every other deviation. — @chteau
- **Throttle the viewport's render loop while the editor window is
  unfocused.** The render thread paced itself to the display's full refresh
  rate no matter whether anyone was looking, burning GPU/CPU (and battery,
  and fan noise) the moment the editor sat behind another window. Losing OS
  focus now caps it to a user-chosen preset, 25 or 30 fps — a toggle next to
  the viewport's quality dropdown, persisted like the rest of Settings — and
  the first focus or input event restores the full rate immediately, ahead
  of whatever window-activation event may still be in flight, so coming back
  never feels sluggish. — @chteau
- **Dock panel chrome lost on a restored layout.** `register_panels`'s
  builder — the path every panel is rebuilt through once a saved dock
  layout exists, i.e. every launch after the first — wrapped the rebuilt
  panel in a bare `Arc::new(panel)` instead of `panel_handle(panel)`. Both
  compile (`Entity<P>` already satisfies the trait object the registry
  asks for), but only the latter is downcastable back to `PanelHandle`,
  which is what the tab bar needs to recover a panel's dropdown menu and
  zoom control. That silently dropped the Viewport panel's "Orthographic"
  toggle and disabled every panel's "Zoom In" the moment a saved layout was
  restored. Fixed to use the same `panel_handle` helper `build()`'s
  first-launch path already used correctly, with a regression test that
  drives a saved-layout round trip and asserts on the `PanelHandle::of`
  recovery directly. — @chteau
- **A frame rate readout in the viewport corner label.** Real Studio's own
  performance surface is a toggle (`Window > Performance > Stats`), not an
  always-on display, so `rbxstudio`'s Viewport panel overflow menu gets a
  "Stats" checkbox next to the existing Orthographic one; switching it on
  adds the render thread's last-measured fps and frame time to the corner
  label already showing quality level and flight speed. No new timing
  mechanism — it reads the same per-second numbers `workspace_view::stats`
  already computed to drive automatic quality scaling, just exposed to the
  UI thread instead of only ever printed to stderr. — @chteau
- **Output dock: a Show Timestamp toggle and per-kind row color/icon**
  (`feat/output-timestamp-kind-styling`). Ships the not-sandbox-dependent
  half of the Output window roadmap bullet: a **Show Timestamp** toggle in
  the Output panel's overflow menu (next to Explorer's and Viewport's own
  toggles) prints each row's `HH:MM:SS.SSS` timestamp, captured at push
  time regardless of whether it's shown (`OutputEntry::timestamp`). Rows
  no longer share one plain `✕`/`✓` marker — `print`/a successful run now
  reads in the default text color with a check icon, `warn` in orange with
  an alert icon, `error` in red with an X icon (`OutputEntry::kind`,
  `RowKind`, `shell/output.rs`). Left open: the duplicate-display gap
  between `command_bar::Feedback`'s own label and the Output dock row,
  `TestService.Message`'s blue/info kind (needs the sandbox), and the rest
  of that roadmap bullet's sandbox-dependent half. — @chteau
- **The Properties panel's `Rect` rows are editable.** `SliceCenter` and
  other `Rect`-typed properties used to fall back to read-only text; they
  now edit as four labeled fields (`Min X`/`Min Y`/`Max X`/`Max Y`), the
  same pattern `Vector2`/`UDim2` already used. — @chteau
- **Colour-coded Explorer folders.** A `Folder` can now carry a colour tag,
  set through a synthetic "Explorer Colour" row the Properties panel shows
  only for a `Folder` (the same `Name`-row trick and colour-picker widget
  the panel already had — no new UI). The tag recolors the folder's own
  Explorer icon, and its row's hover/selected background and selection
  outline read as the tag colour (faded) instead of the theme's default
  blue. The tag lives in its own local, per-place store
  (`folder_colors.json`) rather than the saved place file — there's no real
  Roblox `Folder` property for it, so a real Studio session opening the
  same place never sees an invented one. Keyed by the folder's Explorer
  path rather than a `Ref` (which regenerates on load), so a rename or move
  currently orphans the tag; a load-time prune at least keeps those
  orphaned entries from piling up forever. — @chteau
- **Group/ungroup operations.** `Ctrl+G` (or Model ⟩ Group) wraps the
  current selection in one new `Model`, parented where the selection itself
  was — refused cleanly, rather than guessing, when the selection spans more
  than one parent. `Ctrl+Shift+G` (or Model ⟩ Ungroup) unwraps a selected
  `Model` back into its own parent and removes it; a non-`Model` or an empty
  one is a no-op. Both are one undo step regardless of how many instances
  move. — @chteau
- **New-script templates, and a way to insert a script at all.** The Model
  menu had no way to insert a `Script`/`LocalScript`/`ModuleScript` —
  `Insert Object…` was a disabled placeholder — so `Insert Script`,
  `Insert LocalScript`, `Insert ModuleScript` and
  `Insert ModuleScript (Class)` are now real menu items. Each seeds the new
  instance's `Source` with a starter template instead of an empty string: a
  plain `print("Hello, world!")` for `Script`/`LocalScript`, a `ModuleScript`
  returning a table, and the `(Class)` variant a `.new()` constructor over a
  metatable. The template set is hardcoded for now, not the user-extensible
  set `ROADMAP.md`'s own wording asked for — @chteau
- **Dark/light Explorer icons, a persisted setting.** The class icon kit's
  `light` variant was shipped but nothing read it; both `dark` and `light`
  are now embedded, and a "Light Icons" checkbox in the Explorer panel's
  overflow menu (next to "Show all services") swaps between them at
  runtime, no rebuild needed, dark by default. Persisted in
  `settings.json` alongside quality and service visibility, so a relaunch
  keeps whichever you picked. — @chteau
- **A top-right orientation indicator for the viewport.** Reads as an actual
  cube: up to three visible faces render as flat coloured parallelograms
  tiling around their shared corner (or one square face-on), each labeled
  `Right`/`Left`/`Top`/`Bottom`/`Front`/`Back` and coloured per axis like the
  transform gizmo already uses. Projected from `Pose::basis` as a flat 2D
  isometric-cube trick — the same one Blender's own gizmo uses — rather than
  a literal 3D render, so it tracks the free camera's current orientation
  live with no extra render pass. Toggleable from the Viewport panel's
  overflow menu, on by default and persisted like `Orthographic`. An
  original rbx-native addition (Blender/SketchUp/3ds Max convention), not a
  Studio-parity claim — nothing in Roblox's own viewport documentation
  describes Studio having one. — @chteau
- **A live stud-count readout during a Move or Scale drag.** A small label
  now follows the cursor while a transform-gizmo drag is held, reading the
  straight-line distance moved so far for a Move, or the dragged axis's
  growth (or shrink) in studs for a Scale — an rbx-native addition, not a
  Studio-parity claim. Reuses `gizmo.rs`'s own already-known drag delta
  rather than recomputing it. — @chteau
- **Align tool.** Studio's real Model-tab Align tool (checked against
  `studio/align-tool.md`, not the Move/Scale/Rotate gizmos): moves the
  selected objects' own Min/Center/Max bound on each toggled X/Y/Z axis to
  match a reference value, in World or Local space, relative to either the
  whole selection's collective bounding box or a fixed Active Object (the
  last-selected instance, outlined by staying put while everything else
  moves to meet it). A selected `Model` moves as one rigid body, matching
  the docs' "keeping the model intact." Lives in a small popover on the
  transform toolbar (Move/Scale/Rotate's own strip) rather than a new
  dialog. Not shipped: the docs' own "dynamically previewing the point of
  alignment before confirming" — this Align commits immediately, with no
  live preview. — @chteau
- **Properties panel hides properties Studio itself never shows.**
  `rbx_reflection`'s `PropertyDescriptor` now carries the API dump's
  per-property `Tags` and `Serialization` (`CanLoad`/`CanSave`); the
  Properties panel filters out anything tagged `Hidden` entirely (e.g.
  `BasePart.Position`/`Orientation`, exposed only through the not-yet-built
  Position/Orientation UI — `CFrame`'s own row is still the editable
  stand-in) and renders a non-`Hidden` property Studio can't save back or
  tags `ReadOnly` (e.g. `BasePart.Size`) with no edit widget instead of
  hiding it. — @chteau

## 2026-09-17

- **A batch of viewport editor fixes (`fix/overall-bug-fixes`).** Undo/redo
  and other shortcuts now work with the 3D view focused rather than steering
  the camera (`z` is the forward key, so Ctrl+Z used to fly forward on
  AZERTY), and a Move drag reaches the undo stack again — it opened its
  history entry on the gesture's first *sample*, which a 1-stud grid rounds
  to nothing, so the whole drag landed outside undo. AZERTY's unshifted
  digit row reaches the tool shortcuts too. A Move-tool click on a part in
  front of the selection now selects it instead of dragging the selection
  behind it. The Move/Scale stud increment snaps Scale as well as Move, and
  the Rotate increment is live (Alt+R jumps to it). Scale and Rotate act on
  a whole multi-selection, centred on its bounds, not just the anchor part.
  The hover outline previews what a click would select — the whole model
  plain, one part with Alt held — and both the selection and hover outlines
  are now thick screen-space lines instead of a one-pixel hairline. Dragging
  a Model with many children no longer freezes the viewport: consecutive
  change batches fold to one per frame, an attachment move re-plans only the
  effect kinds the scene actually has, and a Model drag no longer re-walks
  the whole workspace for the free-drag neighbour boxes each frame. The
  sun's specular highlight on plastic is a touch stronger. — @chteau
- **Follow-up viewport fixes on the same branch.** A `MeshPart` or
  `UnionOperation` now shows its selection outline and transform gizmo — both
  read the renderer's placement map, which dropped a mesh part's box the
  moment its mesh became resident (a patch removed it, and a full rebuild
  reseeded from the filtered `placements`); both keep the mesh part's own
  bounding box now. Plain-hovering a model outlines the model as one box
  rather than every child, and the selection box is no longer occluded by
  geometry (it reads as a control, drawn on top). Beams no longer twist into
  a bend on a low-segment curve — the ribbon takes its width from the
  polyline, not the analytic tangent — and are affected by `LightInfluence`,
  dimming with the scene at night. Alt-hover now previews the part Alt-click
  cycling would select next rather than always the nearest hit, so a child
  reached by cycling — the case when the camera sits inside a parent part —
  is previewed before it is picked. A part the camera sits inside no longer
  swallows every click and hover — it is ordered by where the ray leaves it,
  so an Alt-hover or Alt-click reaches a child in front of it directly,
  without selecting the enclosing part first. The sun's specular highlight is
  now scaled by EnvironmentSpecularScale, the dial the environment reflection
  already used: a place that sets it to 0 renders matte with full, vivid
  colours as Studio does, instead of a broad white highlight washing the
  greens toward grey, while a place at 1 keeps the full highlight. — @chteau
- **GUI — full `GuiObject` compatibility (`feat/gui-full-guiobject-compatibility`).**
  The viewer's `ScreenGui`/`BillboardGui`/`SurfaceGui` trees now render the
  rest of what a real place puts in them. `TextLabel`/`TextButton`/`TextBox`
  draw their text, shaped with `cosmic-text` (already in the tree under
  GPUI) in Roblox's own font families, fetched from the Studio content
  package at runtime like textures are and falling back to a system font
  until they land; `TextScaled`, wrapping, both alignments, rich text,
  strokes, truncation and a `TextBox`'s placeholder included. `ImageButton`
  and every image `ScaleType`, sub-rects and pixelated resampling.
  `UICorner`, `UIStroke` and `UIGradient` through an SDF rounded box and a
  gradient ramp in the GUI shader, so corners clip and gradients tint the
  background, image and text alike. `UIPadding`, `UIScale`, the aspect,
  size and text-size constraints, `AutomaticSize`, `SizeConstraint`, the
  `BorderMode`s, `ScreenInsets` and `ZIndexBehavior.Global`; nested
  `Rotation` composes the way `AbsoluteRotation` says. The layout family:
  `UIListLayout` flex and wrapping, `UIFlexItem`, `UIGridLayout`,
  `UITableLayout`. A `StyleSheet`/`StyleRule`/`StyleLink` engine — a real
  selector parser and cascade, derives and tokens, the rule properties
  decoded from the `PropertiesSerialize` attribute blob — and a Style
  Editor panel in `rbxstudio` that edits sheets, rules and their properties
  with the viewport following live. Every behaviour was checked against
  `Roblox/creator-docs`; where a page is silent the code says so. — @chteau
- **GUI parity fixes on the same branch.** Text now measures like Studio's:
  `TextSize` is the line box, the glyph em a fixed 1/1.2 of it (measured
  against Studio captures of three families), and a family lacking the
  requested weight shapes in its closest face instead of a system font —
  which is why FindTheCode lost Fredoka One. Translucent frames read as
  dark as Studio's: the overlay composites in encoded space through a
  non-sRGB view of the target. A `Folder` inside a `ScreenGui` no longer
  swallows its subtree, and is the layout scope the docs describe.
  `ScrollingFrame`, `CanvasGroup`, `ViewportFrame` and `UIPageLayout` draw
  as themselves, and every property of `StarterGui`'s 45 GUI classes is
  either implemented or documented as having no still-frame effect. Against a
  Studio capture of a third place: a `ScrollingFrame`'s scale-sized children
  resolve against a canvas no smaller than the window (so rows inside an
  automatic canvas no longer collapse), a wrapping list's gap between lines is
  `Padding`'s share of the cross axis, and every enabled `UIStroke` on an
  object draws, lowest `ZIndex` first. `StarterGui.ShowDevelopmentGui` is
  honoured (with a `rbxview --show-development-gui` override), text weights
  and styles a family lacks are synthesised (a fake-bold dilation and a
  slant) rather than silently drawn Regular, editing `Font` writes the
  matching `FontFace` and back, `FontFace` is editable in the Properties
  panel, an `ImageLabel` whose image never lands draws Studio's own
  placeholder, `SpawnLocation`'s decal loads from the catalogue, and an
  asset fetch refused with 429 or a gateway error is retried with backoff
  (the keyed asset route allows 1000 requests a minute per key owner) and
  re-queued by the editor's loader instead of leaving the label blank. The
  mouse wheel over a `ScrollingFrame` in the viewport scrolls it, as Studio's
  edit view does, without entering the undo stack; the wheel anywhere else
  still moves the camera. — @chteau


## 2026-09-16

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

- **A union drawn as its recovered pieces is patched piece by piece, not
  rebuilt for.** A legacy `UnionOperation`/`NegateOperation` whose boolean
  cannot be computed draws as the additive parts recovered from its
  operation tree, and every one of those pieces used to answer to the
  union's own referent — so the renderer's per-instance maps could hold
  only one of them, `Scene::resync_part` refused the lot, and moving,
  recolouring, hiding or deleting one union rebuilt the whole scene
  (`Rebuild::Union`). Each piece now carries an identity of its own,
  `scene::PartId`: the union's referent plus the piece's position in the
  tree's additive order. That order is a function of the asset's bytes
  alone — never of where the union stands or what colour it is — so no
  edit can renumber a piece, and the record an edit rewrites is always the
  record that leaf already had. The renderer's instance rosters, batch
  index, blended list and shadow casters are keyed by that id instead of
  by a referent; everything else keeps naming instances by referent, so a
  decal, a mesh instance and a light are untouched. `Scene::resync_part`
  re-derives a union's pieces from the boolean `load::Resident` already
  carved — the boolean is a function of the asset's bytes, so a move costs
  the tree walk that re-places half a dozen pieces, never a BSP build —
  and writes each into the slot it already had, dropping only the ones an
  asset change left over. `Rebuild::Union` is gone with it, and the colour
  a union with `UsePartColor` off takes from its own tree, which was the
  variant's second reason, is now read off that same carving; a union
  whose asset was never carved draws as its own box exactly as a rebuild
  leaves it, and so does one pointed at an asset nobody has fetched yet —
  the background loader is asked for it instead (see
  `Headless::apply_changes`'s asset-streaming path), the same as a
  `MeshPart` named for the first time. `Rebuild::Asset` is narrower for it
  too, then: a material sample past what the renderer uploaded, never a
  missing download. The union itself stays one thing to the
  editor: `Scene::placements` keeps its own box — the very shape
  `pick::parts_along` hit-tests it as — and lists none of its pieces, so
  the selection outline, a `Decal` on it, an emitter's spawn volume and a
  `SurfaceGui`'s adornee all go on drawing against the union rather than
  against whichever piece happened to be last (which is what they did
  before, arbitrarily). Measured on `FindTheCode.rbxl` (7 167 instances,
  4 946 parts, 40 unions drawn as 208 recovered pieces; `marked.rbxl` has
  none — its five `394314025` "Rock" unions all carve successfully and
  draw as computed meshes), assets on, 1280x720, RTX 4070, first frame
  readable / call returned: moving one union 2.12 ms / 0.42 ms against
  24.62 / 22.25 before, recolouring it 2.06 / 0.41 against 25.23 / 23.33,
  and the undo of each the same — against that place's own single-instance
  patch at 2.20 / 0.41 and its full reload at 33.98 / 30.51. The harness
  has all four as phases, skipped with a note on a place that draws no
  union that way. For a union moved, recoloured, turned invisible and
  deleted — each forwards, undone and redone — the patched frame is
  pixel-identical (AE 0, uploads drained) to a cold rebuild of the same
  DOM (`tests/patch_parity.rs`, `--ignored`, needs a GPU and a fixture
  with such a union). — @chteau

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

- **Incremental edits: review fixes.** Four things the patch path above
  got wrong, caught in review — one of them by a pixel comparison. A `Frame`
  dragged from a `ScreenGui` onto a part's `BillboardGui` showed up in both:
  a GUI tree is planned from its container down, and only the container the
  element *landed in* was re-planned, so the overlay kept drawing it where
  it used to be — 21 340 pixels off a rebuild of the same DOM at 640×360.
  `Patcher::left` now re-plans the container an element left, the way it
  already re-derived the part a `SpecialMesh` left, and `patch_parity` has
  the case. A `MeshPart` whose mesh never downloaded (a 404, a file the
  content package lacks) drew as its box, correctly, but every later edit of
  it — a colour, a move, anything — was a full reload for the rest of the
  session: `resync_part` classified by class, saw a `MeshId`, found no mesh
  and asked for a rebuild. The scene now remembers what it asked for and
  never got (`Scene::unresolved`, file meshes and union assets alike) and
  edits such a part as the box a full build leaves it; a `MeshId` nobody
  asked for yet is still a reload's to fetch, and a mesh that lands after
  all still takes over. `Shell::reflect_changes` cloned the entire DOM to
  hand the render thread every edit — once per mouse move of a drag — which
  on `marked.rbxl` (16 742 instances) measured 60 ms an edit, sixty times
  what patching one part costs. The render thread now keeps a mirror of the
  editor's DOM, and an edit crosses as a snapshot of the instances its log
  names (`WeakDom::snapshot`/`WeakDom::mirror`; a move or delete also
  carries the parents whose child lists changed, so an undo puts a child
  back among its siblings rather than after them). With that hand-off timed
  as part of the edit, one part's move on `marked.rbxl` went from 61.4 ms to
  0.02 ms (`call`) and 63.7 ms to 1.0 ms first frame readable; a hundred
  parts from 66.5 ms to 2.9 ms and 70.8 ms to 4.0 ms — see `BENCHMARKS.md`.
  And the per-edit path still scanned the place in four spots: the scene
  found a part's slot, and a mesh part's resolved instance, by walking every
  part; the extent was recounted over every part whenever any moved; and
  `fold` scanned its own output per change. Each is indexed by referent now
  (`Scene::standing`, `Resolved::slot_of`), the log folds through a map, and
  the extent grows in place, recounted only when a part that may have been
  holding an edge moved or went — which is what keeps it exactly what a
  rebuild frames. Still whole-list, left for a later pass: a moved part's
  `BillboardGui`/`SurfaceGui` canvases are re-planned as a list, the ~1.8 ms
  the batch move above attributes to it. — @chteau

- **Async assets: review fixes.** Four things the streaming work below got
  wrong, caught in review. A place with two legacy `Union`/`Negate` parts
  whose asset bytes landed on separate ticks drew the first one's geometry
  twice, for good: every tick runs the file mesh pass and then the union
  pass, both of which put the accumulated unions back into the resolved set,
  and the second call appended the whole set on top of what the first had
  just put there. Both halves of that path are idempotent now:
  `Scene::apply_resolved_unions` drops what an earlier call left, by the
  union referent each instance draws in place of, before laying the set back
  down, and `union::Merged::absorb` ignores a union it has already merged —
  which is also what lets the loader stop tracking that itself and hand
  `Scene::resolve_unions` whatever landed, carved or not. An edit that both
  named a brand-new `MeshId` and set `Transparency` to 1 took the
  "instance removed" branch of `Headless::patch_instance`, which asked for
  nothing: the fetch did not start until some later edit made the instance
  visible again. It asks now, like every other branch. The swap-in throttle
  was reset before the landed batch was checked against the place, so a
  result arriving for a reference nothing names any more delayed the next
  real swap-in by up to 100 ms; the timer moves only when something is
  actually folded in. And `renderer::particles` did not filter
  `AssetRef::Empty` out of its texture list the way `beam`/`trail` do, so an
  emitter with no texture was re-evaluated on every renderer rebuild for the
  life of the session. — @chteau

- **Assets stream in instead of stopping the frame.** Resolving one
  `MeshId`, `TextureID`, material pack or `Decal` image is a download, a
  disk read and a decode — two thirds of a `marked.rbxl` reload, by the
  profile behind the reload entry below — and every bit of it used to run on
  the thread that draws, before anything was drawn. Two things followed.
  Opening a place showed nothing at all until its last asset had arrived.
  And an edit naming an asset the session had never decoded — typing a new
  `MeshId` or `TextureID`, picking a material whose pack was not resident —
  made `Headless::patch_instance`/`patch_effect` answer `Ok(false)`, so the
  render thread fell back to a full scene reload *with the download in front
  of it*, between two frames.

  Nothing waits for an asset now. `load::fetcher` resolves and decodes on a
  small pool of worker threads and hands each result back over a channel the
  render loop drains once a tick (`Headless::tick`); `load::Resident` gained
  the in-flight half of its state machine, which is also what coalesces two
  edits naming the same new asset into one fetch and what lets a result for
  a reference nothing names any more be filed and ignored rather than
  uploaded. `Loaded` now keeps the plans it was joined from — the file mesh
  plan, the union plan, the material catalog, the decor plan — and not just
  the result, so the place can be re-joined to whatever has decoded since
  without the DOM, which costs 60 ms to clone on `marked.rbxl` and is long
  gone by the time an asset lands. Until one does, what draws is the
  fallback this viewer already had for an asset that never resolved at all:
  the box a `MeshPart` falls back to, the untextured mesh, plain plastic, a
  solid line for a `Beam`. Each landing is folded in by the same in-place
  `Renderer::rebuild` the reload entry below added, whose invalidation keys mean it
  uploads the one asset that landed and leaves every other upload where it
  is; landings inside 100 ms of each other are folded in together, so a cold
  load's several hundred assets cost ten rebuilds a second rather than one
  per tick. The four renderer passes that fetched their own textures inside
  `Renderer::rebuild` (`particles`, `beam`, `trail`, `gui::atlas`) read an
  answer from the loader instead, so no call site on the render thread
  resolves an asset any more.

  What is left of `Ok(false)` on those two paths is three cases with no
  single-instance answer at all, none of them about an asset being absent: a
  union repainted from its operation tree (its recovered pieces share the
  union's referent), a referent the scene never built (a `MeshPart` staged
  outside `Workspace`), and an edit whose new material needs a texture-array
  layer past the ones uploaded, which only a rebuild resizes.
  `Headless::patch_effect` keeps only the first of those, for a referent
  that is not a `ParticleEmitter`/`Beam`/`Trail`. A fetch that fails is
  still remembered and never retried; its warning now reaches the Output
  dock once per reference for the life of the viewer rather than again on
  every reload.

  `scripts/bench.sh`, 1280x720, same machine as `BENCHMARKS.md`, assets on.
  `marked.rbxl` (16 742 instances): an edit naming a `MeshId` this session
  had never decoded returns in 0.04 ms and is on screen in 0.90 ms (p95 1.00
  ms), against a ~29 ms full reload plus a fetch and decode before this; the
  mesh itself is fetched, decoded, uploaded and swapped in 3.10 ms from the
  edit. A `TextureID` edit: 0.04 ms, 0.93 ms to the frame, 3.31 ms to the
  swapped one. The first drawable frame of a cold load went from 1004 ms to
  462 ms, with the finished picture at 978 ms — the whole place still takes
  about as long to arrive, it just stops being a blank wait. A full reload
  went from 29.0 ms to 16.8 ms first frame readable, since it no longer
  walks the asset tables through a thread pool. `TestPlace.rbxl`: cold load
  573 ms to 255 ms first frame (388 ms complete), reload 1.6 ms. Verified
  pixel-for-pixel: the frame after a landed mesh is byte-identical to the
  frame the place draws with that mesh resident from the start, and the
  fallback drawn before it is byte-identical to a place built with the asset
  withheld (`crates/rbx_viewer/tests/streamed_pixels.rs`, `#[ignore]`d,
  needs a GPU and `RBX_STREAMING_FIXTURE`); all seven `scripts/shots.sh`
  reference captures are unchanged. — @chteau

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

- **Reload reuse: review fixes.** Four things the reload work above got
  wrong, caught in review. A failed asset fetch was remembered as failed for
  the life of the `Headless`, so a network blip during one load left that
  decal bare through every later reload; `load::Resident` now forgets a
  failure of the machine's (a request that did not complete, a cache that
  would not write, a key not yet configured — see `assets::Failure`) at the
  start of each load — one load still asks it once however many passes name
  it, the next `Headless::reload` tries it again — while a failure of the
  asset's (a 404, a file the content package does not hold, bytes that will
  not decode) stays remembered, because asking again cannot change the
  answer and the ask is the expensive part: `TestPlace.rbxl` names a
  `SpawnLocation.png` its package lacks, and retrying that on every reload
  measured 220 ms a time. When no asset resolver could be built at all (an
  unwritable cache directory, say) the warning was filed under a reference no caller ever
  looked up and reached only stderr; `assets::load_with` now fails every
  requested reference with that message, so it reaches the Output dock, once,
  and is retried like any other failure. The `Trail`, `Beam`,
  `ParticleEmitter` and GUI passes read their quality on/off toggle once when
  first built and never again, so a level changed between two reloads was
  ignored until the place was reopened; every `rebuild` re-reads it. And the
  GUI atlas decoded its `ImageLabel` images on its own, so an image used both
  as a `Decal` and in a GUI was decoded twice — they come out of the same
  `Resident` now, which also puts their warnings in the dock. In passing: the
  reload's dedup helpers (`distinct`, `untried`, `reuse_plan`) scanned a
  `Vec` inside a loop, O(n²) on a place naming hundreds of assets — they hash
  now — and a reload no longer copies the bytes of a union it already carved
  out of the resident table. — @chteau

- **Scale handles sit on the part again.** The Scale tool drew small blocks
  on the end of the same screen-relative arms the Move tool uses, so a
  baseplate's handles hung in a little cluster around its pivot instead of
  anywhere near the plate. They are now balls on the middle of each of the
  six faces of the part's own box — `gizmo::Faces`, built from the
  placement matrix the selection outline already draws its corners from —
  which is the shape `creator-docs` gives the engine's own equivalent
  (`Enum.HandlesStyle.Resize`: spheres "for resizing an adornee along its
  face axes"). Each ball is still sized for the camera, now at its own
  distance rather than the part's centre's, so the far corner of something
  enormous stays grabbable. Move and Rotate are untouched. — @chteau

- **Four fixes to the selection outline.** A `BasePart` with parts parented
  under it — a welded assembly, a `Tool`'s `Handle` with a sight on it — was
  read as a container and outlined with a loose world-axis-aligned box around
  itself and its children instead of its own tight one; `pick::Selected` now
  answers that from the instance's class, and a part stands for itself alone,
  since nothing in Roblox moves a child part because its parent part moved. A
  reload refreshed the draggers but not the box, so a Command Bar script
  parenting another `Part` under the selected model left the outline and the
  gizmo describing the membership the selection had before, until the user
  reselected — both halves are read together now. Dragging a model rebuilt
  the whole aggregate box once per part moved rather than once per frame,
  which made one step of a group drag quadratic in the number of parts.
  And selecting a model together with one of its own parts drew two boxes
  over each other: the dedup `transform::Targets::read` already did is now
  `pick::selection`, which the outline and the targets both come from. —
  @chteau

- **A selected `Model` finally has an outline and a gizmo.** Selecting a
  `Model`, a `Folder`, a `Tool` or any other container drew nothing at all and
  showed no Move/Scale/Rotate handles — and since a viewport click selects the
  outermost `Model` around whatever it hit, that was most selections a user
  makes by clicking. `rbx_viewer::pick::parts_of` now resolves a selected
  instance to the drawable parts beneath it, and both sides go through it: the
  renderer outlines a container with one world-axis-aligned box around
  everything under it, and `transform::Targets::read` gives the editor's own
  hit-testing the same parts, so a Move drag carries a whole model and Scale
  and Rotate anchor on a real part rather than on a `Model` that has no `Size`
  or `CFrame` to write. Meshes were missing the same two things for a second
  reason: `Scene::placements` deliberately leaves out a part whose box a
  resolved `MeshPart`/union mesh replaced, so that a decal is never projected
  onto geometry nothing draws — the outline now reads `Scene::all_placements`
  instead, which keeps them. — @chteau

- **Hover outline: fixed flicker on Wayland orbit, and the per-move
  raycast.** Two review fixes on the new viewport hover outline. Hover
  suppression during a camera look gated on `PointerLock::holds`, which is
  always `false` on Wayland (no OS-level pointer capture there by design —
  see `pointer_lock`'s module doc) — every mouse-move during a Wayland
  orbit was re-resolving and redrawing the hover box against the moving
  cursor. It now gates on whether a look gesture is actually in progress
  (`WorkspaceView::looking`, set by `begin_look`/`end_look`) instead.
  Separately, hover resolution called `pick::parts_along` — a full-scene
  raycast, per-triangle for `MeshPart`s — on every reported mouse-move
  event, not just once per rendered frame; it's now throttled to at most
  one resolve per frame interval (`workspace_view::hover::due`), with the
  latest cursor position always caught up to once due. — @chteau

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

- **The benchmark harness can no longer report a number that isn't the
  number.** Three ways it could. A failed `cargo build` was swallowed by the
  `|| true` on the pipeline that filtered its output, so `scripts/bench.sh`
  ran on whatever binary the last successful build had left in `target/` and
  printed its timings under the *current* commit — the build's status is now
  taken on its own, and the filtering happens afterwards over its log.
  A fixture that failed part-way through a run took the run with it: with
  `TestPlace.rbxl` already measured and `marked.rbxl` unreadable, neither the
  table nor `bench.json` was written at all, and minutes of GPU work went with
  it. Each fixture now carries its own result, so the run reports everything
  that did measure alongside which fixture failed and why, and still exits
  non-zero. `--frame-warmup 0` left `render_frame`'s pipeline empty, which
  makes the first measured frame come back as `None` — the phase collected
  nothing and printed a tidy `0.00 ms` for it, indistinguishable from a real
  sub-millisecond result; the warmup now parses like every other iteration
  count (at least 1), and any column with no samples behind it says so
  instead of printing a zero. The patch phase also tells an exhausted
  candidate search apart from a place with nothing patchable in it, rather
  than reporting both as the latter. — @chteau

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
