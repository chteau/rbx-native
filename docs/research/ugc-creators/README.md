# UGC creators: what rbx-native has, what's missing

Goal: someone who makes avatar items (rigid accessories, layered clothing,
bodies, dynamic heads) can build, check, test and upload them from
rbx-native without opening real Studio. Written 2026-10-25. Docs read are
the `creator-docs` repo pages linked below.

## What real Studio gives a UGC creator

- **Accessory Fitting Tool (AFT)**, Avatar tab > Accessory. Pick a
  `MeshPart`/`Model`, pick Clothing (layered) or Accessory (rigid), pick the
  `AssetType` and body scale, then try it on preset bodies, animations and
  other accessories. It converts the mesh into the real `Accessory` object.
  <https://create.roblox.com/docs/avatar/accessory-fitting-tool>
- **Avatar Setup / Auto-Setup**. Takes a plain mesh and adds the R15 rig,
  skinning, facial animation (FACS), cages, the 15-part split and the
  attachments. Needs a specific input shape (head components, naming).
  <https://create.roblox.com/docs/avatar-setup>,
  <https://create.roblox.com/docs/avatar-setup/auto-setup-requirements>
- **Layered clothing**: `WrapLayer` on the item (inner cage = how it fits the
  body, outer cage = how other layers fit over it), `WrapTarget` on bodies.
  Must be skinned to the R15 armature, max 4 bone influences per vertex.
  <https://create.roblox.com/docs/avatar/layered-accessories/specifications>
- **Rigid accessories**: one `Accessory` with a `Handle` and an attachment,
  fitted by body scale.
  <https://create.roblox.com/docs/avatar/rigid-accessories/specifications>
- **Bodies and dynamic heads**: 15 watertight parts, outer cage on every
  part, bounding box limits per part, and for heads at least 17 FACS poses,
  eye/mouth landmarks in the cage, and 5 facial actions that must deform.
  <https://create.roblox.com/docs/avatar/character-bodies/specifications>,
  <https://create.roblox.com/docs/avatar/dynamic-heads/specifications>
