# Auto-rigger spec: fitting an R15 skeleton to an unrigged humanoid mesh

> **Note.** This is the research spec written *before* `crates/rbx_autorig` and the viewer's skinning were built. For what exists now, see [`animation-handoff.md`](animation-handoff.md). The `/tmp/claude-0/...` and `scratchpad/` paths below are where the sources were read during that research session. They don't exist in the repository; the upstream URLs next to them do.

Researched 2026-10-11 for rbx-native. No repository files were changed.

## Sources and notation

| Tag | What it is |
|---|---|
| `CD:` | Roblox/creator-docs `main`, local clone at `/tmp/claude-0/research/creator-docs/content/en-us/`. Upstream: `https://github.com/Roblox/creator-docs/tree/main/content/en-us/`. |
| `CDH:` | creator-docs history clone at `/tmp/claude-0/research/creator-docs-hist` (HEAD `af5f853c`, 2026-10-10). Older files are read with `git show <rev>:<path>`. |
| `API:` | Engine class reference YAMLs fetched 2026-10-11 from `raw.githubusercontent.com/Roblox/creator-docs/main/content/en-us/reference/engine/classes/<Class>.yaml`. Saved in `/tmp/claude-0/research/raw3/` and `/tmp/claude-0/research/raw/`. |
| `DUMP:` | `/tmp/claude-0/research/raw/Full-API-Dump-latest.json` (Studio 0.742, from the MaximumADHD client-tracker mirror; see `animation-research.md`). |
| `REPO:` | This repository, `/home/user/rbx-native/crates/...`. |
| `RE:` | Public reverse-engineering code, cloned or fetched into `/tmp/claude-0/research/raw3/mesh/`: |
| | `MaximumADHD/Roblox-Utils` `Modules/RobloxMesh/init.luau` @ `5926d8b5`, the most complete public reader (v1–v7, skinning, FACS) |
| | `AntiBoomz/BTRoblox` `js/rbx/Parser/MeshParser.js` @ `7f0cc7a7` |
| | `krakow10/rbx_mesh` (Rust, binrw) `src/mesh/v4.rs`, `v5.rs` @ `0398e5da` |
| | `MaximumADHD/Rbx2Source` `Geometry/Mesh.cs` @ `0f8fe7c4` |
| | `steinann/RoAvatar-Renderer` @ `c11bfe0f` (three.js avatar renderer) |
| `EMP:` | My own measurement, by parsing a real mesh file (scripts in the session scratchpad). |
| `PIN:` | Pinocchio reference source, `pmolodo/Pinocchio` @ `14fe424b`, cloned to `/tmp/claude-0/research/raw3/papers/Pinocchio`. |

**Not reachable from this sandbox:** devforum.roblox.com, web.archive.org, about.roblox.com, people.csail.mit.edu, homes.cs.washington.edu and alecjacobson.com. Those pages are cited only through search-result summaries, and each such citation is marked **(secondary)**.

**Labels used in the text:**
- **[DOC]**: documented by Roblox.
- **[RE]**: established by public reverse engineering.
- **[EMP]**: measured here.
- **[INFERRED]**: my deduction from the cited evidence.
- **[RECOMMEND]**: my design proposal.
- **UNVERIFIED**: no primary source was found.

---

## 1. R15 body: parts, cuts, attachments, joints, Humanoid

### 1.1 The 15 parts and HumanoidRootPart

- **[DOC]** The 15 MeshParts are listed below (`CD:avatar/character-bodies/index.md` "Body parts"; `CD:avatar/character-bodies/specifications.md` "Body parts").

  | Region | Parts |
  |---|---|
  | Head | `Head` |
  | Torso | `UpperTorso`, `LowerTorso` |
  | Left arm | `LeftUpperArm`, `LeftLowerArm`, `LeftHand` |
  | Right arm | `RightUpperArm`, `RightLowerArm`, `RightHand` |
  | Left leg | `LeftUpperLeg`, `LeftLowerLeg`, `LeftFoot` |
  | Right leg | `RightUpperLeg`, `RightLowerLeg`, `RightFoot` |

  They are `MeshPart`s "nested under a single `Model`". In the source DCC file each mesh object is called `<Part>_Geo`, for example `LeftUpperArm_Geo`.
- **[DOC]** For the Marketplace, the 15 parts are grouped into 6 assets, each with a triangle budget (`CD:avatar/character-bodies/specifications.md` "Triangle budgets"):

  | Asset | Parts | Triangle budget |
  |---|---|---|
  | DynamicHead | Head | 4000 |
  | Torso | UpperTorso, LowerTorso | 1750 |
  | LeftArm | LeftUpperArm, LeftLowerArm, LeftHand | 1248 |
  | RightArm | RightUpperArm, RightLowerArm, RightHand | 1248 |
  | LeftLeg | LeftUpperLeg, LeftLowerLeg, LeftFoot | 1248 |
  | RightLeg | RightUpperLeg, RightLowerLeg, RightFoot | 1248 |
  | **Total** | | **10,742** |

  A general mesh may have at most 20,000 triangles (`CD:art/modeling/specifications.md`). `REPO: rbx_import::TRIANGLE_LIMIT` uses the same number.
- **[DOC] Caps.** "Limbs must be capped, so that they form a full watertight mesh when separated from the body. Caps can be rounded or flat" (`CD:.../specifications.md` "Body parts"). Avatar Setup "segments and adds caps to the character's limbs, which may add to your total polycount" (`CD:avatar-setup/auto-setup-requirements.md` item 4).
- **[DOC] Visibility and Marketplace properties** (`CD:.../specifications.md`):
  - Parts must be fully opaque and fill at least 50% of their bounding box in front, side and back views.
  - `Material = Plastic`, `Transparency = 0`, `VertexColor = 1,1,1`.
- **HumanoidRootPart**
  - **[REPO]** The repo's rig builder makes the HumanoidRootPart:
    - A `Part` block of `Size 2,2,1`, taken from Roblox's reference avatar models (`REPO: rbx_studio/src/shell/rig/bundle/tables.rs`, `init: [2.0, 2.0, 1.0]` for both Classic and Mannequin).
    - `Transparency 1` for the root part only (`build.rs::make_part`).
    - Children: `RootRigAttachment` and `RootAttachment` at `(0,0,0)`.
    - It is set as the `Model.PrimaryPart`.
  - **[DOC]** In an R15 FBX/glTF, the root is the bone chain `Root → HumanoidRootNode → LowerTorso …`. "The LowerTorso and Root bone or joint position must be set to `0, 0, 0`" (`CD:.../specifications.md` "Standard R15 rigs").
  - **[INFERRED]** The importer turns `Root`/`HumanoidRootNode` into HumanoidRootPart. It is not a mesh. Weights on `Root` or `HumanoidRootNode` "are dropped when importing" (`CD:art/modeling/rig-a-humanoid-model.md`, `skin-a-humanoid-model.md`).

### 1.2 Where the cuts go

**[DOC]** No public document gives cut heights. Two constraints are documented:

1. A rig attachment must sit at the cut. Validation error: "Rig attachment must be placed near the joint boundary where this body part connects to an adjacent part. Current position is too far from the connection plane" (`CD:marketplace/validation-system.md`, around lines 159–161).
2. Each part must be a capped, watertight volume.

The Blender rig tutorial says each bone "should be positioned in the center of their corresponding mesh objects" (`CD:art/modeling/rig-a-humanoid-model.md`). Read with the R15 hierarchy, this means the bone's head (the joint) sits at the boundary with the parent part and its tail runs through the part.

**[EMP] Reference proportions.** I computed these from the attachment tables of Roblox's own reference bodies (`REPO: rig/bundle/tables.rs`), assembled with `Part1 = Part0 · C0 · C1⁻¹`. Script: `scratchpad/layout.py`. `H` is total height (feet to head top), measured from the ground. Joint positions are in studs, with HumanoidRootPart at the origin. The Mannequin (Rthro "Normal", at InitialSize) is **authored in an A-pose**: the upper arm hangs 33° from vertical.

| Joint (Part0 → Part1) | Mannequin world pos | Mannequin y/H | Classic y/H |
|---|---|---|---|
| Root (HRP → LowerTorso) | (0, −0.849, 0) | 0.533 | 0.382 |
| Waist (LowerTorso → UpperTorso) | (0, −0.375, −0.107) | 0.602 | 0.460 |
| Neck (UpperTorso → Head) | (0, 1.285, −0.006) | **0.844** | 0.770 |
| Shoulder (UpperTorso → UpperArm) | (±0.599, 0.912, 0.059) | 0.789 | 0.696 |
| Elbow | (±1.132, 0.091, 0.056) | 0.670 | 0.580 |
| Wrist | (±1.660, −0.722, 0.054) | 0.551 | 0.477 |
| Hip (LowerTorso → UpperLeg) | (±0.406, −1.000, −0.118) | 0.511 | 0.389 |
| Knee | (±0.406, −2.502, −0.087) | 0.292 | 0.201 |
| Ankle | (±0.406, −4.098, 0.084) | 0.060 | 0.067 |

