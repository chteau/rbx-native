# Animation — research notes

The primary-source research behind `crates/rbx_animation` and the Animation
Editor document: Roblox's classes, enums and blend rules for
`KeyframeSequence` playback, `AnimationConstraint` rigs, the Animation
Editor's feature set, the Animation Graph system, and the routes a Blender
animation can take into a place. Each fact names its source. **UNVERIFIED**
marks the ones no primary source settles.

## What the implementation decided where the sources disagree

- **Pose easing direction.** The `PoseEasingDirection` enum doc says pose
  `In`/`Out` are backwards from TweenService. The Animation Editor docs, and
  the Blender exporter (checked against `Animator:StepAnimations`), give them
  the standard meaning. `PoseEasingStyle` deprecates `Cubic` because its
  direction "is reversed between the editor and runtime". `rbx_animation`
  therefore plays `CubicV2`, `Elastic` and `Bounce` with the standard meaning
  and the legacy `Cubic` reversed. This still needs checking against a
  running client.
- **Constant.** `Out` holds the earlier pose, `In` jumps to the next one
  (the exporter's reading), and `InOut` swaps at the midpoint.
- **Missing poses.** Each joint interpolates across only its own keys. A
  weightless `Pose` with poses under it is structural and is not a key.
- **Animation Graph storage.** How a node's properties are stored is not
  documented. The editor stores them as attributes on the
  `AnimationNodeDefinition` and wires pins as `ObjectValue` children named
  after the pin, which is how the docs describe `Mask`, `EntryState`,
  `From` and `To`.
- **Blender units.** No source states the scale Studio's importer applies.
  The glTF and FBX importers fit the skeleton to the rig instead, which
  recovers both the axis convention and the units from the rig itself.

---

Researched 2026-10-11 for rbx-native.

**Sources used**
- Roblox/creator-docs `main` (cloned 2026-10-11). Raw base: `https://raw.githubusercontent.com/Roblox/creator-docs/main/content/en-us/`. Below, `CD:` stands for that prefix.
- API dump for Studio **0.742.0.7421053**, from the MaximumADHD/Roblox-Client-Tracker mirror: `https://raw.githubusercontent.com/MaximumADHD/Roblox-Client-Tracker/roblox/API-Dump.json` and `.../Full-API-Dump.json` (the second one includes default values). `setup.rbxcdn.com` was blocked by the egress proxy, so the mirror was used instead. The tracker's `version.txt` matches the version above.
- Blender addon sources:
  - `https://github.com/Cautioned/Blender-Animations-Plugin` (GPL-3.0, v3.1.1, HEAD 540a3cc, 2026-10-09)
  - Den_S/Cautioned v1.96 single-file addon (MIT-style permission notice): gist `https://gist.github.com/AlirezaF80/199c1e4f5b1f7bef11de94b23d7443cc`

**Not reachable from this sandbox:** devforum.roblox.com, which was blocked by both the proxy and WebFetch DNS. The Animation Graph beta post (4554788) and the Avatar Joint Upgrade posts could not be read directly. Everything here about the Animation Graph comes from creator-docs and the API dump instead. Statements marked **UNVERIFIED** are not backed by a primary source.

---

## 1. Classes

### AnimationClip (abstract base class; NotCreatable)
Source: `CD:reference/engine/classes/AnimationClip.yaml`
- Subclasses: `KeyframeSequence`, `CurveAnimation`, and `AnimationGraphDefinition` (the last one appears in the API dump only).
- `Loop: bool`. Means "intended to loop". `AnimationTrack.Looped` defaults to this value.
- `Priority: AnimationPriority`. `AnimationTrack.Priority` defaults to this value.
- `Length: float`. ReadOnly and NotReplicated. Returns 0 until the clip has loaded.
- Hidden properties: `Guid: string` and `GuidBinaryString: BinaryString` (serialized), per the API dump.

### KeyframeSequence : AnimationClip
Source: `CD:reference/engine/classes/KeyframeSequence.yaml`
- Holds `Keyframe` children. "The Keyframe with the highest Time determines the length of an animation."
- `AuthoredHipHeight: float`. Hidden, with PluginSecurity for read and write. The docs say it is "the hip height of the Humanoid of the model used to author", with a default of `1.35`, the standard R15 value. However, the Full API dump lists the default for a new instance as `2`. The two sources disagree.
- Methods:
  - `AddKeyframe(keyframe: Instance)` parents the keyframe and does not type-check it.
  - `GetKeyframes() -> {Keyframe}`.
  - `RemoveKeyframe(keyframe)` sets the parent to nil and does not destroy the keyframe.
- To preview without uploading, use `KeyframeSequenceProvider:RegisterKeyframeSequence()` or `AnimationClipProvider:RegisterAnimationClip()`.

### Keyframe : Instance
Source: `CD:reference/engine/classes/Keyframe.yaml`
- `Time: float`, in seconds. The default is 0.
- Poses are "named in accordance with the BaseParts they correspond to and are structured in terms of joint hierarchy… each Pose is parented to the Pose corresponding to the part it is attached to". Part names must be distinct.
- The style of interpolation is set on the Pose. The Keyframe only holds Poses at a point in time.
- Methods: `AddPose`, `GetPoses`, `RemovePose`, `AddMarker(marker)`, `GetMarkers() -> {KeyframeMarker}`, `RemoveMarker`. None of them type-check their argument.
- Keyframe names do not need to be unique. The deprecated `KeyframeReached` event fires for every keyframe that is not named "Keyframe". `GetMarkerReachedSignal` is used for markers.

### KeyframeMarker : Instance
Source: `CD:reference/engine/classes/KeyframeMarker.yaml`
- `Value: string` is passed to `GetMarkerReachedSignal(name)` handlers. `Name` is the event name and does not need to be unique.
- Markers are parented to a Keyframe. When the keyframe is reached, one event fires per marker.

### PoseBase : Instance (NotCreatable)
Source: `CD:reference/engine/classes/PoseBase.yaml`
- `EasingStyle: PoseEasingStyle`. "The easing style to use to reach the **next** Pose's value."
- `EasingDirection: PoseEasingDirection`. "The easing direction to use to reach the **next** Pose's value."
- `Weight: float`. Deprecated: "Use AnimationTrack:AdjustWeight()". It is still serialized (CanLoad and CanSave are true).

**Consequence: the easing set on the earlier ("from") pose controls the segment from that pose to the next one.** This follows directly from the doc wording "to reach the next Pose's value".

### Pose : PoseBase
Source: `CD:reference/engine/classes/Pose.yaml`
- `CFrame: CFrame`. The default is identity.
- `MaskWeight: float`. Deprecated. It is NotReplicated and CanSave is false.
- Methods: `AddSubPose`, `GetSubPoses` (returns all children), `RemoveSubPose`.
- Mapping from Poses to joints. Quote:

  > Although a Pose is assigned to a BasePart by name, the object manipulated during animation playback is actually the Motor6D connected to this part… In an R15 rig the root part is the HumanoidRootPart. The LowerTorso is connected to the HumanoidRootPart by a motor named 'Root'. Therefore, the CFrame of a Pose named 'LowerTorso' would be applied to the motor named 'Root'… the Pose's CFrame is applied to the motor connecting the part associated with the pose to the part associated with the pose's parent.

  So the tree is `Keyframe > Pose "HumanoidRootPart" > Pose "LowerTorso" > Pose "UpperTorso" > …`. A Pose named for **Part1** sits under the Pose named for **Part0**.
- Pose.CFrame is written to `Motor6D.Transform`, and "the original C0 and C1 values are not changed."
- `Motor6D.ChildName` and `ParentName` are read-only and NotScriptable. They mirror `Part1.Name` and `Part0.Name`. Their docs say: "The animation system uses this name to match the joint against the corresponding pose channel". Source: `CD:reference/engine/classes/Motor6D.yaml`.

### NumberPose : PoseBase (FACS)
Source: `CD:reference/engine/classes/NumberPose.yaml`
- `Value: double`. The default is 0. The control it drives is chosen by the NumberPose's name.
- NumberPoses cannot be nested. They must sit in a `Folder` named **FaceControls** that is a child of the **Head** Pose.
- Values are clamped to [0, 1].

### CurveAnimation : AnimationClip
Source: `CD:reference/engine/classes/CurveAnimation.yaml`
- Under a CurveAnimation, a hierarchy of `Folder`s mirrors the joint hierarchy (Motor6Ds or Bones). Each joint Folder can contain:
  - `Position: Vector3Curve`, which drives local translation.
  - `Rotation: EulerRotationCurve` or `RotationCurve`, which drives local rotation.
- **Partial and relative matching:** not every joint has to be present. The first child folder can be, for example, `UpperTorso`, and it matches any sub-hierarchy in the model that fits.
- FACS: a Folder named after a `FaceControls` instance holds `FloatCurve`s, each named after a FaceControls property.

### FloatCurve
Source: `CD:reference/engine/classes/FloatCurve.yaml`; `CD:reference/engine/datatypes/FloatCurveKey.yaml`
- `Length: int` (read-only) is the number of keys.
- Methods:
  - `GetKeyAtIndex(int)`.
  - `GetKeyIndicesAtTime(float) -> {int,int}`. Returns the index of the last key with time ≤ t and the index of the first key with time ≥ t.
  - `GetKeys`, `GetValueAtTime(float) -> float?`.
  - `InsertKey(FloatCurveKey) -> {bool added, int index}`. A key at the same time is replaced.
  - `RemoveKeyAtIndex(start, count=1) -> int`.
  - `SetKeys(array) -> int`. Sorts the keys and removes duplicate times in a stable way.
- `FloatCurveKey.new(time, value, Interpolation)` has these fields:
  - `Time`.
  - `Value`.
  - `Interpolation: KeyInterpolationMode`. The default is **Cubic**. It applies to the segment that starts at this key.
  - `LeftTangent` and `RightTangent: number?`. Both default to nil, which evaluates as 0. They can only be set when the key is Cubic. Switching the key to a non-cubic mode clears `RightTangent`.

### RotationCurve
Source: `CD:reference/engine/classes/RotationCurve.yaml`; `CD:reference/engine/datatypes/RotationCurveKey.yaml`
- It has the same API as FloatCurve, except `GetValueAtTime -> CFrame?`. An empty curve evaluates to identity.
- `RotationCurveKey.new(time, cframe, Interpolation)`. Only the rotation part of the CFrame is used.
- Interpolation modes:
  - Constant holds the value.
  - Linear uses **slerp**.
  - Cubic uses a "cubic hermite spline shaped by RightTangent and the next key's LeftTangent".
- The default interpolation is Cubic.

### EulerRotationCurve
Source: API dump
- `RotationOrder: RotationOrder`. The default is XYZ.
- Methods: `GetAnglesAtTime(t) -> Array`, `GetRotationAtTime(t) -> CFrame`, and `X()`, `Y()`, `Z()`, each returning a `FloatCurve`.

### Vector3Curve
Source: `CD:reference/engine/classes/Vector3Curve.yaml`
- Holds three child `FloatCurve`s named X, Y and Z. `X()`, `Y()` and `Z()` create an empty curve if one is missing.
- `GetValueAtTime(t) -> {x?,y?,z?}`. A channel with no curve or no keys evaluates to nil.

### MarkerCurve
Source: `CD:reference/engine/classes/MarkerCurve.yaml`
- A list of time-sorted string markers. Each string can be at most 64 characters and must contain only printable characters.
- Methods: `InsertMarkerAtTime(t, s)`, `GetMarkers`, `GetMarkerAtIndex`, `RemoveMarkerAtIndex(start, count=1)`.

### Animation
Source: `CD:reference/engine/classes/Animation.yaml`
- `AnimationId: ContentId`.
- `AnimationContent: Content`. It is the underlying storage for AnimationId, and setting either one updates the other.
- Replication rules:
  - An Animator under a player's character replicates animations that the client starts.
  - Any other Animator must be played on the server.
  - An Animator created locally does not replicate.

### Animator
Source: `CD:reference/engine/classes/Animator.yaml`; API dump
- `LoadAnimation(Animation) -> AnimationTrack`. It always creates a new track. The Animator must be in Workspace, otherwise it cannot get AnimationClipProvider and the call errors.
- `GetTrackByAnimationId(ContentId) -> AnimationTrack?`.
- `GetPlayingAnimationTracks()`. Includes tracks that are fading out. They are removed once their weight reaches 0.
- `StepAnimations(deltaTime: float)`. PluginSecurity (command bar and plugins only). It advances `TimePosition` for all playing tracks and applies the result to the model. Afterwards the joints must be reset manually.
- `ApplyJointVelocities(motors: Variant)`. Uses the playing tracks, their times and their speeds to compute relative velocities between parts, and applies them to `Motor6D.Part1` in the order given. A joint is skipped if both of its parts are still in the same assembly. You must disable the rigid joints first. The motors do not have to be in the DataModel.
- `EvaluationThrottled: bool` (read-only). True when the animator reused the previous frame's pose because of LOD.
- `PreferLodEnabled: bool`. The default is true.
- Members in the API dump only:
  - `RootMotion: CFrame` and `RootMotionWeight: float`, both read-only and NotBrowsable.
  - `RegisterEvaluationParallelCallback(fn)`, `SynchronizeWith(otherAnimator)`, `StepAnimationsInternal(dt, options)`.
  - `AnimationPlayed(track)` event.

### AnimationTrack (NotCreatable)
Source: `CD:reference/engine/classes/AnimationTrack.yaml`; API dump
- `Play(fadeTime: float = 0.1, weight: float = 1, speed: float = 1)`. The weight ramps from 0 to `weight` over `fadeTime`. To start from a specific TimePosition, call Play first.
- `Stop(fadeTime: float = 0.1)`. The weight goes to 0 over fadeTime no matter what the starting weight was. After Stop, `Stop` and `AdjustWeight` do not work until `Play` is called again. Using fadeTime 0 together with `Motor.MaxVelocity` 0 freezes the joints.
- `AdjustWeight(weight: float = 1, fadeTime: float = 0.1)`. `WeightCurrent` moves toward `WeightTarget` over fadeTime.
- `AdjustSpeed(speed: float = 1)`. Negative values play backward and 0 pauses.
- Properties:
  - `Looped: bool`. A change while playing takes effect at the end of the current loop. Reverse playback is handled.
  - `Priority: AnimationPriority`.
  - `TimePosition: float`. NotReplicated. Setting it only works while the track is playing.
  - `Speed: float` (read-only).
  - `Length: float` (read-only). 0 until loaded.
  - `WeightCurrent` and `WeightTarget` (read-only). WeightTarget defaults to 1.
  - `IsPlaying` and `Animation` (read-only).
- Events:
  - `Stopped`.
  - `Ended`. Fires after the fade-out finishes and the subject is neutral.
  - `DidLoop`.
  - `KeyframeReached(name)`. Deprecated.
  - `ParameterChanged(key, value)`. In the API dump.
- `GetMarkerReachedSignal(name)` and `GetTimeOfKeyframe(name) -> double`. The latter returns the first keyframe with that name and errors if it is missing or not loaded.
- Animation Graph members:
  - `SetParameter(key: string, value: Variant)`. Values can be set before the graph loads and are applied later. NaN and Inf become 0 and produce a warning.
  - `GetParameter(key) -> Variant`. Returns nil if the parameter was never set.
  - `GetParameterDefaults() -> Dictionary`. These are the attributes on the `AnimationGraphDefinition`. Returns {} for animations that are not graphs.
  - In the API dump only: `ResetGraph()`, `UpdateGraphNodeDefinition(nodeId: string, nodeDefinition: Instance) -> bool`, `UpdateGraphNodeProperty(nodeId, propertyName, value, inputPinName = "") -> bool`, `SetTargetInstance(name, target)`, `GetTargetInstance(name)`, `GetTargetNames()`, `GetDebugData()`.

### AnimationClipProvider (service)
Source: `CD:reference/engine/classes/AnimationClipProvider.yaml`
- Replaces the deprecated KeyframeSequenceProvider.
- `GetAnimationClipAsync(ContentId) -> AnimationClip` yields. `GetAnimationClip` and `GetAnimationClipById(int64, useCache)` are deprecated.
- `RegisterAnimationClip(clip) -> ContentId` returns a hash ID. `RegisterActiveAnimationClip(clip) -> ContentId` returns an `active://` URL. Both IDs are temporary and work in Studio only.
- `GetAnimationsAsync(userId) -> InventoryPages`.
- `GetClipEvaluatorAsync(ContentId) -> ClipEvaluator`. Samples a clip at any time without an Animator.

---

## 2. Enums

Integer values below are verified against both the creator-docs YAML (`CD:reference/engine/enums/<Name>.yaml`) and the API dump.

| Enum | Items (value) |
|---|---|
| **PoseEasingStyle** | Linear 0, Constant 1, Elastic 2, Cubic 3 (**deprecated**), Bounce 4, CubicV2 5 |
| **PoseEasingDirection** | In 0, Out 1, InOut 2 |
| **EasingStyle** (TweenService) | Linear 0, Sine 1, Back 2, Quad 3, Quart 4, Quint 5, Bounce 6, Elastic 7, Exponential 8, Circular 9, Cubic 10 |
| **EasingDirection** (TweenService) | In 0, Out 1, InOut 2 |
| **KeyInterpolationMode** | Constant 0, Linear 1, Cubic 2 |
| **AnimationPriority** | Idle 0, Movement 1, Action 2, Action2 3, Action3 4, Action4 5, **Core 1000** |
| **RotationOrder** | XYZ 0, XZY 1, YZX 2, YXZ 3, ZXY 4, ZYX 5 |
| **RolloutState** (AvatarJointUpgrade) | Default 0, Disabled 1, Enabled 2 |

### PoseEasingStyle notes
- Pose.EasingStyle defaults to Linear.
- The direction applies to the asymmetric styles: Elastic, Bounce and CubicV2.
- "Cubic is a legacy alias with a known direction-reversal bug between the Animation Editor and runtime; use CubicV2." The deprecation message reads: "Cubic has a bug where the direction is reversed between the editor and runtime."
- Constant does not interpolate. It "snaps to the keyframe indicated by the PoseEasingDirection".

### PoseEasingDirection, quoted from the docs
> When a Pose has an EasingStyle which is not symmetric, the EasingDirection determines if the curve is applied from previous keyframe to next, next keyframe to previous, or in both directions… For the symmetric Linear and Constant EasingStyles, this property has no effect. **For legacy compatibility reasons, the use of In and Out are backwards from how these terms are used by most other animation tools and by TweenService.**
> - In: "The EasingStyle curve is reversed, with the easing becoming linear as it approaches the next keyframe."
> - Out: "EasingStyle curves are applied in the forward direction, from the current keyframe to the next."
> - InOut: "Two easing curves are applied back-to-back with the linear ends of the curves meeting in the middle."

The PoseEasingDirection enum doc says Constant has no effect, but the PoseEasingStyle enum doc says Constant snaps according to the direction. The two docs conflict.

The Cautioned addon, in `roblox_animations/animation/easing.py`, says "Constant Out holds the previous pose; In jumps to the next pose immediately". That is the more useful behaviour, but it is community evidence only (**UNVERIFIED**).

### Conflicting evidence about direction (treat as UNVERIFIED and test empirically)
- `CD:animation/editor.md` describes the editor menu with standard meanings: "In — slower at the beginning and faster toward the end; Out — faster at the beginning and slower toward the end". `CD:animation/curve-editor.md` says the same for generated Bounce and Elastic curves.
- The Cautioned addon maps Blender `EASE_IN → "In"` and `EASE_OUT → "Out"` for CubicV2 and Bounce. Its comment says this was "Verified against Animator:StepAnimations, not just enum names". It also notes "Studio's Bounce InOut differs from Blender's combined bounce curve".
- One possible reading is that the CubicV2 runtime matches the standard meaning while the legacy Cubic is reversed. The docs do not state this. **Verify in Studio** with `StepAnimations` and a single joint before you fix the direction semantics.

### AnimationPriority blending (enum description, verbatim semantics)
> When multiple AnimationTracks are played concurrently by the same Animator and affect the same animated joints, the tracks are evaluated in order from high to low priority, **per joint**, while the total track weight sum remains less than 1.0. When the track weight sum reaches or exceeds 1.0 for a joint, evaluation stops and no lower-priority tracks will be evaluated for that joint. AnimationTracks with the same priority… blend proportional to their track weights (normalized if the sum exceeds 1).

- Priority order from highest to lowest: Action4 > Action3 > Action2 > Action > Movement > Idle > **Core**. Core has the numeric value 1000 but is the **lowest** priority. Roblox's default character animations and the catalog bundles play at Core.
- From the AnimationTrack.AdjustWeight docs: "The pose that is shown at any point in time is determined by the weighted average of all the Poses and the WeightCurrent of each AnimationTrack."

---

## 3. Playback semantics

### Joint formula
- The Weld docs give `Part1.CFrame * C1 == Part0.CFrame * C0` (`CD:reference/engine/classes/Weld.yaml`).
- For Motor6D, the docs say that Transform is "the offset between these parts" and is applied in addition to C0 and C1. They do **not** give the exact multiplication order. That order is **UNVERIFIED in docs**, but it is the widely used convention:
  `Part1.CFrame = Part0.CFrame * C0 * Transform * C1:Inverse()`
- The Den_S addon's math is consistent with this order. See section 7: the exported per-bone value works out to exactly `Transform` under this formula.

### Timing of Transform
- Transforms are not applied immediately. They are applied as a batch in a parallel job after `RunService.PreSimulation` and immediately before physics.
- The Animator overwrites Transform after `PreAnimation` and before `PreSimulation`.
- Source: `Motor6D.yaml` and `AnimationConstraint.yaml`.

### Interpolation between keyframes
- The easing comes from the earlier pose. PoseBase says "easing … to reach the next Pose's value".
- **Missing poses:** no official doc covers this case. Community implementation evidence, in `src/.../Components/RigPart.lua` of the Cautioned plugin, says: "Missing keys must remain missing. The Animator interpolates each channel across its own authored keys. Synthesizing intermediate keys restarts nonlinear easing whenever a sibling has a key."
- The same plugin writes "structural" Poses with `Weight = 0` when a joint has no key of its own but its descendants do. This matches how Studio's Animation Editor exports, but it is **UNVERIFIED** against official docs.
- Implementation recommendation:
  1. Build a per-joint channel from the keyframes where a Pose with that name exists and has Weight > 0.
  2. Interpolate between neighbouring keys in that channel, using the earlier key's EasingStyle and EasingDirection.
  3. Before the first key or after the last key, hold the value.
- Looping does not interpolate from the last keyframe back to the first. The editor docs say: "duplicate the first keyframes and use them as the final keyframes."
- Rotation is interpolated as quaternions. `curve-editor.md` says "KeyframeSequences work with quaternions."

### FACS
NumberPose values are clamped to [0, 1].

---

## 4. AnimationConstraint and the Avatar Joint Upgrade

Source: `CD:reference/engine/classes/AnimationConstraint.yaml`, `CD:physics/constraints/animation.md`, `CD:characters/index.md`, API dump (Full API dump for defaults)

- Inherits `Constraint`, so it has Attachment0, Attachment1, Enabled and so on.
- Properties:

| Property | Type | Default (Full API dump) |
|---|---|---|
| Transform | CFrame | identity |
| IsKinematic | bool | **false** for `Instance.new` |
| MaxForce | float | 10000 |
| MaxTorque | float | 10000 |
| LinearStrength | float | 1 |
| AngularStrength | float | 1 |
| LinearDamping | float | 1 |
| AngularDamping | float | 1 |

  - The class doc says "IsKinematic: When true (default), behavior is equivalent to Motor6D." The likely meaning is that upgraded avatar rigs ship with IsKinematic set to true (**UNVERIFIED**).
  - Hidden properties: `EnableSkinning` (default true). Also `C0`, `C1`, `Part0` and `Part1`, which are read-only, NotReplicated and deprecated.
- Strength is "normalized natural frequency, f/60". The default of 1 means 60 Hz. Damping is a damping ratio ζ, and 1 is critical damping. Strength and damping only apply when IsKinematic is false, and the resulting force or torque is capped by MaxForce or MaxTorque.
- Read-only aliases: `C0 = Attachment0.CFrame`, `C1 = Attachment1.CFrame`, `Part0 = Attachment0.Parent`, `Part1 = Attachment1.Parent`. "Do not modify RigAttachment.CFrame directly."
- "Constrains its Attachments so that they're offset by Transform." When IsKinematic is true, "the parts follow the transform perfectly (identical to Motor6D behavior)".
- Derived formula (**UNVERIFIED as a doc quote**; it follows from the aliases above plus Motor6D equivalence):
  `Attachment1.WorldCFrame = Attachment0.WorldCFrame * Transform`, which means
  `Part1.CFrame = Part0.CFrame * Att0.CFrame * Transform * Att1.CFrame:Inverse()`
- `Transform` "works identically to Motor6D.Transform — the Animator writes to it each frame".
- Procedural layering pattern: during `PreSimulation`, `if not animator.EvaluationThrottled then neck.Transform = rot * neck.Transform end`.
- `animConstraint:IsA("Motor6D")` returns false. Code should check `FindFirstChildWhichIsA("AnimationConstraint")` first and fall back to Motor6D.
- **StarterPlayer.AvatarJointUpgrade: RolloutState**.
  - Security is RobloxScriptSecurity, it is NotReplicated, and it is not serialized.
  - The serialized twin is `AvatarJointUpgrade_SerializedRollout`.
  - When enabled, avatars spawn with `AnimationConstraint`s plus `BallSocketConstraint`s connecting the limbs.
  - It is "the default for new experiences".
  - `StarterPlayer.CharacterBreakJointsOnDeath` requires AvatarJointUpgrade.
- DevForum references named in the docs (not fetched, because devforum was blocked):
  - https://devforum.roblox.com/t/avatar-joint-upgrade-for-physically-simulated-character-movement-is-now-live/4298561
  - https://devforum.roblox.com/t/avatar-joint-upgrade-aju-phase-2-rollout-updated-migration-recommendations/4656414
- Bone (API dump):
  - `Transform: CFrame`, NotReplicated.
  - `TransformedCFrame` (hidden) and `TransformedWorldCFrame` (read-only).
  - CurveAnimation and Pose channels can drive Bones as well.

---

## 5. Roblox Animation Editor

Sources: `CD:animation/editor.md`, `CD:animation/curve-editor.md`, `CD:animation/events.md`, `CD:avatar/emotes/import.md`, `CD:art/characters/export-avatar-animations-from-maya.md`, `CD:avatar/animation-packs.md`

### Layout
The editor opens from **Avatar** tab ⟩ Animation Editor, or from Window ⟩ Avatar. It has three regions:
1. **Media and playback controls**: the animation name, the playback buttons, a ⋯ menu, and a position indicator in **seconds:frames**.
2. **Track list**:
   - The rig name.
   - A **Manage IK** button.
   - A "+" menu for adding tracks.
   - A per-track ⋯ menu with **Add Keyframe** and **Delete Track**.
3. **Timeline**:
   - The scrubber.
   - Time or frame units.
   - A settings menu with Timeline Unit, Frame Rate, and toggles for Show Animation Events, Default Rotation Type and Default Euler Angles Order.
   - Zoom draggers.

The dope sheet is the default timeline view. A toggle switches it to the Curve Editor.

### Defaults and posing
- The default is **30 fps**, and `0:15` means 0.5 s. The default visible length is 1 s (1:00). To extend it, type a value into the right box of the position indicator.
- Animations can only be created on rigs. Select a Bone or MeshPart and move or rotate it. Doing so creates a track (if needed) and a keyframe at the scrubber.
- **R** toggles between Move and Rotate, using Studio's snap increments.
- **Space** plays.

### Keyframe operations
- Right-click ⟩ **Add Keyframe Here** inserts at the clicked time, not at the scrubber.
- Drag moves keyframes.
- Ctrl/⌘+C and Ctrl/⌘+V paste at the scrubber.
- Delete or Backspace removes keyframes.
- Operations can target a single track, or all tracks via the top summary row.

### Easing
Right-click selected keyframes to get two menus:
- **Easing Style**: Linear, Constant, CubicV2, Elastic, Bounce.
- **Easing Direction**: In, Out, InOut.

### Optimize Keyframes
- Automatic optimization happens for facial animation and when promoting a clip to curves:
  - If 3 or more consecutive keyframes have the same value, only the first and last are kept.
  - A track whose keys all hold default values (CFrame identity, or 0) is removed.
- On-demand: ⋯ ⟩ **Optimize Keyframes** opens a slider that removes the least impactful keyframes first.

### Loop and priority
- The **Looping** button toggles looping. Looping does not interpolate from the last key back to the first.
- ⋯ ⟩ **Set Animation Priority** sets the priority.

### Save
- ⋯ ⟩ **Save** or **Save As** stores a `KeyframeSequence` or `CurveAnimation` in `ServerStorage ⟩ RBX_ANIMSAVES ⟩ <rig name>`. The location is stated in `avatar/animation-packs.md`.
- It also adds a rig child `AnimSaves` (an `ObjectValue`) that references that folder. Access it as `rig.AnimSaves.Value.<Name>`.
- ServerStorage does not replicate.

### Export
- ⋯ ⟩ **Publish to Roblox** opens Asset Configuration with a title, a description and a Creator (choose the group for group-owned games).
- If the animation replaces a default character animation, rename the final keyframe to `End` first (case-sensitive), via **Rename Key Keyframe**.

### Events and markers
- Timeline settings ⟩ **Show Animation Events** shows the event bar.
- **Edit Animation Events** ⟩ **+ Add Event** takes a name and an optional **Parameter** string.
- Events are detected with `GetMarkerReachedSignal(name)`. Each event becomes a KeyframeMarker whose `Value` holds the parameter.

### FBX import
- ⋯ ⟩ **Import** ⟩ **From FBX Animation** (Maya guide), shown as **From File** in the emote guide.
- The target is a rig with a standard R15 armature, with hierarchy and names matching exactly:

  ```
  Root
  └── HumanoidRootPart
      └── LowerTorso
          ├── UpperTorso
          │   ├── Head
          │   ├── LeftUpperArm → LeftLowerArm → LeftHand
          │   └── RightUpperArm → RightLowerArm → RightHand
          ├── LeftUpperLeg → LeftLowerLeg → LeftFoot
          └── RightUpperLeg → RightLowerLeg → RightFoot
  ```

  The Maya guide lists these names in order. The nesting shown follows the standard R15 rig and is not printed in the guide itself.
- Mute the root translation so the animation plays in place.
- Bake the animation.
- **Emotes must be CurveAnimation**, not KeyframeSequence.

### Live Animation Creator (beta)
⋯ ⟩ Import ⟩ Live Animation Creator imports from an .mp4 or .mov video (`CD:animation/capture.md`).

### IK (Manage IK)
The track list has a **Manage IK** button. The docs here describe IK only through runtime `IKControl` (`CD:animation/inverse-kinematics.md`). The editor's IK modes ("Body Part" and "Full Body", pinning) are **UNVERIFIED**: no primary doc text was found in the current repo.

### Curve Editor (`CD:animation/curve-editor.md`)
- Opens from a Curve Editor icon at the top left of the timeline.
- It converts the KeyframeSequence to a **CurveAnimation**. This cannot be undone once quaternions have been converted to Euler.
- The **Default Rotation Type** setting chooses Euler Angles or Quaternions. Quaternion conversion preserves the motion exactly.
- Euler order is set in **Default Euler's Angles Order** (XYZ, XZY, YXZ, YZX, ZXY, ZYX).
  - "XYZ means rotate Z first, then Y, then X." In matrix terms, R = X·Y·Z.
  - Changing the order of an existing Euler track changes the motion.
- Euler curves have left and right tangents for each key. A quaternion curve segment always runs from 0 to 1 over the segment, with a tangent at each end.
- Unset tangents appear as hollow circles and are auto-computed. Right-click a tangent ⟩ **Set Tangent To Zero** resets it.
- Interpolation modes are Linear (default in the editor), Constant and Cubic. Only Cubic has tangents.
- Bounce and Elastic are not native to curves. **Generate Curve ⟩ Bounce|Elastic ⟩ In|Out|InOut** bakes intermediate keys.
- Rulers: position on the left and rotation on the right.

---

## 6. Animation Graph (beta, now apparently full release)

Sources: `CD:animation/graph-editor.md`, `CD:animation/graph-editor-state-machine.md`, `CD:animation/expressions.md`, `CD:reference/engine/classes/AnimationNodeDefinition.yaml`, `CD:reference/engine/enums/AnimationNodeType.yaml`, `CD:reference/engine/enums/AnimationNodeBlendMode.yaml`, API dump 0.742.

Release status:
- The DevForum beta post 4554788 could not be fetched (blocked).
- A web search shows a later "[Full Release] Animation Graphs" post at https://devforum.roblox.com/t/full-release-animation-graphs-create-complex-character-motion-visually/4739840 (not fetched).
- The current `graph-editor.md` on main no longer carries a beta alert.

### Classes (API dump)

| Class | Base | Members |
|---|---|---|
| `AnimationGraphDefinition` | AnimationClip | No members of its own. It is the asset or container. Graph parameter defaults are stored as **attributes** on it (from the GetParameterDefaults doc). |
| `AnimationNodeDefinition` | Instance | `NodeId: string` (default "", RobloxScriptSecurity per docs), `NodeType: AnimationNodeType` (default InvalidNode), hidden serialized `InputPinData: BinaryString`. Methods `AddInputPin(pin: string)`, `RemoveInputPin(pin)`, `GetOrderedInputPinNames() -> Array`, `SetOrderedInputPinNames(pins: Array)`. Event `InputPinsChanged()`. Changing NodeType resets the pins. **Pins are wired by child `ObjectValue`s whose Name equals the pin name.** Pin order matters for Sequence, RandomSequence and PrioritySelect. |
| `StateMachineDefinition` | Instance (NotReplicated) | `NodeId: string` |
| `StateMachineTransitionDefinition` | Instance | `From: Instance`, `To: Instance`, `Priority: int` (default 0), `TransitionId: string` |
| `AnimationNode` | Object (NotCreatable, NotReplicated) | No members listed. It is presumably the runtime handle. |
| `AnimationRigData` | Instance | `LoadFromModel`, `LoadFromHumanoid`, `GetNames`, `GetParents`, `GetTransforms`, `GetPreTransforms`, `GetPostTransforms`, `GetLabels`, `IsValidR15`, `IsValidR15Plus`, `Dump` |

How node properties such as `AnimationId`, `Speed` and `Weight` are stored is not documented. They are likely attributes on the `AnimationNodeDefinition`, or ObjectValue pins for inputs that can be connected (**UNVERIFIED**). The docs do say that `Mask` is an ObjectValue child carrying weight attributes, and that EntryState, From and To are ObjectValues.

### Graph enums (API dump)

| Enum | Items (value) |
|---|---|
| AnimationNodeType | InvalidNode 0, AddNode 1, OverNode 2, Blend1DNode 3, Blend2DNode 4, ClipNode 5, GraphOutput 6, MaskNode 7, PrioritySelectNode 8, RandomSequenceNode 9, SelectNode 10, SequenceNode 11, SpeedNode 12, SubtractNode 13, OneShotNode 14, StateMachineNode 16 (no 15) |
| AnimationNodePlayMode | Loop 0, PingPong 1, OnceAndHold 2, OnceAndReset 3 |
| AnimationNodeTransitionType | CrossFade 0, InertialBlend 1, DeadBlend 2 (not described in docs) |
| AnimationNodeTransitionWhen | Finished 0, BeforeFinished 1 |
| AnimationNodeWaitFor | Finished 0, Trigger 1 |
| AnimationNodeInterruptible | Always 0, Finished 1, Trigger 2 |
| AnimationNodePhaseSync | Synced 0, Unsynced 1 |
| AnimationNodeBlend2DInputMode | Cartesian 0, Polar 1 |
| AnimationNodeBlendMode | Over 0, Add 1, Subtract 2 |
| AnimationValueNodeType | Invalid 0, Expression 1 |

### Node reference (from the AnimationNodeType enum doc)

**ClipNode** is a leaf node. Properties:
- `AnimationId` (string).
- `PlayMode`. Loop is the default; the others are PingPong, OnceAndHold and OnceAndReset.
- `Reverse` (bool).
- `Speed` (number). 0 pauses, 1 is normal, 2 is double speed.
- `Trim` (bool), plus `TrimStart` and `TrimEnd` in absolute seconds.

It emits the clip's markers. When trimmed, only markers inside [TrimStart, TrimEnd] (inclusive) are emitted.

**SelectNode** has inputs Input1…N and a `Selection` string that matches an input name. Changing the selection starts a transition.

**PrioritySelectNode** checks its ordered inputs from top to bottom and plays the first one whose `Trigger` (bool) is true. Each input has these settings:
- `TransitionOverrideInterruptible` (`AnimationNodeInterruptible`). The choices are Always (default), Finished, and Trigger together with `InterruptibleTrigger`.
- The node-level `DefaultInterruptible`.

**SequenceNode** plays inputs in order.
- `LoopCount` (0 means infinite) and `DefaultWaitFor` are node-level.
- Each input can set `TransitionOverrideWaitFor` and `WaitForTrigger`.
- When it advances with Finished:
  - A play-once clip advances when it ends.
  - A looping clip advances after one loop.
  - An infinite sub-sequence advances after one cycle.
  - A finite sub-sequence advances after all of its loops.

**RandomSequenceNode** picks inputs at random by weight.
- Each input has a `Weight`.
- `PlayCount` (0 means infinite).
- `Seed`: -1 means random. A fixed seed keeps clients consistent.
- It supports `DefaultTransitionWhen` and `TransitionOverrideWhen`.

**OverNode** has inputs Base and Over. `Weight` defaults to 1 and is unclamped; it attenuates Over. Joints masked out of Over reveal Base. Events from Over are scaled by Weight.

**AddNode** has inputs Base and Additive. `Weight` is unclamped.

**SubtractNode** computes `A − Weight·B`. Weight defaults to 1 and is unclamped. Its job is to turn a clip into an additive pose.

**OneShotNode** has inputs Base and OneShot. Properties:
- `Trigger`: a shot starts on a false→true edge.
- `Cancel`.
- `Interruptible`: default true. A new edge restarts the shot.
- `BlendMode`: Over, Add or Subtract.
- `TransitionInDuration` and `TransitionOutDuration`: default 0.15. Negative values become 0.
- `TransitionInCurve` and `TransitionOutCurve`: PoseEasingStyle, but only Linear and CubicV2 take effect; anything else falls back to Linear.
- `TransitionOutWhen`: Finished or BeforeFinished.

Use a **Trigger parameter** for Trigger. It evaluates like a bool and auto-resets to false one frame after being set.

**Blend1DNode** has inputs that each carry a `Position`, plus a node `Position`.
- It linearly interpolates between the two nearest inputs, and **extrapolates** outside the range.
- `PhaseSync`:
  - Synced (default) computes a "virtual duration" from the weighted average of the active inputs and adjusts each child's dt so all children share the same normalized phase.
  - Unsynced lets each child advance on its own.
- Only events from the input with the highest weight are emitted, scaled by its weight.

**Blend2DNode** has inputs that each carry `X` and `Y`, plus node `X`, `Y`, `InputMode` and `PhaseSync`.
- Cartesian mode uses x and y as coordinates.
- Polar mode uses X as the angle in radians and Y as the magnitude. Direction is weighted more heavily.
- The triangulation or weighting algorithm is not documented.

**MaskNode** has a Pose input. Properties:
- `Mask`: an ObjectValue child of the node. It either carries attributes that map rig object names to weights, or references another Instance that carries them.
- `Invert`: when true, each weight becomes 1−w.

The rig schema comes from `HumanoidRigDescription` or from a picked rig.

**SpeedNode** has a Pose input and a `Speed` property that multiplies dt.

**GraphOutput** has a single `Pose` input. It is always present.

**StateMachineNode** has inputs State1…N, and each input name must match a state name.
- `EntryState` is an ObjectValue pointing to the entry state's AnimationNodeDefinition.
- Only events from the active state are emitted.

### Transitions
For Select, PrioritySelect, Sequence and RandomSequence:
- `DefaultTransitionDuration` (seconds) and `DefaultTransitionCurve` (PoseEasingStyle; only Linear and CubicV2).
- Per-input overrides: `TransitionOverrideDuration` and `TransitionOverrideCurve`. An override applies when transitioning **to** that input. The exception is BeforeFinished, which uses the outgoing input's overrides.
- `When = BeforeFinished` starts the blend one duration before the end.

### State machine transitions
- Each transition has:
  - `From`: ObjectValue. Unset means **(Any)**.
  - `To`: ObjectValue.
  - `WaitFor`: Trigger, shown as "Expression", or Finished.
  - `When`.
  - `Expression`: string.
  - `Priority`: 0–65535; higher wins.
  - `Duration`: default **0.15 s**.
  - `Curve`: Linear (default) or CubicV2.
- If several expressions are true, the highest priority wins. Ties are broken at random.
- The editor shows "(Entry)" and "(Any)" pseudo-states.

### Expressions (`CD:animation/expressions.md`)
- This is a Luau subset. Supported:
  - Arithmetic `+ - * / // % ^` and comparisons.
  - `== ~=` on numbers, bools and strings.
  - `and`, `or`, `not` with Luau truthiness and short-circuiting.
  - `if … then … elseif … else` expressions.
  - The `math.*` library, except `frexp`, `noise` and `randomseed`. `modf` returns only the integer part.
  - Construction of Vector2, Vector3, Color3 and CFrame (`new`, `Angles`, `identity`, `fromRGB`, `zero`), plus member access (`Magnitude`, `Unit`, `X`, `LookVector`, `Position`, `R`, …).
  - String `..`.
  - Multi-line bodies with `local`, `return` and single-level `if`. No nested `if` and no `local` inside branches.
- Expressions can drive node properties, for example Blend1D `Position`, and transition conditions.

### Parameters and Luau
- Parameter types seen in the docs are Number, Boolean, Trigger and String (Select uses String).
- In the editor, dragging from a property pin creates a parameter node. There is also a Parameters pane.
- Runtime use:

  ```lua
  local a = Instance.new("Animation")
  a.AnimationId = "rbxassetid://<graph id>"
  local track = animator:LoadAnimation(a)
  track:Play()
  RunService.Stepped:Connect(function()
      track:SetParameter("humanoidSpeed", hrp.AssemblyLinearVelocity.Magnitude)
  end)
  ```

### Events
- Events (markers) propagate upward with a weight. A weight of 0 silences them.
- Markers that reach the top can be observed with `GetMarkerReachedSignal`.

### Authoring in Studio
1. Avatar tab ⟩ **Graph Editor**.
2. Select a rig, then Graph ⟩ **New**. This creates an `AnimationGraphDefinition`.
3. Right-click ⟩ Insert Node.
4. Connect output connectors to ports and press Play to preview.
5. **Graph ⟩ Create Animate script** generates `Animate` (ModuleScript) with `RunClient` (LocalScript) and `RunServer` (Script, RunContext Legacy) as children. Its attributes are `GraphName`, `CharacterName`, `SourceAssetId`, `IsServerAuthority` and `PreviewInStudio`.

### Replication
Replication is chosen by `Workspace.AuthorityMode`.
- In **Automatic** mode:
  - The owning client drives player graphs and the server drives NPC graphs.
  - SetParameter values replicate, coalesced per frame on a last-writer-wins basis.
- In **Server** mode:
  - The server replicates parameters and the full node state: elapsed time, selections, transition progress, loop counts and RNG seeds.
  - The owning client uses `PredictionMode` rollback prediction.

---

## 7. Blender → Roblox

### 7a. Official route: FBX into the Animation Editor
Sources: `CD:art/modeling/export-requirements.md`, `CD:art/blender.md`, `CD:art/characters/creating/export-character.md`, `CD:art/characters/export-avatar-animations-from-maya.md`

- Blender FBX export settings:
  - Path Mode: Copy, with Embed Textures on.
  - **Apply Scalings: FBX Unit Scale**. The alternatives are Transform Scale 0.01, or scene Unit Scale 0.01.
  - Armature: **Add Leaf Bones off**.
  - **Bake Animation on**. In the character guide: NLA Strips, All Actions and Force Start/End Keyframes off, and Simplify set to 0.0.
  - Include Custom Properties.
  - For the generic exporter: **Forward = Z Forward, Up = Y Up**.
- Studio also imports glTF. The Blender doc notes that "GLTF … do not require these additional scaling configurations". The Importer supports rigs and animation data in both .fbx and .gltf (`CD:studio/importer.md`).
- Requirements:
  - Bone names must match the rig part names exactly, using the standard R15 hierarchy.
  - Root translation must be muted so the animation plays in place.
  - The FBX must be baked.
  - Roblox is Y-up and Blender is Z-up. Blender's exporter handles the axis conversion when Y Up is set.
- Units: Roblox studs versus metres. Blender's default FBX export imports "unexpectedly large" unless FBX Unit Scale or 0.01 is used.

### 7b. Den_S "Rbx Animations" addon (the panel in the user's screenshot)

**Lineage**
- The original is by Den_S/@DennisRBLX (DevForum topic 34729), distributed through Pastebin. The Pastebin raw file was blocked from here.
- The single-file v1.96 copy, with the header "Copyright 2024 Den_S/@DennisRBLX / Cautioned's Fork v1.96", is at https://gist.github.com/AlirezaF80/199c1e4f5b1f7bef11de94b23d7443cc. It carries an MIT-style permission notice.
- The current package is **RBXMonkey**, v3.1.1, by Cautioned: https://github.com/Cautioned/Blender-Animations-Plugin. It is **GPL-3.0** and has a companion Studio plugin, Creator Store asset 16708835782.

**Operators in v1.96**
These match the screenshot. The operators are listed in the gist with their `bl_idname`s; the panel labels are from the screenshot.

| Operator (`bl_idname`) | What it does |
|---|---|
| `rbxanims_importmodel` | Import rig data (.obj) |
| `rbxanims_genrig` | "Rebuild rig" in the panel |
| `rbxanims_genik` and `rbxanims_removeik` | Create IK constraints |
| `rbxanims_importfbxanimation` | Import FBX. Requires exactly one armature, and every target bone name must exist in the source. |
| `rbxanims_mapkeyframes` | Map keyframes by bone name |
| `rbxanims_applytransform` | Apply armature object transform to the root bone for each keyframe |
| `rbxanims_bake` | Export: copies the result to the clipboard |
| `rbxanims_bake_file` | Export to a file |

**Studio → Blender (rig export)**
- Studio writes an `.obj` file.
- Metadata is carried in extra mesh objects named `Meta<N>q1<chunk>q1…`. The regex is `^Meta(\d+)q1(.*?)q1\d*(\.\d+)?$`.
- Concatenate the chunks in order of N, replace `'0'` with `'='`, then base32-decode (case-insensitive) to get JSON:
  - `{ rigName, parts, rig }`
  - `rig = { jname, transform, jointtransform0, jointtransform1, aux: [meshNames], children: [...] }`
- Fields, per Cautioned `src/.../Components/RigPart.lua`:
  - `jname = part.Name`.
  - `transform = part.CFrame:GetComponents()`, in world space.
  - `jointtransform0 = joint.C0` and `jointtransform1 = joint.C1`. These are absent for the root.

**Blender → Studio ("Export animation" / Bake): verified from source**

```
clipboard = base64( zlib.compress( utf8( json.dumps(obj, separators=(",",":")) ), level 9 ) )
obj = {
  "t":   <float seconds = (frame_end - frame_start) / fps>,   // duration
  "kfs": [ { "t": <seconds since frame_start>,
             "kf": { "<PartName>": [x,y,z, r00,r01,r02, r10,r11,r12, r20,r21,r22], ... } }, ... ]
}
```

- The component order is the same as `CFrame:GetComponents()` and `CFrame.new(unpack(c))`, with rows of the rotation matrix. Source: `mat_to_cf`.
- Keys are bone names, which are Roblox **Part1** names (jname). The root bone (HumanoidRootPart) is never emitted.
- Integers are emitted as JSON ints.
- Rounding to 4 decimals exists but is off by default (`cf_round = False`).
- **Identity values are omitted.** `if statel != identity_cf: state[name] = statel`. In the v1.96 full bake there is one keyframe per frame, stepping by `frame_step`.
  - An importer should therefore treat a missing bone in a baked frame as **identity**, not as "no key".
  - The modern Cautioned Studio plugin instead treats a missing bone as "no key" and leaves that bone to interpolate. That is a behavioural difference worth knowing about.
- The "Optimized Bake" option (`ignore_unchanged_keyframes`) only emits a frame when the state changes, plus a held copy one frame before the change. This keeps constant segments correct under linear interpolation.
- **The per-bone CFrame is the Motor6D.Transform (that is, Pose.CFrame).** Derived from `serialize_animation_state`:
  1. `orig = (Pp·Cp1)⁻¹(P·C1)` and `cur = (Pp'·Cp1)⁻¹(J')`.
  2. The emitted value is `orig⁻¹·cur`.
  3. With `P1 = P0·C0·T·C1⁻¹` this reduces exactly to `T`.
  4. Blender's Z-up frame is undone with `axis_conversion(from_forward="Z", from_up="Y", to_forward="-Y", to_up="Z")`.
- Studio side (Cautioned `AnimationSerializer.lua`): strip whitespace, base64-decode, inflate with zlib (`DeflateLua.inflate_zlib`), then `HttpService:JSONDecode`. For each kf entry: `Pose{Name=PartName, CFrame=CFrame.new(unpack(c))}`, nested by the rig hierarchy, with Weight 0 on structural parents.

**Extensions in v3.x** (Cautioned `Components/Rig.lua` `Rig:LoadAnimation` and `animation/serialization.py`; a parser should accept all of them)
- A pose value can take three forms:
  - The legacy flat 12-number list.
  - `[[12 comps], "EasingStyle"?, "EasingDirection"?]`.
  - `{components:[…], easingStyle?, easingDirection?}`.
  - Style and direction strings are `PoseEasingStyle` or `PoseEasingDirection` names. The defaults are Linear and Out.
- `"<Name>_deform"` keys mark deform bones.
- Top-level additions:
  - `is_deform_rig` or `is_deform_bone_rig`.
  - `bone_hierarchy: {bone: parent|null}`.
  - `export_info{fps, fps_base, frame_start, frame_end, frame_step, time_unit: "seconds"|"frames", …scale fields}`. When `time_unit == "frames"`, the times are frames divided by fps.
- Per-keyframe `fc` holds face-control (FACS) values.
- The v3 plugin normalizes the rotation columns on import.
- Blender → Roblox easing mapping in v3 (`easing.py`):
  - LINEAR → Linear, CONSTANT → Constant, CUBIC → CubicV2, BOUNCE → Bounce.
  - EASE_IN → In, EASE_OUT → Out, EASE_IN_OUT → InOut.
  - AUTO → Out for Bounce and Elastic, In for Cubic.
  - Any other interpolation, and Bounce with InOut, is baked as Linear.
- v3 also has a live-sync HTTP/WebSocket server inside Blender on port 31337. This is out of scope.

### 7c. Rust parsing options

Crate data is from crates.io as of 2026-10-11.

| Crate | Latest | License | Notes |
|---|---|---|---|
| `fbxcel` 0.9.0 | 2022-10 | MIT OR Apache-2.0 | Low-level pure-Rust FBX binary parser (tree). **Already a dependency** in `crates/rbx_import`. |
| `fbxcel-dom` 0.0.10 | 2023-02 | MIT OR Apache-2.0 | "Highly experimental". Binary FBX 7.4/7.5 only, no ASCII, read-only. Covers geometry, materials and textures. **No animation-curve helpers** (nothing in `src` mentions animation). |
| `ufbx` 0.11.5 | 2026-09 | MIT OR Unlicense | Bindings to the single-file C library ufbx, built with `cc`. Not pure Rust. Full support for skinning, animation stacks and layers, and baking. The most practical FBX animation reader. |
| `gltf` 1.4.1 | 2024-05 | MIT OR Apache-2.0 | Pure Rust. **Already a dependency** in `crates/rbx_import` with `default-features=false, features=["names"]`. Reading animation samplers needs the **`utils`** feature (`animation::Channel::reader()` → `read_inputs` and `read_outputs` give rotations, translations, scales and morph weights). Interpolation is LINEAR, STEP or CUBICSPLINE. |

**Recommendation**
- glTF (`.glb`) is the most practical route for Blender exports. Blender's glTF exporter is built in, uses metres, and is Y-up (the exporter's "+Y Up" option is on by default; **UNVERIFIED** here). It produces TRS per node with clean sampler data.
- To turn glTF into Poses:
  1. Convert each joint's local TRS into a delta relative to its rest pose, mapping the joint's local frame to Roblox joint space with C0 and C1.
  2. Scale translations from metres to studs. The physics units table in `CD:physics/units.md` gives 1 stud = 28 cm, but the same page says the engine "does not use unit conversions". The scale Studio's importer actually applies to glTF and FBX is **UNVERIFIED**.
- Supporting a paste of the Rbx-Animations clipboard format needs only `base64` (already in the workspace) and a zlib inflater (`flate2` or `miniz_oxide`). It produces Pose CFrames directly, with no axis math.

---

## 8. Key UNVERIFIED items and how to settle them
1. **PoseEasingDirection runtime meaning for CubicV2, Elastic and Bounce.** Do In and Out match TweenService or not? Test with `Animator:StepAnimations` on a one-joint rig.
2. **Missing Pose handling and Weight-0 structural Poses.** This is community evidence only.
3. **Motor6D formula order** `P0·C0·T·C1⁻¹`. This is the universal convention and is consistent with the addon's math, but no doc quotes it.
4. **AnimationConstraint IsKinematic default.** The API dump gives `false` for `Instance.new`, while the docs say "true (default)" for upgraded rigs.
5. **AuthoredHipHeight default.** The docs give 1.35 and the API dump gives 2.
6. **Animation Graph node property storage** (attributes versus ObjectValues), and the meaning of `AnimationNodeTransitionType` (CrossFade, InertialBlend, DeadBlend). The DevForum beta post 4554788 was not readable from this sandbox.
7. **Animation Editor IK modes and settings.** Not covered by the current docs repo.