- **Validation rules** (the ones that matter for a checker):
  - 4k triangles max for rigid and layered accessories, 20k per mesh in
    general (`art/modeling/specifications`)
  - Marketplace textures 2048x2048 max
  - max size per accessory type in studs, centred on the attachment
  - cages: never delete vertices or touch UVs, inner and outer cage named
    after the item
  - bodies watertight, each limb inside its bounding box
  - Roblox also ships a Blender/Maya "Layered Accessory Validation Tool"
    (<https://create.roblox.com/docs/art/accessories/validation-tool>)
- **Upload and publish**. Explorer > right click > Save to Roblox, Content
  Type "Avatar Item", pick title, asset category, creator (user or group).
  Studio validates, then you pay an upload fee (80 Robux per submission, 500
  if it uses an emissive mask) and it goes into moderation (up to 24h). After
  that you publish it on the Marketplace, which takes a refundable
  publishing advance. Creator must be ID verified or have a parental link.
  The fees page: <https://create.roblox.com/docs/marketplace/marketplace-fees-and-commissions>,
  the rigid walkthrough:
  <https://create.roblox.com/docs/art/accessories/creating-rigid/publishing>
- **Testing**: the AFT's preview bodies, and "test" docs for bodies and heads
  (<https://create.roblox.com/docs/avatar/character-bodies/test-studio>).

## What the Open Cloud Assets API can and can't do

From <https://create.roblox.com/docs/cloud/guides/usage-assets>: it creates
Animation, Audio, Decal/Image, Model (`.fbx`, `.gltf`, `.glb`, `.rbxm`,
`.rbxmx`, imported as a Model with MeshParts) and Video, one asset per call,
20 MB max. Mesh create is Roblox-only. Updating content only works for
`.fbx`. The page says nothing about avatar items or Marketplace listing.

So this is the hard wall: **the upload fee, moderation submission and
Marketplace publish are not in Open Cloud as far as the docs say.** What we
can do is upload the Model and hand the user to the Creator Hub for the
last step. Nobody has checked if a Model uploaded through the API can be
turned into an avatar item there; that needs a live try (stage 5).

## What rbx-native has today

Checked against `ROADMAP.md` "What's been implemented", `CHANGELOG.md` and
the code on `origin/dev`.

- **3D import** (`crates/rbx_import`, `shell/import3d.rs`): `.fbx`, `.obj`,
  `.gltf`, `.glb`, per-mesh triangle count against the **20k** general limit
  (`TRIANGLE_LIMIT`), uploads the self-contained file through
  `rbx_cloud::create_asset`. There is no 4k accessory budget and no
  asset-type awareness.
- **Auto-rigger** (`crates/rbx_autorig`): humanoid mesh to the 15 R15 parts,
  joints, bone-heat skinning (4 influences, sum 255), fit panel, draggable
  joints, accessory attachments from the fit, and the Mannequin's outer
  cages bent onto the fit (`fit_cages`), uploaded as `<Part>_OuterCage`.
  Live upload confirmed Roblox's importer makes one `WrapTarget` per part
  and a layered jacket and pants follow the published body. This is the body
  side of Avatar Setup, minus facial animation and head work.
- **Rig Builder, Player / My Avatar**: builds R6/R15 rigs, loads a real
  player's avatar with accessories welded at the right attachment, layered
  clothing keeping its `WrapLayer` and fitted cage to cage (see stage 3
  below). Close to Roblox's fit, not its exact deformer.
- **Properties, Explorer, viewer**: `Accessory`, `Attachment`, `WrapLayer`,
  `WrapTarget` and `FaceControls` load, show and edit as normal instances.
- **Save/Publish to Roblox** uploads places. Nothing there uploads an
  avatar item.
- **Skinned mesh rendering** (mesh v4 to v7) so a rig bends in the viewport.

## Gap analysis

### Have
Import and upload of a Model, auto-rig plus body cages, rig and avatar
loading, instance editing for all the avatar classes, skinned rendering.

### Missing (we can build it)
1. **Accessory Fitting Tool**: pick a mesh, pick Clothing or Accessory, pick
   the asset type and scale, preview on preset bodies, animations and other
   items, write out the real `Accessory` (`Handle`, attachment, `WrapLayer`).
2. **Validation panel** with the real numbers above: 4k tris, 2048 textures,
   size per asset type, max 4 influences, cage vertex and UV integrity,
   watertight bodies, bounding boxes, 17 FACS poses. Run before any upload.
3. **Cage authoring for items**: auto-make an inner/outer cage for a
   layered accessory (the auto-rigger only does body cages), show the cage
   as an overlay, and a way to nudge it.
4. **Real layered-clothing preview**: was a nearest-vertex blend, now a
   cage-to-cage fit checked against real Studio (stage 3).
5. **Head side**: dynamic head cage landmarks, FACS pose check, facial
   animation preview. `FaceControls` already loads.
6. **Avatar Setup for items**, not just bodies: attachments and skin for a
   rigid or layered item from a plain mesh.
7. **Upload flow**: a window that validates, uploads the Model, then opens
   the Creator Hub item page, with the fee spelled out before anything is
   sent.
8. **Test**: wear the item on the Player / My Avatar rigs and the preset
   bodies, play animations, swap body scale.

### Blocked by Roblox
- **Upload fee, moderation submit, Marketplace publish and the publishing
  advance** have no documented Open Cloud endpoint. We can't do the last
  step from here, so the plan ends in a handoff to the Creator Hub.
- **Mesh create** in the Assets API only takes content from Asset Delivery.
- **Roblox's actual deformer and server-side validator** aren't available.
  We can mirror the documented rules, not the checker itself.
- Avatar Schema changes (procedural bones, higher mesh resolution, more FACS,
  silhouette-preserving fit) are announced for 2027 and not in the API dump
  yet. See the Creator Roadmap section of `ROADMAP.md`.

## Staged plan

1. **Validator** (stage 1). Done: `crates/rbx_ugc` (no UI; `validate(&Item)`
   returns findings with rule, severity, measured value, limit and the part)
   and Avatar › Check Item. Import 3D can check a file as an avatar item.
   - Checked in real Studio 2026-10-28 on generated items (1k, 5k and 21k
     triangle spheres, a 4-stud hat, a 4096 texture, a shirt with no
     cages): Save to Roblox gave the same verdicts as ours, with the same
     numbers (5000 over 4000, 4096x4096 over 2048x2048, the Hat box
     [3, 4, 3]).
   - What Studio taught us that no page says: the size box comes from the
     handle's `AvatarPartScaleType` value and an accessory without one is
     sized as Classic (so an unmarked hat gets 3 x 4 x 3, not Normal's
     1.87 x 2.5 x 1.87); a rigid handle needs a `TextureID` or a
     `SurfaceAppearance`, a layered one doesn't. Both are mirrored.
   - Second pass, Studio 0.745 on 2026-11-02, Save to Roblox run up to its
     verdict on about 50 generated hats, shirts, eyebrows, eyelashes and
     shoe pairs (owned meshes, nothing published). All of this is now
     mirrored, with Studio's wording, in `rbx_ugc::schema` and the item
     rules:
     - Schema. An accessory may hold one `Handle` (`MeshPart`, or a `Part`
       with a `SpecialMesh`) and a `ThumbnailConfiguration`
       `Configuration`, which then needs an `ObjectValue`
       `ThumbnailCameraTarget` and a `CFrameValue` `ThumbnailCameraValue`.
       The handle may hold its one type attachment (the attachment itself
       can't have children), an `AvatarPartScaleType` `StringValue`
       (Classic, ProportionsSlender or ProportionsNormal), an
       `OriginalSize` `Vector3Value`, `SurfaceAppearance`, and a
       `WrapLayer` on layered items only. Everything else (a `Part`,
       `Decal`, `Sound`, `Weld`, `ParticleEmitter`, a `Folder`, a second
       attachment or handle, a value under the wrong parent) is listed with
       every descendant in one "not part of the approved schema" error. A
       `WrapLayer` on a rigid item adds "Asset type 'Hat' is not a layered
       clothing category". A renamed handle gives "Could not find required
       MeshPart called Handle". Studio stops at a schema error and hides
       the rest; ours lists everything at once.
     - Tags anywhere, and attributes anywhere but on the handle itself, are
       errors; an anchored handle is too ("Expected to be = false"), rigid
       and layered alike. A script inside gives only "Unknown error" in
       Studio (its log says the asset quality job failed); ours lists it
       as a stray instance.
     - "Missing HSR data" isn't about HSR assets: Studio makes that data
       itself, and a layered item with both cages passes without it. It
       shows up, next to "Missing InnerCage (i.e. invalid
       ReferenceMeshId)", only when the `WrapLayer` has no
       `ReferenceMeshId`.
     - Eyebrows and eyelashes need `WrapLayer.AutoSkin` EnabledOverride and
       the mesh joints `RBX_Leader` and `RBX_Follower` ("must use skinning
       transfer"); `FaceFrontAttachment` is accepted. A layered handle whose
       `Size` isn't its `MeshSize` is refused ("mesh parts with wrap layers
       do not support scaling").
     - Shoes: the box is 8 x 8 x 8 like the rest of layered clothing ("The
       max size for type LeftShoeAccessory is [8.00, 8.00, 8.00]"), on
       `LeftFootAttachment` / `RightFootAttachment`. The pair must come as
       one `Model` holding `LeftShoeAccessory` and `RightShoeAccessory`;
       anything else is "Unknown error" in the dialog and "Expected
       LeftShoeAccessory and RightShoeAccessory for shoe validation" in the
       log.
   - Import 3D now decimates a mesh over 20,000 triangles like Studio's
     importer instead of refusing it (meshoptimizer's quadric simplifier,
     OBJ, glTF, GLB and binary FBX; only an ASCII FBX is still refused).
     Same 21,000-triangle sphere in both: Studio's comes out at exactly
     20,000 faces, 1.19959 x 1.2 x 1.19972 studs, edges 0.0046 to 0.0405;
     ours 20,000 faces, the same box to 1e-5, edges 0.0022 to 0.0405.
   - Still not mirrored, and why: the cage numbers (outer cage inside the
     mesh, mesh outside the outer cage, cage-to-mesh distance 0.60, "no
     more than 14% of the modified UVs" outside the body region) and
     eyebrows' "deforms beyond 1.95, 0.65, 0.65 when placed on an
     avatar", because each needs Studio's own fit of the item on its body
     and its region maps; the ownership checks ("not owned by an authorized
     creator"), which are server side.
   - Watertight: no Studio message about open meshes came up on any item,
     and every mesh owned for these tests is closed, so whether Studio
     refuses an open mesh is unknown (its validator does measure it, the
     file list has an asset quality "watertight" measure). Ours stays a
     warning, counted by welding positions and finding edges with one
     face; `rbx_autorig`'s voxeliser isn't needed for that.
   - Template cages: Studio doesn't hold cages to a vertex count. It passed
     a shirt whose inner and outer cages are 5410 and 5408 vertices, and
     checks where the cage UVs land on the body per type instead (above).
     Ours used to refuse that pair; a vertex count that differs from the
     inner cage or the template is now a warning, and UVs are still
     compared vertex by vertex when the counts match.
   - Side by side in ours (Check Item on the same places): the stray-part,
     script, second-attachment, no-ReferenceMeshId shirt and eyebrow items
     give the same messages as Studio, word for word.
   - The AFT now gets past Select (pick the part, Clothing or Accessory,
     the type, the body) to its fitting view, and shows no checks of its
     own on the way: in Studio the checks live in Save to Roblox and the
     Avatar tab's UGC Validation panel, which is what ours mirrors.
     Generate was not pressed.
   - No page gives a publishing advance for eyebrows and eyelashes, and
     that row stays empty. The Slender hair depth row adds up wrong on the
     page (1.892 + 1.189 for 2.08); 0.892 front is used.
   - Can't be tested here, ever: whether the Creator Hub turns one of our
     uploads into an avatar item, and whether it takes a cage made from
     the Mannequin's. Both need an account in the UGC program, and Chee's
     isn't in it.
2. **Fitting tool, rigid items** (stage 2). Done: Avatar › Accessory opens
   the Accessory Fitting Tool. Pick a `MeshPart` or `Model`, Accessory or
   Clothing, the asset type (Studio's list, Shoulder with its five
   attachments) and the body scale, place it on the test body, Generate.
   - Checked against Studio's AFT run on the bow (6874004992), its output
     saved and dumped: the `Accessory` is named after the source with
     `AccessoryType` Unknown, the source part becomes `Handle` and keeps
     its mesh, size and children, then a `StringValue`
     `AvatarPartScaleType` (Classic, ProportionsNormal, ProportionsSlender)
     and an `Attachment` named for the type whose `CFrame` is the handle's
     inverse times the body attachment's world `CFrame` (identity for an
     untouched fit, `(0.783, 0, 0)` after a 0.783 stud move). `Anchored`
     and `Massless` off, rigid `CanCollide` on, layered off. Ours writes
     the same; the generator's tests are built from that dump.
3. **Fitting tool, layered items plus cage authoring** (stage 3). Done:
   Clothing makes `<item>_InnerCage` and `<item>_OuterCage` from the
   Mannequin's body cages (`rbx_autorig::item_cages`: the template welded,
   pushed out along its normals until it clears the item, smoothed, every
   UV kept; each bit of the item pushes only the cage vertex nearest it),
   shows them as an overlay with a nudge by region, and writes a
   `WrapLayer` the way Studio's tool does (renamed `HandleWrapLayer`,
   Order 1, Puffiness 1, ShrinkFactor 0, `AutoSkin` kept).
   - The deformer is new (`rbx_viewer` `wrap/cage_map.rs`): body cages and
     every item cage share one template, and the template UV is the
     pairing key (checked: all 705 UVs of the Mannequin's UpperTorso cage
     are in the Denim Jacket's inner cage). Each garment vertex is bound to
     the closest spot on its inner cage and rebuilt at the same spot of the
     body cage with the same clearance; each layer's outer cage becomes the
     surface the next layer is worn over.
   - Checked in real Studio on the catalog Denim Jacket (8516609675) and
     Cargo Pants (6984740059) on the default blocky body at Normal,
     Slender and Classic, same camera: garment-pixel XOR over union went
     from 0.81 / 0.75 / 0.76 with the old nearest-vertex fit to 0.11 /
     0.13 / 0.12 (whole frame 0.78 to 0.12).
   - Hidden surface removal (`wrap/hsr.rs`): Studio hides the body under
     the cloth, which left grey spots between the legs in ours. Roblox's
     only page on it
     (<https://create.roblox.com/docs/reference/engine/classes/HiddenSurfaceRemovalAsset>)
     says the data marks "the faces of a wrap's mesh that are fully
     occluded by clothing layers worn above it" so the engine strips them;
     the format isn't published. Every `WrapLayer` and `WrapTarget` carries
     it, inline as `HSRData` (the Mannequin's packages, the Denim Jacket)
     or as an `HSRAssetId` model (the blocky body, the Cargo Pants).
     Read off those (versions 16 and 18): a bit per inner cage vertex, 0
     where the layer covers what's under it (the pants' 0 bits are the
     legs and hips), then per mesh face the cage vertices it sits under
     (the outer cage for a layer: 0.14 studs from the face on average,
     1.9 on the inner one). A face hides when every one of those is
     covered, by UV, by a layer above it. Studio's scene saved and drawn
     by ours from the same cameras: XOR 0.119 / 0.124 / 0.128 down to
     0.070 / 0.068 / 0.075 on Classic / Normal / Slender, the grey patch
     between the legs gone. With the garments made transparent in both,
     Studio and ours hide the same faces (neck, the strip under the open
     jacket, hands, ankles and the blocky shoulders, feet and head stay).
     Ours draws the back of that strip from behind where Studio culls it,
     which only shows with see-through clothing.
   - Stock bodies get the `HSRAssetId` the reference avatar models give
     each body mesh, and a player's package parts keep theirs. Those
     stock HSR assets answer 401 without sign-in, so the fitting tool's
     test body hides under clothing only with an API key; not checked with
     one.
   - `WrapLayer` tuning, measured in Studio on the same jacket (Normal
     body, one camera per setting): `BindOffset` moves the fitted garment
     as a whole, about the character's `HumanoidRootPart`. `(0, 0.5, 0)`
     raises it half a stud (21 px in Studio, 20 px in ours, same camera),
     `CFrame.Angles(0, 0, 0.3)` turns it about the root part, not the
     handle (checked on a 1.5x tall body where the two sit 0.58 studs
     apart). Ours does the same. `Puffiness` (-1, 0, 0.5, 1,
     and 2, which Studio clamps to 1) and `ShrinkFactor` (-1, 0.5, 1) draw
     pixel for pixel the same as the defaults in Studio, alone or over a
     second layer; the API page marks both deprecated. So they're written
     but not applied, and the tool clamps them to -1..1 like Studio.
   - The Mannequin jacket isn't slimmer than Studio's. The 0.31 (3736
     against 5363 px) came from the measurement: our test scene had no
     `Lighting` and kept the jacket's Plastic, so the brighter highlights
     fell out of the magenta mask. Studio's 5363 px comes back exactly from
     the same camera; our fitting tool's own Mannequin and jacket, with
     Studio's `Lighting` and SmoothPlastic, read 5185 px and XOR 0.054,
     the same as Studio's own scene drawn by ours (0.054 jacket, 0.059
     jacket and pants). Same meshes, cages, sizes and handle on both
     sides; the outlines sit on each other.
   - Eyebrows and eyelashes: Studio's AFT lists them with the rigid types,
     but Save to Roblox on a rigid eyebrow says "Could not find required
     WrapLayer" and "Asset type 'EyebrowAccessory' is not a rigid
     accessory category. It can only be used with layered clothing." The
     tool keeps them in the Accessory list and fits them layered on the
     Head, and Check Item keeps asking for their cages.
   - Not checked: whether Roblox's own
     validator accepts cages built from the Mannequin template once
     uploaded, since that takes a paid upload.
4. **Heads** (stage 4). Done: `rbx_ugc::head` (FaceControls, the 17
   required FACS poses present and moving, the 5 facial actions moving
   their landmark, eye and mouth landmarks found on the cage and close to
   the mesh, the emote box, part settings, extra objects), FACS read from
   the mesh (`rbx_mesh::Facs`) and posed in the viewer from `FaceControls`
   the way Studio does, and Avatar › Face Preview (a slider per control,
   the 5 expressions, landmarks drawn over the face).
   - Checked in real Studio 2026-10-28 on Roblox's own
     `characterR15DynamicHeadV2`. Same camera, `JawDrop`, `LeftEyeClosed`
     and `Pucker` at 1: the dark eye and mouth regions overlap Studio's at
     IoU 0.91 (0.83 at rest), the open mouth is within 2% of Studio's area
     and the closed eye's slit 339 against 389 px. The outline match
     reads IoU 0.89, held back by our sky being close to the head's
     colour (the edges sit on each other in the overlay).
   - Save to Roblox › Avatar Item › Head on the same head (the validation
     step only): Studio says `LeftCheekRaiser` and `RightCheekRaiser` are
     inactive, same as ours. Removing `FaceControls`, putting a hand mesh
     in (no FACS) and pointing the cage at a hand cage (no landmarks) gave
     Studio's "Could not find required FaceControls", "missing FACS
     information ... at least 17 poses" and "must be properly caged so
     that the eyes and mouth vertices on the cage line up" errors, which
     ours raise too.
   - What Studio taught us that no page says, now mirrored: a head needs an
     `AvatarPartScaleType` `StringValue` and `CollisionFidelity` Default
     (Roblox's own head ships as Box, stored as a `BLOCK` marker in
     `PhysicalConfigData`), and the outer cage keeps the template's 343
     unique UVs (to 1e-4).
   - Not mirrored: Studio's asset ownership checks, its cage-to-mesh
     distance (0.60) and cage-inside-mesh percentages, and the server side
     scoring of facial actions. Our landmark thresholds (projection under
     8% of head height, an action moving its landmark over 1%) are ours.
   - A head with one required pose missing was not run in Studio: making
     one needs a new mesh asset and nothing gets uploaded. It is unit
     tested only.
5. **Upload and handoff** (stage 5). Done: Avatar › Upload Item (type,
   title, creator, the check, the fees, Model upload through Open Cloud,
   Creator Hub link). Tested against a mock Open Cloud server.
   - Still to check, live: does a Model uploaded by the API show up as
     something the Creator Hub can turn into an avatar item? The window
     says it isn't confirmed. Needs the tester's account, a key with the
     assets scope, and real Robux for the upload fee, so Chee has to be
     the one pressing it.
6. **Test rig** (stage 6). Done, inside the fitting tool: Player / My
   Avatar bodies and the Normal, Slender and Classic presets, the default
   animations, body scale swaps and other items layered on top.
   - Check: the same jacket and pants on the same three bodies in real
     Studio, screenshot against screenshot (the stage 3 numbers above).

Every stage needs a real place or item opened in real Studio on an X server
before it counts as done, same as the rest of the project.

## Open questions

- Which accessory types the AFT lists today. Save to Roblox's Asset
  Category list (2026-10-28): Body, Head, Shoes, Avatar Animations, Back,
  Dress Skirt, Emote, Eyebrow, Eyelash, Eye Makeup, Face, Face Makeup,
  Front, Hair, Hat, Jacket, Lip Makeup, Neck, Pants, Shirt, Shorts,
  Shoulder, Sweater, T-Shirt, Waist.
- Whether the Open Cloud Assets API will ever take avatar item metadata.
- Whether Studio refuses an open (non-watertight) accessory mesh. Needs an
  owned open mesh to run through Save to Roblox.