Mannequin `H = 6.865` studs; Classic male `H = 5.159` studs.

Mannequin part extents as fractions of H. Mesh bounding boxes overlap slightly, so these are not a partition:

| Part | Range (y/H) |
|---|---|
| Head | 0.844–1.000. The bottom of the head mesh is the neck joint, so the head mesh includes the neck. |
| UpperTorso | 0.569–0.860 |
| LowerTorso | 0.473–0.588 |
| UpperArm | 0.651–0.827 |
| LowerArm | 0.553–0.685 |
| Hand | 0.450–0.568 |
| UpperLeg | 0.267–0.546 |
| LowerLeg | 0.075–0.298 |
| Foot | 0.000–0.081 |

Useful ratios (Mannequin):
- **Elbow** is 0.50 of the way from shoulder to wrist (segment lengths 0.979 and 0.969).
- **Knee** is 0.486 of the way from hip to ankle.
- **Hand**: wrist to the bottom of the hand is about 0.10·H.
- **Hip spread** (L/R hip joint distance) = 0.812 studs ≈ 0.118·H.
- **Shoulder spread** = 1.198 studs ≈ 0.175·H.

**[RECOMMEND] Cut rules that match these conventions.** Each cut is a plane through the joint, oriented by the limb. Section 4.4 describes how to apply it to the mesh.

| Cut | Plane |
|---|---|
| Neck | Horizontal plane at the base of the neck, where the cross-section starts widening into the shoulders/trapezius. The head part keeps the neck, as in the Mannequin (head bottom = neck joint). |
| Waist | Horizontal plane through the waist joint, about 27% of the way from the hip-joint height to the neck-joint height (Mannequin 0.602 vs hip 0.511 and neck 0.844). Usually just above the iliac crest, at the narrowest torso slice between crotch and chest. |
| Shoulder | Through the shoulder joint, normal along the upper-arm axis, clipped to the arm "tube". This is the armpit boundary: the arm keeps the deltoid cap. |
| Elbow / knee | Through the joint, normal = normalized bisector of the parent and child bone directions. |
| Wrist / ankle | Through the joint, normal along the forearm/shin axis. The ankle plane is roughly horizontal, just above the malleoli (Mannequin ankle at 0.06·H; the foot part spans 0–0.081·H). |
| Hip | Through the hip joint, normal along the thigh axis, clipped to the leg tube. Below the crotch the legs are already separate; the cut defines the upper-thigh / pelvis boundary, an oblique "panty line" from the crotch to above the hip joint laterally. |

### 1.3 Attachments per part

**[REPO + DOC]** Rig attachments (one per joint end). Values are the real reference data in `REPO: rig/bundle/tables.rs`. The repo's builder makes them with the names `<Joint>RigAttachment` (`build.rs::rig_attachments`).

| Part | Rig attachments |
|---|---|
| HumanoidRootPart | `RootRigAttachment` |
| LowerTorso | `RootRigAttachment`, `WaistRigAttachment`, `LeftHipRigAttachment`, `RightHipRigAttachment` |
| UpperTorso | `WaistRigAttachment`, `NeckRigAttachment`, `LeftShoulderRigAttachment`, `RightShoulderRigAttachment` |
| Head | `NeckRigAttachment` |
| L/R UpperArm | `<S>ShoulderRigAttachment`, `<S>ElbowRigAttachment` |
| L/R LowerArm | `<S>ElbowRigAttachment`, `<S>WristRigAttachment` |
| L/R Hand | `<S>WristRigAttachment` |
| L/R UpperLeg | `<S>HipRigAttachment`, `<S>KneeRigAttachment` |
| L/R LowerLeg | `<S>KneeRigAttachment`, `<S>AnkleRigAttachment` |
| L/R Foot | `<S>AnkleRigAttachment` |

- **[DOC]** "Rig attachments cannot be rotated. Please set the orientation to (0,0,0)" (`CD:marketplace/validation-system.md` ~l.174). They carry position only.
- **[DOC]** `Humanoid:BuildRigFromAttachments()` (`API: Humanoid.yaml`): "starting from the humanoid's RootPart … collects all Attachments … whose name ends with RigAttachment … searches for a matching attachment in the character that shares the same name … a Motor6D joint is generated based on the parts associated with the two attachments and the CFrame of the attachments. … also scales the character and sets body colors."
  - **[INFERRED]** So the joint name is the attachment name minus `RigAttachment`. This is how the repo names them (`r15.rs`).

**[DOC] Accessory attachments.** Importer names use an `_Att` suffix; Studio names add the word `Attachment`. Placement rules (`CD:.../specifications.md` "Attachments"):

| Part | Attachment | Placement rule |
|---|---|---|
| Head | `FaceCenter_Att` | anywhere in the bounding box |
| Head | `FaceFront_Att` | front half |
| Head | `Hat_Att`, `Hair_Att` | top half |
| UpperTorso | `LeftCollar_Att`, `RightCollar_Att` | left/right top half |
| UpperTorso | `Neck_Att` | top half |
| UpperTorso | `BodyBack_Att` | back half |
| UpperTorso | `BodyFront_Att` | front half |
| LowerTorso | `Root_Att` | must be at `(0,0,0)` |
| LowerTorso | `WaistFront_Att`, `WaistBack_Att` | front / back half |
| LowerTorso | `WaistCenter_Att` | anywhere |
| L/R UpperArm | `<S>Shoulder_Att` | top half |
| L/R Hand | `<S>Grip_Att` | anywhere. Oriented **perpendicular to the lower-arm bone**; for an I-pose, rotation `(90,0,0)`, pointing forward. |
| L/R Foot | `<S>Foot_Att` | anywhere |

- Rule of thumb: "position them so they overlap halfway with the character model's mesh part".
- **[DOC]** "Each avatar character must include their associated 19 attachment points" (`CD:avatar/character-bodies/index.md`).
- **[REPO]** The instance names in Studio are:
  - Head: `HatAttachment`, `HairAttachment`, `FaceFrontAttachment`, `FaceCenterAttachment`
  - UpperTorso: `NeckAttachment`, `BodyFrontAttachment`, `BodyBackAttachment`, `LeftCollarAttachment`, `RightCollarAttachment`
  - LowerTorso: `WaistCenterAttachment`, `WaistFrontAttachment`, `WaistBackAttachment`
  - Arms, hands, feet: `<S>ShoulderAttachment`, `<S>GripAttachment`, `<S>FootAttachment`
  - HumanoidRootPart: `RootAttachment`

  The repo's `layout.rs` has size-relative fallback formulas for these. The Mannequin values, in part space at InitialSize, are in `tables.rs`.
- **[REPO]** On MeshParts, the repo also adds a `Vector3Value OriginalPosition` under every attachment, and `OriginalSize` (Vector3Value) plus `AvatarPartScaleType` (StringValue: `ProportionsNormal` / `Classic`) under every part, mirroring the reference models (`build.rs::attachment`, `dress_mesh`). Body scaling uses these. **[INFERRED]** Our generated parts should carry them too.
- **[DOC] Orientation conflict.**
  - `CD:.../specifications.md` (Geometry): "Characters must face positive Z and stand up in positive Y". That describes the DCC/FBX export.
  - Auto Setup: "The character body's front should face the negative Z axis" (`CD:avatar-setup/auto-setup-requirements.md` item 7). Also "Manually Align Front … aligns it with the negative Z-axis in world space" (`CDH: aa75c2ac:content/en-us/avatar-setup/auto-setup.md`).
  - **[REPO/EMP]** In Roblox instance space the front is **−Z**: `FaceFrontAttachment z = −0.6` and `BodyFrontAttachment z < 0` in the reference tables, and `CFrame.LookVector` is −Z. **Our generated rig must face −Z.**

### 1.4 Joints

- **[REPO]** Joint list (`r15.rs`), as `Name: Part0 → Part1`, 15 joints:
  - `Root`: HumanoidRootPart → LowerTorso
  - `Waist`: LowerTorso → UpperTorso
  - `Neck`: UpperTorso → Head
  - Per side `S`:
    - `SShoulder`: UpperTorso → SUpperArm
    - `SElbow`: SUpperArm → SLowerArm
    - `SWrist`: SLowerArm → SHand
    - `SHip`: LowerTorso → SUpperLeg
    - `SKnee`: SUpperLeg → SLowerLeg
    - `SAnkle`: SLowerLeg → SFoot
- **[REPO]** The joint instance is parented to **Part1** on R15 (to Part0 on R6) (`layout.rs` `JointSpec.host`).
- **[REPO/DOC] C0 and C1:**
  - `C0 = Part0.<Joint>RigAttachment.CFrame` and `C1 = Part1.<Joint>RigAttachment.CFrame`.
  - The rest relation is `Part1.CFrame = Part0.CFrame · C0 · Transform · C1⁻¹` (`REPO: rig/cframe.rs::joined`; `animation-research.md` §7).
  - For AnimationConstraint, `C0, C1, Part0, Part1` "map to `Attachment0.CFrame`, `Attachment1.CFrame`, `Attachment0.Parent`, `Attachment1.Parent`" (`API: AnimationConstraint.yaml`).
- **[DOC] Motor6D vs AnimationConstraint.**
  - AnimationConstraint is "the default joint type for R15 avatar rigs". With `StarterPlayer.AvatarJointUpgrade` (default for new experiences), player characters spawn with AnimationConstraints instead of Motor6Ds (`API: Motor6D.yaml`, `AnimationConstraint.yaml`).
  - The Importer has a **Constraint Type → Joint Upgrade** option (`CD:avatar/character-bodies/import.md`).
  - **[REPO]** The repo already builds both styles, `JointStyle::Motor6D` and `JointStyle::AnimationConstraint`. The upgraded style adds a `<Joint>BallSocket` `BallSocketConstraint` and `NoCollisionConstraint`s (`build.rs::constraints`, `no_collisions`).
- **[DUMP]** Both `Motor6D` and `AnimationConstraint` have a hidden `EnableSkinning: bool = true`. **[INFERRED]** This is the switch that lets a part joint drive skinned-mesh bones of the same name (see 2.4).

### 1.5 Humanoid

- **[DOC]** `RigType` must be `R15` (Enum value 1). If it is wrong, the humanoid misbehaves; for example, an R15 humanoid set to R6 dies (`API: Humanoid.yaml`).
- **[DOC] HipHeight (R15):** `Height = 0.5·RootPart.Size.Y + HipHeight`, the height of the RootPart centre above the ground. "The height of the legs is not used" (`API: Humanoid.yaml`).
  - **[REPO]** So `HipHeight = (HRP centre y − lowest foot y) − HRP.Size.Y/2` (`r15.rs`: `rig.hip_height = -rig.ground() - root/2`).
  - **[EMP]** This gives 3.508 for the Mannequin at InitialSize and 2.008 for Classic.
- **[RECOMMEND] HRP placement for a generated rig:**
  - HRP centre x,z = midpoint of the hip joints. This matches the Mannequin, where HRP and LowerTorso share x and z ≈ 0.
  - HRP y = Root joint y + 0.849·(H/6.865), or Root joint + 0.5 stud for the Classic ratio. More simply, put `RootRigAttachment` on HRP at `(0, −HRP.Size.Y/2 + ε, 0)` and on LowerTorso at the Root joint. The Root joint position should equal the LowerTorso pivot (bone at 0,0,0 in the DCC rule).
  - Keep `Size = (2,2,1)`, `Transparency = 1`, `CanCollide` per the reference models.
  - Then compute HipHeight with the formula above.
- **[REPO]** The repo also adds:
  - `Animator`
  - `HumanoidDescription` with 15 `BodyPartDescription`s
  - scale `NumberValue`s (`BodyTypeScale`, `BodyProportionScale`, `BodyWidthScale`, `BodyHeightScale`, `BodyDepthScale`, `HeadScale`)
  - `InternalBodyScale` / `InternalHeadScale`
  - the `Animate` script
  - `BodyColors`

  (`build.rs::humanoid`, `description`). A generated rig should go through the same builder; see §6.
- **[DOC]** `AutomaticScalingEnabled` (default true) rescales parts and accessories from the scale values (`API: Humanoid.yaml`). **[INFERRED]** For a custom body, set `AvatarPartScaleType` / `OriginalSize` consistently, or disable automatic scaling, so the engine does not rescale our parts on spawn.

### 1.6 Cages

- **[DOC]** "Character bodies only require an outer cage, but layered accessories require both an inner and outer cage" (`CD:.../specifications.md` "Outer cages").
  - Outer cages are named `<Part>_OuterCage` and become a `WrapTarget` on each part (`CD:art/modeling/specifications.md` "Inner and outer cages"; `CD:studio/importer.md` "Uses Cage").
  - The cage must be a Roblox template cage with unchanged vertices and UVs.
- **[DOC]** Cages are needed for **layered clothing** and **Marketplace validation** ("All body parts must include cages for full-body validation", `CD:marketplace/validation-system.md`). They are not needed to animate.
  - "Character models must include an outer cage in order to properly equip layered accessories" (`CD:art/modeling/specifications.md`).
  - Non-R15 or Humanoid-less targets need a `Weld` on the layer (`CD:characters/appearance.md` "Layered accessories on non-R15").
- **[INFERRED] A rig without cages works in Studio and in game:** it animates, moves and wears rigid accessories, which use attachments only. Layered clothing will not fit it, and it cannot pass Marketplace validation.
- **[RECOMMEND] v1:** no cages; optionally reuse the stock Mannequin cage ids. **v2:** fit template cages by projecting the template cage vertices onto our mesh. Out of scope here.

---

## 2. Skinned meshes (option B) and how Roblox consumes them

### 2.1 Importer requirements for a skinned FBX/glTF

| Item | Value | Source |
|---|---|---|
| Max influences | **4** per vertex | **[DOC]** `CD:avatar/character-bodies/specifications.md` "Skinning"; `CD:art/modeling/specifications.md`; `skin-a-humanoid-model.md` ("Studio doesn't support more than 4 bones") |
| Root influences | none on `Root` / `HumanoidRootNode`; dropped on import | **[DOC]** `rig-a-humanoid-model.md`, `skin-a-humanoid-model.md` |
| Bone transforms | frozen; scale 1,1,1; rotation 0,0,0; root at 0,0,0 | **[DOC]** `CD:art/modeling/specifications.md` "Rigging and skinning" |
| LowerTorso / Root position | 0,0,0 | **[DOC]** `CD:.../specifications.md` "Standard R15 rigs" |
| Bind pose | I-, A- or T-pose. Upper-arm bones may carry rotation to achieve it. | **[DOC]** same |
| Leaf bones | Blender "Add Leaf Bones" **off**; FBX Unit Scale; Maya Up Axis Y | **[DOC]** `CD:art/modeling/export-requirements.md` |
| Orientation | face +Z, up +Y in the DCC export; Auto Setup input faces −Z (see 1.3) | **[DOC]** |
| Zero-influence bones | removed unless "Keep Zero Influence Bones" is set | **[DOC]** `CD:studio/importer.md` "File general" |
| Rig Type | R15 / Custom / No Rig, auto-detected. "Custom Humanoid" for advanced R15. | **[DOC]** `CD:studio/importer.md`; `CD:avatar/character-bodies/import.md` |
| Rig Scale | Default (Classic) / Rthro (Normal) / Rthro Narrow (Slender) | **[DOC]** same |
| Units | "Scale Unit … By default … Studs" (Studio importer) | **[DOC]** `CD:studio/importer.md` |
| Bone count | standard R15 = 16 bones (15 + Root, plus HumanoidRootNode); advanced R15 adds up to **37** optional bones (Spine, Chest, Head→HeadBase, L/R Clavicle, 3×5 fingers per hand, L/R ToeBase). Extra bones beyond the schema are a validation error for bodies ("Body part is skinned to a virtual joint which is not part of the R15plus schema"). | **[DOC]** `CD:.../specifications.md` "Advanced R15 rigs"; `CD:marketplace/validation-system.md` ~l.209 |
| Facial rig | `RootFaceJoint` (usually `DynamicHead`) child of `Head`; no bone limit; 17 required FACS poses; neutral at frame 0 | **[DOC]** `CD:avatar-setup/auto-setup-requirements.md` "Rig requirements" |
| Skin validity | "Skin weight count must match vertex count"; bodies and layered accessories must include skinning; rigid accessories must not | **[DOC]** `CD:marketplace/validation-system.md` |
| Bone placement | a Bone must lie within its MeshPart's half-extents times a tolerance; a child bone must not be too close to its parent; `Bone.Transform` must be identity on upload | **[DOC]** `CD:marketplace/validation-system.md` ~l.214–226 |

Points that are still open:

- **UNVERIFIED** The importer's unit conversion for glTF. The glTF spec uses metres. The repo's freeze code comments: "the importer may rescale (glTF is metres, Roblox studs)" and it checks proportions after upload (`REPO: rbx_studio/src/freeze/mesh.rs` header).
  - **[RECOMMEND]** Read `InitialSize`/`Size` back from the uploaded model and correct it, as `freeze` already does.
- **UNVERIFIED** Whether Open Cloud model upload, which takes `.fbx/.gltf/.glb/.rbxm/.rbxmx` and creates a `Model` of `MeshPart`s uploaded as a package (`CD:cloud/guides/usage-assets.md`), auto-detects an R15 rig the way the Studio importer does. No import settings can be passed. **This has to be tested with a real key.**
- **[DOC]** Open Cloud `Mesh` asset upload (`model/x-file-mesh-data`) "Only content downloaded from Asset delivery API is accepted" (`CD:cloud/guides/usage-assets.md`). **We therefore cannot upload our own `.mesh` file.** Geometry and skinning must go up as FBX or glTF inside a Model.

### 2.2 What the importer produces

- **[DOC]** "In Studio, each joint of the rigging armature is represented by `Bone` objects that connect the character's body part MeshParts" (`CD:avatar/character-bodies/index.md`). "Roblox saves this influence assignment data to the MeshPart asset data" (`CD:art/modeling/rigging.md`). Bones can be parented to Bones or to parts. A child bone's world position is relative to its parent bone (`API: Bone.yaml`).
- **[REPO, from the real Mannequin reference model]** Each R15 part is a `MeshPart`, and **no Bone instances exist for the 15 core joints**. The joints are Motor6D/AnimationConstraint between parts.
  - Only the 37 optional bones exist as hidden `Bone`s. Examples:
    - `UpperTorso/Spine/Chest/{Left,Right}Clavicle`
    - `RightHand/RightHandIndex1/RightHandIndex2/…`
  - Each bone's `CFrame` is an offset from its parent (part or bone) (`REPO: rig/bones.rs`; `Visible = false`).
  - **[INFERRED]** The importer converts the 15 core bones to part joints plus RigAttachments, and keeps only the non-part bones as `Bone` instances.
- **[DUMP]** `MeshPart.HasSkinnedMesh` (hidden) is set by the engine when the mesh has joints (`API: MeshPart.yaml`).

### 2.3 Animating Bones

- **[DOC]** `Bone.Transform` (CFrame, NotReplicated): "Roblox uses the offset of the bones from the default pose to drive an animation … you can use `Bone.Transform` in the same way as `Motor6D.Transform`" (`API: Bone.yaml`). `Bone` inherits `Attachment`, so `Bone.CFrame` is its rest offset.
- **[DOC/REPO]** Keyframe `Pose`s and CurveAnimation folders are matched by name and can drive Bones as well as Motor6Ds (`animation-research.md` §4, from DUMP; Animation Editor "Select a Bone or MeshPart").
  - **[INFERRED]** A KeyframeSequence Pose named after a Bone, nested under the Pose of the part (or Bone) it hangs from, drives that Bone's `Transform`.

### 2.4 How skinned R15 and Motor6D coexist

**[EMP]** I parsed a real Roblox dynamic-head mesh: `version 5.00`, asset `13674780763`, from the krakow10 sample set (`scratchpad/skin45.py`). Findings:

- **Bone table.** It holds **38 bones**:
  - The full ancestor chain: `Root`, `HumanoidRootNode`, `LowerTorso`, `UpperTorso`, `Head`.
  - Then `DynamicHead` and 32 face bones.
- **Bone frames.** Bone CFrames are bind poses in **mesh space**, the part's own space, all with identity rotation.
  - Bone `Head` is at `(0, −0.586, 0)`. That is exactly the Classic head's `NeckRigAttachment` (`tables.rs`).
  - `UpperTorso` is at y −2.197 and `LowerTorso`/`Root` at y −2.537, in the head mesh's space.
  - **[INFERRED]** So:
    1. A part-named bone's position equals the joint/rig-attachment position.
    2. The importer derives RigAttachments from bone heads.
    3. A part's mesh carries the bones of other parts it is weighted to, positioned where those joints would be in the bind pose.
- **Weights.** Weight bytes sum to exactly 255 for every vertex.
- **Subsets.** The mesh is split into 6 subsets, 2 per LOD (LOD count 4 → 3 levels). Each subset uses ≤ 26 bones (20, 6, 20, 6, 17, 6).
- **[RE] Runtime binding.** Renderers resolve a mesh bone by **name**: they look for a sibling instance (part or Bone) with that name in the character model and apply its current joint transform. Code:
  - RoAvatar-Renderer `src/code/render/subDescs/local-skeletonDesc.ts` `getPartEquivalent` / `updateBoneMatrix`
  - MaximumADHD `RobloxMesh` creates Bones from the table
- **[INFERRED] What "skinned R15" means.** Each of the 15 MeshParts has its own mesh, and its vertices are weighted to part-named bones, typically:
  - its own part,
  - its parent part (for example `Head` + `UpperTorso` near the neck, the case `skin-a-humanoid-model.md` describes),
  - and optional R15+ bones.

  The Motor6D/AnimationConstraint `Transform` moves the parts. Because `EnableSkinning` is on, each joint's transform also drives the matching named bone in every skinned mesh of the character. So the neck vertices of the Head mesh follow `UpperTorso` while the rest follows `Head`, and the seams blend.

**Consequence for the design.** Roblox's real avatar format is **A and B combined**:
- 15 separate, capped MeshParts joined by Motor6D/AnimationConstraint (A),
- each skinned with ≤ 4 influences to part-named bones (B), so joints bend smoothly instead of hinging.

Roblox's own Avatar Setup does this: it "Adds a R15 rigging armature", "Skinning — Adds weights and influences", and "Partitioning — Separates the body mesh into the appropriate 15 body parts" (`CD:avatar-setup/index.md`).

A single unsplit skinned mesh (an "R1" mesh in `CD:art/modeling/rigging.md` terms) is also legal, but:
- It is not an avatar body. Validation, `HumanoidDescription` and body-part swapping all need 15 parts.
- A Humanoid needs real parts named `HumanoidRootPart`, `Head` and so on. **[INFERRED]** A single skinned MeshPart plus `Bone`s works for NPCs with the Animator playing on Bones. `Humanoid` R15 locomotion still needs the part rig.

**[RECOMMEND]** Treat B as "A plus weights": always produce the 15-part split, then add per-vertex weights to the parent and child joints near each cut. The pure single-mesh variant is just "skip the split, keep all bones as `Bone`".

---

## 3. Mesh binary skinning layouts (v4/v5 and v6/v7 `SKINNING`)

All values are little-endian.

### 3.1 v4.00 / v4.01 / v5.00

**[RE]** Sources: krakow10 `src/mesh/v4.rs`, `v5.rs`; MaximumADHD `RobloxMesh/init.luau` lines 348–454, 673–760; Rbx2Source `Mesh.cs` 140–330. **[EMP]** Confirmed on asset 13674780763.

```
"version 4.00\n" | "version 4.01\n" | "version 5.00\n"
Header (sizeof_header u16 = 24 for v4, 32 for v5):
  u16 sizeof_header
  u16 lod_type            // 0 None,1 Unknown,2 RbxSimplifier,3 ZeuxMeshOptimizer,(4 seen)
  u32 num_verts
  u32 num_faces
  u16 num_lods
  u16 num_bones
  u32 sizeof_bone_names
  u16 num_subsets
  u8  num_high_quality_lods
  u8  unused
  // v5 only:
  u32 facs_format         // 1
  u32 sizeof_facs_data
Vertex[num_verts]   40 bytes: f32 px,py,pz, f32 nx,ny,nz, f32 u,v, i8 tx,ty,tz,ts, u8 r,g,b,a
Envelope[num_verts] (present iff num_bones > 0), 8 bytes:
  u8 bone_slot[4]         // index into the vertex's SUBSET bone table, not a global bone id
  u8 weight[4]            // 0..255, sum == 255 [EMP]
Face[num_faces]     u32 a,b,c
u32 lod_offsets[num_lods]   // face offsets; last == num_faces
Bone[num_bones]     60 bytes:
  u32 name_offset         // byte offset into the name table
  u16 parent              // 0xFFFF = none
  u16 lod_parent          // [EMP] equals parent except root (0xFFFF)
  f32 culling             // radius-like; 0 for structural bones [EMP]
  f32 r00,r01,r02, r10,r11,r12, r20,r21,r22   // rotation, row-major
  f32 x,y,z               // position
  // CFrame is the bone's bind pose in MESH (part-local) space, not parent-relative
u8  bone_names[sizeof_bone_names]   // NUL-terminated strings
Subset[num_subsets] 72 bytes:
  u32 faces_begin, u32 faces_length
  u32 verts_begin, u32 verts_length
  u32 num_bone_indices    // <= 26
  u16 bone_indices[26]    // global bone ids, 0xFFFF padding
// v5 only: u8 facs_data[sizeof_facs_data] (layout in 3.3)
```

Details for the bone records and the skin:
- **Matrix order.**
  - MaximumADHD reads three row vectors then the position into `CFrame.new(x,y,z, R00..R22)`.
  - BTRoblox stores `cframe[3..12]` = 9 rotation floats and `cframe[0..3]` = position.
  - Rbx2Source writes index `(m+3)%12`.
  - **Rbx2Source disagreement:** it names the two u16s `Id` and `ParentId` (the second u16 as parent). krakow10, MaximumADHD and BTRoblox all read the **first** u16 as parent. **[EMP]** The sample agrees with "first = parent": `L_eyeD → L_eye`, and the second u16 has the same value.
- **Resolving a vertex weight** (MaximumADHD `readSubsets`; BTRoblox):
  1. Find the subset `s` whose `[verts_begin, verts_begin+verts_length)` contains the vertex.
  2. `bone = bones[ subsets[s].bone_indices[ envelope.bone_slot[k] ] ]`, with weight `weight[k]/255`.
  3. Slot entries pointing at 0xFFFF are empty.
- **Subsets** partition faces and vertices contiguously. **[EMP]** There is one group of subsets per LOD.
- **Writing.** A writer must split any face range that uses more than 26 bones into several subsets and duplicate vertices at subset boundaries. **[INFERRED]** That follows from the 26-slot limit. A 15-bone R15 body part never hits it.

### 3.2 v6.00 / v7.00 chunked

**[RE]** BTRoblox `MeshParser.js` l.488–570; MaximumADHD l.840–915. **[REPO]** Chunk framing is in `REPO: rbx_mesh/src/chunked.rs`.

```
"version 6.00\n" | "version 7.00\n", then chunks to EOF:
  char[8] type ("COREMESH","LODS\0\0\0\0","SKINNING","FACS\0\0\0\0","HSRAVIS\0")
  u32 chunk_version
  u32 size
  u8  payload[size]

SKINNING, chunk_version 1:
  u32 num_skinnings                 // == num_verts
  Envelope[num_skinnings]           // 8 bytes, same as v4
  u32 num_bones
  Bone[num_bones]                   // 60 bytes, same as v4
  u32 sizeof_bone_names
  u8  bone_names[...]               // name_offset relative to the start of this table
  u32 num_subsets
  Subset[num_subsets]               // 72 bytes, same as v4
FACS, chunk_version 1: u32 sizeof_facs + the same FACS block as v5 (MaximumADHD l.894–896)
```

The counts are u32 here, where v4 uses u16.

### 3.3 FACS block (for completeness)

**[RE]** MaximumADHD `readFACS` l.478–560:

```
u32 sizeof_face_bone_names
u32 sizeof_face_control_names
u64 sizeof_quantized_transforms
u32 sizeof_two_pose_correctives     // /4 = count
u32 sizeof_three_pose_correctives   // /6 = count
char[] face_bone_names
char[] face_control_names
6 × QuantizedMatrix (px,py,pz,rx,ry,rz):
  u16 format
  u32 rows
  u32 cols
  format 1: f32[rows*cols]
  format 2: f32 min, f32 max, u16[rows*cols]
then u16 pairs and u16 triples of corrective indices
```

Face bones are "virtual" bones bound to `FaceControls` (`API: EditableMesh.yaml` AddBone). The auto-rigger does not need this.

### 3.4 Compared with this repo's `rbx_mesh`

`REPO: crates/rbx_mesh/src/v4.rs`:
- Header field widths match 3.1 exactly (24/32-byte headers, u16 bone/subset counts).
- `SKIN_ENVELOPE_SIZE = 8`, `BONE_SIZE = 60` and `SUBSET_SIZE = 72` all match. Its comment about the "70 bytes" misread is correct, and its TODO ("index bytes are subset-local slots … resolve through `subsets[s].bone_indices[slot]`") is confirmed.
- What it lacks:
  - Envelopes, bones, names and subsets are **skipped, not exposed**.
  - FACS is skipped.
  - The tangent is dropped.
  - There is **no writer**, apart from a test-only Draco encoder in `draco.rs`.

`REPO: chunked.rs`:
- Reads `COREMESH` v1 (raw records) and v2 (Draco) and `LODS` v1.
- `SKINNING`, `FACS` and `HSRAVIS` are skipped by size.

To expose skinning, add to `Mesh`:

```
skin: Option<Skin { bones: Vec<Bone{name,parent:Option<u16>,lod_parent,culling,cframe:[f32;12]}>,
                    subsets: Vec<Subset{faces:Range<u32>,verts:Range<u32>,bone_ids:Vec<u16>}>,
                    joints: Vec<[u16;4]> /*resolved global ids*/, weights: Vec<[u8;4]> }>
```

Parse it with the layouts above. Only the v4/v5 tail and a new `SKINNING` arm in `chunked.rs` are needed. **We do not need this to upload** (§2.1: we upload glTF/FBX). It is needed to:
- preview Roblox's skinned bodies deforming in the viewer, and
- write local preview meshes with skinning (§6.3).

---

## 4. Auto-rigging algorithms without ML

Assume up to 20k triangles (≈10k vertices after welding), CPU only, Rust.

### 4.1 Pre-processing: up axis, facing, scale

1. **Weld.** Merge vertices by position (ε ≈ 1e-5·diag) into a "topology mesh", keeping a map back to the render vertices. UV and normal seams otherwise split the graph.
2. **Up axis.**
   - glTF is always +Y up (glTF 2.0 spec). FBX declares `GlobalSettings.UpAxis/UpAxisSign`; the repo's `rbx_animation` FBX reader does not read it yet.
   - Verify with a heuristic: the up axis is the bbox axis along which the bottom 5% of vertices form **two** separated clusters (feet) and the top 5% form **one** (head).
   - Do not rely on the longest bbox axis or PCA: in a T-pose, arm span ≈ height.
3. **Facing.** Roblox wants −Z front (§1.3). After the up axis is fixed, candidates are ±X and ±Z.
   - Pick the horizontal axis with the smallest bbox extent as depth, since left-right width is larger in A/T pose.
   - Choose the sign by **toe direction**: the bottom 4% slice of each foot extends further forward than the ankle column. The foot's centroid is offset forward of the leg centroid one slice higher (Mannequin foot centre z −0.221 vs ankle z +0.084, so forward = −z).
   - Secondary checks: nose/face protrusion at head height; chest vs back curvature.
   - Auto Setup has the same failure mode and offers a "Manually Align Front" override (`CD:avatar-setup/index.md`). **We must offer it too**: a "Rotate 90°/flip" control.
4. **Scale.** Normalize internally to H = 1. The output scale is the user's choice:
   - keep the file's units (importer "Scale Unit"), or
   - fit a Roblox body-scale height. The bounds are per body scale (`CD:.../specifications.md` "Body scale"), for example Normal total Y between 3.6 and 9.5 studs; Classic max 9.1.
   - Default **[RECOMMEND]**: scale so H ≈ 5.2 studs (Classic) or 6.9 (Rthro Normal), matching the rig scale chosen.

### 4.2 Landmark detection

**[RECOMMEND]** Use a voxel volume plus slices, which is robust to non-watertight and multi-part meshes.

1. **Voxelize** at 128 cells on the longest axis (≈ 7.8 mm at H = 1 m).
   - Mark surface voxels with a triangle/box SAT overlap test.
   - Flood-fill the exterior from the grid border with 6-connectivity. Solid = not exterior. This closes holes smaller than one voxel.
   - This step is the Dionne & de Lasa idea (4.4.3).
   - Cost: ≤ 2.1 M voxels; the flood fill is O(voxels), about 10 ms.
2. **Distance transform** inside the solid: exact Euclidean via Felzenszwalb–Huttenlocher, separable and O(n), giving the local thickness r(x). Optionally take the medial ridge (local maxima of r) as a skeleton proxy.
3. **Extremities.**
   - Geodesic BFS through solid voxels (26-connectivity) from the voxel with maximum r (chest/pelvis core).
   - The 5 largest local maxima of geodesic distance that are ≥ 0.2·H apart are the head top, two hands and two feet.
   - Classify: the two lowest are feet, the highest near the centre axis is the head, the remaining two are hands, and the sign of x relative to the facing gives left/right.
   - **Left/right convention:** the Mannequin `LeftUpperArm` is at x = −0.897 in rig space, with front = −Z (`REPO: tables.rs`, `layout.py` output). So the character's **Left is −X** in Roblox space. This agrees with geometry: facing −Z with +Y up, the character's right is (−Z)×(+Y) = +X.
   - Failure cases:
     - Fewer than 5 maxima: hands touching hips in an I-pose, or a long skirt. Fall back to the slice method below and ask the user to place markers.
     - Extra maxima: tail, wings, hair spikes. Keep the 5 that best match the template distances.
4. **Limb paths.** For each extremity, take the geodesic shortest path back to the core, pushed toward the medial axis by weighting edges with 1/r². Resample each path to arc length.
5. **Cross-section profile along each path.**
   - At each sample, intersect the **solid voxels** with the plane ⟂ to the path tangent.
   - Measure the area of the connected component containing the path point, and its equivalent diameter.
   - Equivalently, slice the triangle mesh with that plane and keep the closed loop around the path point (mesh slicing: O(F) per plane; 100 planes × 20k triangles is fine).
6. **Joints from the profiles:**
   - **Armpit / crotch / neck base** (torso boundary). Walk from the extremity inward; the boundary is the first sample where the section area jumps more than about 2× its running median. The section then contains torso.
     - Crotch alternative using horizontal slices from the bottom: the lowest height where the two leg loops merge into one.
     - Armpit for an A-pose: the highest horizontal slice still showing 3 components (torso and two arms).
     - Armpit for a T-pose: vertical slices along x from the hand inward; the innermost x where the arm loop is still separate.
   - **Wrist / ankle:** the minimum section diameter in the distal 20–35% of the arm or leg path, excluding the hand/foot. For the ankle, also clamp to y ≤ 0.10·H.
   - **Elbow / knee:** 0.50 / 0.49 of the way along the path between shoulder→wrist / hip→ankle (Mannequin ratios, §1.2). Optional refinement: the bend angle of the path, if the pose is bent, or the local minimum of section width near the midpoint.
   - **Shoulder joint:** at the armpit boundary plane, take the loop centroid, move it up by 0.35·(loop diameter) toward the top of the arm, and pull it medially to x = ±0.087·H (Mannequin shoulder spread). Clamp it between those two estimates.
   - **Hip joint:** at the crotch height + 0.04·H, take the centroid of each leg loop, then shift laterally to ±0.059·H. The Mannequin hip spread is 0.118·H and the hip joint is 0.511·H vs a crotch of about 0.47·H.
   - **Neck joint:** on the head-top → chest path, the neck is the minimum-section run between 0.78·H and 0.92·H. The **joint and cut go at the lower end of that run**, where the section starts growing into the shoulders. This matches the Mannequin, whose Head mesh includes the neck. If no clear neck exists (a "No neck" failure in Auto Setup), use 0.82·H on the centre axis.
   - **Root / Waist:**
     - Root (LowerTorso pivot) = midpoint of the hips, raised by 0.022·H (Mannequin 0.533 vs 0.511).
     - Waist = hip y + 0.27·(neck y − hip y), on the centre line. Snap it to the narrowest torso horizontal slice in [hip + 0.15, hip + 0.40]·(neck − hip) when one exists.
   - Centre-line joints use x = 0 after symmetry alignment. Shift x to the measured centroid when the mesh is asymmetric. Keep the Mannequin z offsets as a fallback (for example neck z ≈ torso centroid z).
7. **Symmetry.** By default, mirror the left/right estimates about the sagittal plane and average them, as Auto Setup asks for symmetric input (`auto-setup-requirements.md` item 8). Offer a "symmetric markers" toggle.

Prior art:
- Mixamo's auto-rigger asks for user markers at the chin, wrists, elbows, knees and groin, with symmetry, and then rigs and skins automatically. Sources **(secondary)**: [CG Channel 2012](https://www.cgchannel.com/2012/06/mixamo-makes-auto-rigger-free-for-low-poly-meshes), [Overdare manual](https://docs.overdare.com/manual/studio-manual/asset-and-resource-creation/using-mixamo-to-create-studio-animations).
- Cross-section and minimum-circumference landmarking is standard in body-scan anthropometry, for example [US 7561726](https://patents.google.com/patent/US7561726) and Leong et al. 2007, *CAD* 39(7) **(secondary, abstract not read)**.

**[RECOMMEND] UX**, matching Mixamo and Auto Setup:
- Show 15 joint markers (plus chin/head top for orientation) on the mesh in the 3D view, initialized by the steps above.
- The user drags markers. Mirroring is on by default.
- Every drag re-runs the cut and weights; both are fast enough for interactive use (§4.5).

### 4.3 Skeleton embedding (Pinocchio)

Baran & Popović, "Automatic Rigging and Animation of 3D Characters", SIGGRAPH 2007 (ACM TOG 26(3) art. 72). PDF links (not reachable here): https://people.csail.mit.edu/ibaran/papers/2007-SIGGRAPH-Pinocchio.pdf, https://dspace.mit.edu/handle/1721.1/100396.

**[PIN]** Pipeline (`Pinocchio/pinocchioApi.cpp`):

```
prepareMesh → constructDistanceField (adaptive octree) → sampleMedialSurface
→ packSpheres → connectSamples (graph) → computePossibilities → discreteEmbed
→ splitPaths → refineEmbedding (continuous) → Attachment (bone heat)
```

- The template is a normalized 18-joint human (`skeleton.cpp::HumanSkeleton`): shoulders, back, hips, head, then thigh/knee/ankle/foot and shoulder/elbow/hand per side. It declares symmetry pairs, feet and "fat" joints.
- The discrete embedding fits the template graph onto sphere centres with a branch-and-bound search that minimizes a learned weighted sum of penalties (bone length, direction, symmetry, foot lowness and others).
- Refinement then optimizes continuous joint positions.
- The paper reports under 1 minute per character on 2007 hardware (abstract, via search **(secondary)**).

Trade-offs for us:
- It is fully automatic and pose-agnostic.
- It needs a reasonably closed mesh.
- It is complex: the octree distance field, the sphere packing, and a penalty weight vector learned with an SVM.
- Its 18-joint template maps onto R15 only approximately: "back" sits between Waist and Root, and there is no separate neck. It would need re-learning the penalties for R15.

**Verdict.** It is overkill when we already know the topology class (biped in A/T pose) and give the user markers. Use the targeted landmark method of 4.2 and borrow only Pinocchio's attachment step.

### 4.4 Automatic skin weights

#### 4.4.1 Bone heat (Pinocchio)

**[PIN]** Exact code, `Pinocchio/attachment.cpp` l.45–197. Solve per bone *j*:

```
(−L + H) w_j = H p_j
```

- `L` is the **cotangent Laplacian**: off-diagonal −(cot α + cot β), diagonal Σ(cot α + cot β).
- It is scaled by the **inverse one-ring area** `D_i = 1 / Σ|e_ij × e_ik|`. The code builds `A = (sum + H_i/D_i)` on the diagonal, so it solves `A w = H/D · p`.
- `H_ii = c / d_i²` with c = `initialHeatWeight` = 1.0, where d_i is the distance to the **nearest bone segment**.
- `H_ii` is added only for bones that are (a) within 1.0001× of the nearest distance and (b) **visible**: a segment from the vertex to its projection onto the bone stays inside the volume (`VisTester` on the distance field), and the direction lies in the cone of the adjacent face normals (`vectorInCone`, dot > 0.5).
- `p_j,i = 1` if bone j is one of those nearest visible bones.
- One sparse Cholesky factorization `LLT`, then one back-solve per bone.
- Results are clipped to ≤ 1, entries ≤ 1e-8 dropped, and each vertex normalized to sum 1.
- Cost: factorization of an n×n SPD matrix with about 7n non-zeros. For n = 10k this is milliseconds to tens of milliseconds with a fill-reducing ordering; 15 solves cost about the same.
- **Weaknesses:**
  - Needs a manifold, connected mesh. Disconnected components with no visible bone give zero rows, so the system becomes singular for that component.
  - Weights can bleed across touching limbs (hands on thighs, inner thighs) when visibility fails.
  - Unwelded seams break diffusion.

#### 4.4.2 Inverse distance plus Laplacian smoothing (simplest)

- `w_ij ∝ (1/d_ij)^k` with k ≈ 2–4, using Euclidean distance to segment *j*.
- Mask each vertex to its own segment and the adjacent segments (parent and children) to avoid cross-limb bleed. With a known R15 segmentation, the mask is "own part ± its joint neighbours".
- Then run 10–30 iterations of uniform or cotangent Laplacian smoothing on each weight channel, keeping the vertices far from any joint fixed at 1.
- Then prune to 4 influences and normalize.
- O(n·bones + iterations·nnz). Trivial and robust, but with a lower quality ceiling: it is distance-only.

#### 4.4.3 Geodesic voxel binding (Dionne & de Lasa, SCA 2013)

"Geodesic Voxel Binding for Production Character Meshes", the Maya "Geodesic Voxel" bind:
1. Voxelize the mesh and fill the interior, with Maya's resolution parameter defaulting to **256** (power of two) and an optional "validate voxel state".
2. Rasterize the bones into the grid.
3. Compute geodesic distances from each bone **through the solid voxels**.
4. Map distance to weight with a **falloff** in [0,1] (bind stiffness).
5. Limit to `maxInfluences`.

Parameters from Maya `geomBind` docs (`FXTD-ODYSSEY/MayaDoc` `CommandsPython/geomBind.html`): `bindMethod 3 = Geodesic Voxel`, `falloff [0..1]`, `geodesicVoxelParams = [resolution, validate]`, default `256 true`, `maxInfluences`.
- **UNVERIFIED:** the paper's exact kernel. One open implementation (`fernandotonon/QtMeshEditor` `src/GeodesicVoxelBind.h`) uses `w = (1/d)^falloff`, multi-source Dijkstra with 26-connectivity, and keeps the best K = 8 bone/distance pairs per voxel.
- **Strengths:** handles non-manifold, multi-component and self-intersecting meshes (eyes, teeth, separate shoes). Distances go through the interior, so hand-to-thigh bleed is impossible by construction.
- Cost: one Dijkstra over about 10⁶ voxels at 128³ fill, O(V log V), ~100 ms.
- Weights are only as smooth as the voxel resolution, so a light Laplacian smoothing pass afterwards helps.

### 4.5 Recommendations

**(A) Segmentation into 15 parts: plane cuts restricted by geodesic ownership.**
1. Compute each topology vertex's **owner bone**: argmin over the 15 segments of the voxel-geodesic distance from 4.2 (reuse 4.4.3's Dijkstra). This gives a coarse, pose-robust labelling without bleed.
2. For each joint, define the **cut plane** of §1.2: through the joint position, normal = bisector of the bone directions, or limb-axis / horizontal for the torso cuts.
3. For each limb joint, restrict the cut to the **child region**:
   - Vertices with the child bone or its descendants as owner, plus the parent-labelled vertices within a capsule of radius 1.5× the measured limb radius around the child bone.
   - Split triangles straddling the plane inside that region **exactly at the plane**, adding new vertices with interpolated UV, normal and colour.
   - Assign the sides.
   - Outside the region (torso flank under an A-pose armpit), keep the geodesic label boundary and smooth it with 2–3 rounds of majority voting on the face adjacency.
4. **Caps.**
   - Each part's open boundary loops are closed: flat cap = ear clipping on the loop projected to its best-fit plane, or a rounded cap = fan to the centroid pushed inward by 0.3·r.
   - The cap gets its own small UV island, or reuses the nearest boundary UV.
   - Both neighbours get a cap, so parts are watertight as required (§1.1).
   - Flag a part over budget (Auto Setup has the same warning).
5. Each part's mesh is **re-centred on its bbox centre**. Its rig attachments are the joint world positions minus the part's centre, unrotated, since rig attachments must have no rotation (§1.3).
6. Complexity: O(F) per cut plus the Dijkstra. Interactive re-cut on marker drag stays under 50 ms for 20k triangles.
7. Why not a pure plane cut: under the arm in an A-pose, and at the crotch/hip, a plane through the joint also passes through the torso or the other leg. The geodesic ownership limits which triangles a plane may cut.

**(B) Weights: bone heat over the welded mesh, with voxel-geodesic "visibility" and a geodesic-voxel fallback.**
1. Bones = the 15 R15 segments. Each segment is joint → child joint; for end parts (head, hands, feet) it runs from the joint to the extremity landmark. `HumanoidRootNode`/`Root` get **no weights** (§2.1).
2. Replace Pinocchio's ray visibility test with the ownership mask from (A): bone j "sees" vertex i if j is i's owner, or i's owner's parent or child **and** i is within 1.5·r of the joint. This removes the need for a closed volume and prevents cross-limb bleed.
3. Build the cotangent Laplacian on the welded mesh, then `H_ii = 1/d_i²` and `p_ij` as in 4.4.1. Solve the 15 right-hand sides:
   - sparse Cholesky: `faer`'s sparse LLT, or `sprs` + `sprs-ldl`; or
   - Jacobi-preconditioned conjugate gradient (about 100–300 iterations; 15 × 10k-vertex solves take well under 1 s).
4. Components with no seeds (separate eyes, teeth, accessories) take the GVB weights from 4.4.3, or 100% of their owner bone.
5. Post-process:
   - Keep the top 4 influences.
   - Renormalize.
   - **Quantize to u8 so the bytes sum to exactly 255** (largest-remainder rounding), as observed in Roblox meshes (§2.4).
   - Copy weights from welded to render vertices.
6. **For the combined A+B output** (recommended final form): after splitting, each part's vertices keep their weights, restricted to {own bone, parent bone, child bones}. Cap vertices get 100% of their own bone.

**Robustness ranking for user meshes:** GVB/voxel > bone heat > inverse distance. **Smoothness:** bone heat > GVB > inverse distance. The hybrid above takes GVB's locality and heat's smoothness.

---

## 5. Roblox's Avatar (Auto) Setup: behaviour to match

**History**
- Alpha announced March 2024 ([newsroom](https://about.roblox.com/newsroom/2024/03/roblox-introduces-ai-powered-avatar-and-texturing-technologies-to-accelerate-3d-creation) **(secondary)**).
- Open beta about October 2024 ([devforum 2960683](https://devforum.roblox.com/t/avatar-auto-setup-beta-release/2960683) **(secondary)**). Per that summary: it "automatically rig[s], cag[es], segment[s], and skin[s]", is "trained on Roblox-created avatars", and "sometimes … produces artifacts in the geometry".
- It is ML-based. Our version is not, so our behaviour should match the **conventions**, not the internals.
- A later post "Avatar auto setup now supports clothing and accessories" is linked from `CDH: aa75c2ac:content/en-us/avatar-setup/auto-setup.md`.

**Inputs** (`CD:avatar-setup/auto-setup-requirements.md`):
- One or more meshes with no rig. Multiple meshes are "recombined … as a single mesh, then decimate[d]".
- Or 15 meshes named per the convention without a rig. Auto Setup "attempts to place joints according to the user-partitioned body parts".
- Or meshes with a standard/advanced R15 rig. It keeps the rig; a non-conforming rig is replaced with a new one.
- Or a single mesh with body and face rig plus FACS.

**Model requirements and failure modes**
- Required:
  - 5 separate head components: 2 eyes in eyebags, plus upper teeth, lower teeth and tongue in a mouthbag. They must not share vertices with the head.
  - Total ≤ 10,742 triangles.
  - Humanoid shape (2 arms, 2 legs, torso, head).
  - **A-pose or T-pose**; an I-pose may be lower quality; no limb occluding another in front view.
  - Front toward **−Z**.
  - Roughly symmetric about the Y axis.
  - Watertight except eyes and mouth.
  - No accessories (hair, brows, lashes).
  - A **distinct neck**.
  - At least one texture.
- Listed failures: extreme asymmetry, I-pose, non-contiguous mesh, no neck, long neck / out-of-proportion limbs.

**Outputs** (`CD:avatar-setup/index.md`):
- A `Model` with 15 MeshParts plus "other expected avatar components":
  - R15 armature (rigging)
  - skinning weights
  - FACS face rig and animation
  - cages
  - partitioning into 15 parts with caps
  - attachments
- Options:
  - Preset: **Platform Avatar** (Marketplace-strict) or **Development Avatar**.
  - Per-mesh type: Body / Layered / Rigid / Eye Layered.
  - Handle face as 2D decal.
  - Create R15 with optional joints (advanced R15).
  - Manually Align Front.
  - Improve facial caging (~30 s, per `CDH`).
  - Reduce triangle count (auto-decimation; can be disabled only for Development).
- "This can take several minutes."
- Post-edit tools: an **Attachment Tool** (move attachment points with the Move tool) and a **Cage Brush** (symmetric brush on the cage).
- Testing tabs: animations, clothing, accessories, body / skin tone.

**What our auto-rigger should match**
- **[RECOMMEND]** Same accepted poses (A/T; warn on I-pose), the same −Z/+Y output convention, and a front-alignment override.
- The same naming: 15 parts and the 19 attachments with Studio names (§1.3).
- Rig attachments unrotated, at the cut planes.
- Caps on every cut.
- Symmetric by default.
- Budget warnings using the 6-asset table.
- A "Development vs Platform" distinction:
  - Platform needs cages and FACS, which we don't produce, so say so.
  - Development is our target.
- If the input already has 15 `_Geo`-named meshes, skip segmentation and only place joints. If it has an R15 armature, skip both.

---

## 6. What already exists in the repo, and how to wire it

### 6.1 Building blocks

| Module | What it gives the auto-rigger |
|---|---|
| `rbx_studio/src/shell/rig/` | Builds stock R15/R6 rigs. See the four rows below. |
| … `r15.rs` | Part list; 15 joints from `<Joint>RigAttachment` positions scaled by `Size/InitialSize`; `hip_height`. |
| … `layout.rs` | `Layout`/`PartSpec`/`JointSpec`: a rig as plain data; `solve` places parts through joints. |
| … `build.rs` | Instances in reference order: Motor6D (C0/C1) or AnimationConstraint + BallSocket + NoCollision; Humanoid (RigType, HipHeight, scale values, HumanoidDescription); MeshPart dressing (`MeshId`, `InitialSize`, `OriginalSize`, `AvatarPartScaleType`, `WrapTarget`, `SurfaceAppearance`, `FaceControls`). |
| … `bundle.rs` | `Piece` = one body part's mesh + `init` size + rig attachments + extra attachments. `RigOptions.pieces` is explicitly "a player's own packages replace stock entries". **This is the integration point**: the auto-rigger can produce 15 `Piece`s and call `build_rig` unchanged. |
| … `bones.rs` | Adds the 37 optional hidden `Bone`s (Mannequin offsets). A template for "advanced R15" bone placement in our output. |
| `rbx_import` | Reads `.fbx` (fbxcel tree; **triangle counts only**, no geometry), `.gltf/.glb` (`gltf` crate; mesh triangle counts), `.obj` → glTF (`tobj`; positions, normals, UVs, no materials). `prepare()` returns the upload bytes. **Missing:** full geometry and skin readout from FBX. Options: extend the fbxcel walker (Vertices / PolygonVertexIndex / LayerElementUV / Deformer-Cluster) or adopt `ufbx` (C library, already evaluated in `animation-research.md` §7c; local copy `/tmp/claude-0/research/ext/ufbx-rust`). |
| `rbx_studio/src/shell/import3d.rs` | Open Cloud model upload (`rbx_cloud::create_model_asset`), download of the created model, and placement in Workspace. `RBX_STUDIO_IMPORT_MOCK=<rbxm>` substitutes a local model for upload and download. The auto-rigger's "Publish" step should reuse this with its own glTF. |
| `rbx_studio/src/freeze/` | Prior art for "bake geometry → glTF → upload as Model → read back MeshId/InitialSize → verify proportions" (`mesh.rs`; `PROPORTION_TOLERANCE`; `RBX_STUDIO_FREEZE_MOCK`). It refuses skinned meshes (`has_bones`). |
| `rbx_viewer/src/export` | glTF writer (`Export`, `ExportMesh`, `ExportNode`, `gltf_files`): positions, normals, tangents, UVs, maps. **No skin** (no `JOINTS_0`/`WEIGHTS_0`/`skins`/`inverseBindMatrices`). Option B needs these added: node per bone named `Root`, `HumanoidRootNode`, `LowerTorso`, …; meshes named `<Part>_Geo`; one skin. |
| `rbx_animation/src/import/` | FBX and glTF **skeleton + animation** readers (`Skeleton`, `Bone`, rest pose), Horn best-fit world alignment (`skeleton::fit`), retargeting with Mixamo/Blender/Unreal **bone-name aliases** (`retarget.rs::ALIASES`). Reusable for the "input already rigged" path: map an existing armature onto R15 names and skip landmark detection. |
| `rbx_mesh` | Parses v1–v7 geometry; skips skinning (§3.4). No writer. |
| `rbx_assets` | `AssetRef::{Id, Native, Thumb, Empty}`; `AssetResolver` = disk cache → network for ids, cache → Studio content packages / Sober → substitutes for `rbxasset://`. `AssetCache` stores `ids/<id>` and `native/<relative path>` under `$XDG_CACHE_HOME/rbx-native/assets/`. |
| `rbx_viewer/src/scene/filemesh*`, `assets.rs` | `MeshPart.MeshId` → `AssetRef::parse` → `assets::load_meshes` → `rbx_mesh::parse`. `Fit::Part` scales the native mesh so its extent matches `Size`, using `InitialSize` if set. **No bone skinning in the renderer.** The "Skin" enum in `renderer/filemesh/pipelines.rs` is about textures, not skeletal skinning. |

### 6.2 Recommended end-to-end flow

```
file ─► (new) geometry reader: glTF via `gltf` (utils); FBX via ufbx or extended fbxcel; OBJ via tobj
     ─► normalize (weld, up, front −Z, scale)                         §4.1
     ─► landmarks → 15 joints (+ draggable markers, mirrored)         §4.2
     ─► cut into 15 capped parts (A)                                  §4.5(A)
     ─► weights (B, optional)                                         §4.5(B)
     ─► LOCAL PREVIEW: 15 MeshParts via local mesh content + build_rig with custom Pieces   §6.3
     ─► Publish: write one glTF (15 `<Part>_Geo` meshes [+ armature + skin]) → import3d upload
        (mock: RBX_STUDIO_IMPORT_MOCK) → read MeshIds/InitialSize from returned Model
        → swap the local MeshIds for rbxassetid:// ids → keep our own rig instances
```

**Rigid variant (A).** Upload the 15 meshes un-skinned and keep our own joints and attachments, which is the most predictable path.

**Skinned variant (A+B).** Upload with an armature and let Roblox's importer build the rig. Afterwards, compare the returned RigAttachments with ours. Whether Open Cloud auto-detects the R15 rig is **UNVERIFIED** and must be tested (§2.1).

### 6.3 Previewing a not-yet-uploaded mesh

**Facts:**
- The viewer only draws `MeshPart`s whose `MeshId` resolves through `AssetResolver`.
- `AssetResolver::resolve_native(path)` **first checks `AssetCache::get_native(path)`**, before Studio content packages (`REPO: rbx_assets/src/resolver.rs`).
- `AssetCache` maps `rbxasset://<p>` to `<cache>/native/<p>` (`cache.rs`).
- `rbx_mesh` parses `version 2.00` meshes.

**[RECOMMEND] Concrete approach, needing no renderer changes:**

1. Write each generated part as a `version 2.00` mesh:

   ```
   "version 2.00\n"
   u16 12, u8 40, u8 12, u32 nverts, u32 nfaces
   vertex: f32 pos[3], f32 normal[3], f32 uv[2], i8 tangent[4] (0 = absent), u8 rgba[4] (255)
   u32 face[3]
   ```

   This layout is per `REPO: rbx_mesh/src/v2.rs`, `records.rs`. Vertices are in part space, centred on the bbox centre. UV origin is top-left as in glTF: the repo's glTF export writes mesh UVs unflipped and only OBJ writes `1 − v` (`rbx_viewer/src/export/obj.rs`).
2. Store it with `AssetCache::put_native("rbx-native-local/autorig/<content-hash>/<Part>.mesh", bytes)`.
3. Set `MeshPart.MeshId = "rbxasset://rbx-native-local/autorig/<hash>/<Part>.mesh"` and `InitialSize = Size = bbox size`.
4. Feed these as custom `Piece`s into `rig::build_rig`, so the preview is a real Humanoid rig that the Animation Editor and Animator can already pose.
   - `Piece.mesh` is currently a `u64` asset id. Change it to a content string or enum so it can carry the local URI.
5. Content-hash naming makes re-cuts on marker drag new paths. That avoids stale-cache problems, because the viewer keys meshes by `AssetRef`.

**Caveats:**
- These `rbxasset://rbx-native-local/...` ids are meaningless to real Roblox. The Publish step must replace them.
- Save and Export should warn while any remain.
- Studio's own `rbxasset://` resolves inside its content folder, so the syntax is valid; it just won't be found there.

**Cleaner long-term alternative:**
- Add `AssetRef::Local(PathBuf)` or an in-memory registry (`AssetResolver::register(uri, bytes)`) and a `rbxnative-local://` scheme. That needs changes in `asset_ref.rs`, `resolver.rs` and `viewer::assets`.
- Roblox's own analogue is an `EditableMesh` assigned via `MeshPart.MeshContent` (`Content` supports "asset URIs and EditableMesh objects", `API: MeshPart.yaml`).
- `EditableMesh` also has `AddBone{Name, ParentId, CFrame in mesh local space}`, `SetVertexBones` and `SetVertexBoneWeights` (`API: EditableMesh.yaml`). That is a useful model for an in-memory skinned-mesh type.

**Skinned preview (B):**
- The renderer has no skeletal skinning.
- For ≤ 20k vertices, **CPU linear-blend skinning** per frame is cheap:
  `v' = Σ w_j · (Part_j.CFrame · C1_j-relative bind⁻¹) · v`
  Upload the result as a transient mesh, re-registering it under a per-frame or per-pose key.
- Or add a GPU path later with joints/weights vertex attributes and a bone-matrix uniform array of 15–52 entries.
- Until then, the rigid A preview, with each part moving with its joint, is a faithful preview of the joints.

---

## 7. Open questions to verify with a real key or in Studio

1. Does Open Cloud model upload (glTF/FBX) auto-detect R15 and produce Motor6D/AnimationConstraint joints and RigAttachments? Which joint type does it use, and does it keep `_Geo` names or strip them?
2. Importer unit scale for glTF (metres → studs?) and for FBX. Compare returned `InitialSize` with the authored bbox; the `freeze` code already does this check.
3. In an imported skinned R15 body, are the 15 core bones absent as `Bone` instances? Expected **yes** (§2.2).
4. The exact geodesic-voxel weight kernel from the Dionne & de Lasa paper. Unread; only one implementation's choice is known.
5. HRP size or height convention for Rthro-scale custom bodies. The reference models use 2,2,1 for both families.
