# Local Run

Local Run is our own Roblox runtime. You hit Run and the place's scripts and physics go, on your machine, with no Roblox client, no Sober, no network. It's clean room: we write every line ourselves, from the docs and from watching the real client.

This file is the plan, the rules, the sources, and where we're at. Milestones 1 to 3 are done: phase 0 and phase 1, Run in Studio, then humanoids, a local player, avatars, animation and Play (local). What's left and the proposal for Milestone 4 are at the end.

## The rules

These aren't negotiable. Everyone working on Local Run, human or agent, follows them.

- We never write proprietary code. No copying, translating or paraphrasing Roblox's code: not the engine, not CoreScripts, not PlayerModule, not Animate, not any Lua Roblox wrote. If a Roblox script does something we need, we read the docs for what it should do and write our own.
- No leaked sources, no "revival" projects, no decompiled engine code. Not even to peek.
- Reverse engineering is only for watching behaviour or learning about things the docs don't cover (an undocumented property, an event order). We run Lua in a real server and look at what it prints. We don't disassemble the engine to copy how it does something.
- Sources we can use: create.roblox.com and the creator-docs repo (CC-BY-4.0, credit them), the API dump, Luau (MIT), observation of the real client, and MIT-compatible open source. MPL-2.0 is OK only as an unmodified crate dependency. No GPL, no AGPL, anywhere in the tree.
- Live tests use Chee's own config and key through the built binary. We never print the key, and we never upload anything except to the sandbox place in his settings (universe 1113565074, place 3086963177).
- Numbers in our docs are measured. Goldens come from a real server or they say `unrecorded`. We don't make them up.

## Sources

| Source | License | What we use it for |
|---|---|---|
| [create.roblox.com](https://create.roblox.com/docs) / [creator-docs](https://github.com/Roblox/creator-docs) | CC-BY-4.0 | API semantics, RunService frame order, task library, material physics table (`rbx_materials::physics`), adaptive timestepping, collision groups, deferred events |
| `assets/API-Dump.json` (Roblox's API dump, synced nightly to dev) | Roblox's published API description, used as data | Classes, members, tags (`NotScriptable`, `Service`), defaults through `rbx_reflection`, `COVERAGE.md` |
| [Luau](https://github.com/luau-lang/luau) | MIT | The VM (through `luau0-src`) |
| [mlua](https://github.com/mlua-rs/mlua) 0.12 | MIT | Rust bindings to Luau |
| [Rapier](https://rapier.rs) `rapier3d-f64` 0.36 | Apache-2.0 | Rigid bodies, contacts, joints (the f64 build: test places sit thousands of studs out, where f32 positions lost whole substeps of motion) |
| [parry3d](https://parry.rs) `parry3d-f64` 0.31 | Apache-2.0 | Collision shapes, ray and shape casts, and its V-HACD port for `CollisionFidelity` decompositions (through Rapier) |
| Our `rbx_mesh`, `rbx_assets` | ours | `MeshPart.MeshId` geometry from the local asset cache |
| glam / glamx | MIT OR Apache-2.0 | Math (through Rapier) |
| [rbx-dom](https://github.com/rojo-rbx/rbx-dom) (our `rbx_dom`, `rbx_binary`, `rbx_xml`, `rbx_reflection`) | MIT | Loading .rbxl/.rbxlx, the instance tree, reflection |
| [Lune](https://github.com/lune-org/lune) | MPL-2.0 | Prior art only. We looked at how it shapes a Roblox-like runtime; no code taken, not a dependency |
| Real Roblox server, observed via `rbx_play` | Observation | The conformance goldens (`crates/rbx_sim/conformance/goldens/`) |
| `avatar.roblox.com/v1/users/{id}/avatar`, `users.roblox.com` (names) | Public web API responses | The signed-in player's avatar for Play (local), `GetHumanoidDescriptionFromUserId`; cached under `cache_root()/avatars/` |
| Roblox asset delivery (anonymous, the Open Cloud key as fallback) | Public web API responses, fetched at runtime | Worn assets and the default animation ids, loaded at runtime into the local caches. No animation or avatar file is in the tree |
| Our own rig, avatar import and auto-rig code (`rbx_studio::shell::rig`, `rbx_autorig`) | ours | The R15 body an avatar is dressed on; the noob is our own R6 blocks (`rbx_sim::character`) |
| [Recast](https://github.com/recastnavigation/recastnavigation) | Zlib | Later, for PathfindingService (phase 3). Not used yet |

## How it fits together

```
place.rbxl ──> rbx_sim::load ──> WeakDom (shared, Rc<RefCell>)
                                   │
              ┌────────────────────┼─────────────────────┐
              v                                          v
   rbx_lua::server::ServerVm                    rbx_sim::physics::World
   (Luau, our engine: signals,                  (Rapier: one rigid body per
    scheduler, instances, services)              assembly, colliders per part)
              │                                          │
              └──────────── rbx_sim::Simulation ─────────┘
                       the 60 Hz frame loop, change routing
```

- `crates/rbx_lua` already had the engine the Command Bar and the plugin host run on (instances, signals, the task scheduler, require, services). Local Run adds `rbx_lua::server`: a VM with a server host (sim clock, frame-based waits, newest-first signals), server hooks (RunService, Touched/TouchEnded, PhysicsService, BasePart mass and impulses, HttpService JSON, CollectionService, WaitForChild) and a tiny `Physics` trait the sim implements. The changes to the shared engine are opt-in host flags (`frame_clock`, `newest_first`, `default_deferred`) and one hook (`after_write`), so the Command Bar and plugin host behave as before, and their tests pass.
- `crates/rbx_sim` owns the loop. It reads the DOM change log after every bit of Lua and routes it: a structural change (size, anchored, a new weld) rebuilds the physics world before the next step, a CFrame write teleports a body, a velocity write sets it, a new Script in a server container starts.
- Physics writes go back to the DOM without logging changes, so moving parts don't fire `Changed` for every frame. That matches Roblox: physics doesn't fire `Changed` for CFrame either.

### Which scripts run

`Script`, not disabled, and (since Milestone 3) `LocalScript`s that run on a client: under the local player (`PlayerScripts`, `PlayerGui`, `Backpack`) and its character. Local Run is one VM for both sides, so `Players.LocalPlayer` answers server scripts too. `RunContext.Legacy` runs under Workspace and ServerScriptService. `RunContext.Server` also runs under ReplicatedStorage. ModuleScripts run when required. That's straight from the RunContext docs.

### The frame

Measured on a real server (the RunServiceOrder golden), one frame is:

1. `PreAnimation`
2. `Stepped(time, dt)`
3. `PreSimulation`
4. physics (four 240 Hz substeps)
5. `PostSimulation`
6. resume threads whose wait is over (`task.wait`, `task.delay`, `wait`)
7. `Heartbeat`
8. `Touched`/`TouchEnded` from this step (the TouchedOrder golden shows them after Heartbeat)
9. the deferred queue (`task.defer`, deferred events)

The docs list the events but not where waits resume, and they say `Stepped` and `PreSimulation` are the same moment. On a real server `Stepped` fires first. We go with the server.

### Time

- 60 frames a second, fixed. Server `dt` is 1/60. Source: the task library and RunService docs, and every frame delta in our goldens.
- Physics at 240 Hz, four substeps a frame. Source: the adaptive timestepping docs ("By default, Roblox simulates physics at 240 Hz").
- Adaptive stepping: the same docs say easy assemblies go on a 60 Hz island but not how one is picked. The FreeFallTrace and FreeFallRates goldens (cubes of 1, 2, 4 and 8 studs, dropped, thrown down, sideways and up, position and velocity every frame) pin it down for a free body:
  - the frame after it's built it steps at 240 Hz;
  - then 60 Hz semi-implicit (one gravity kick a frame, the substeps coast);
  - while gravity speeds it up it moves to 120 Hz past T120 and to 240 Hz past T240, speeds that grow with its size s (largest side): T120 ≈ 29.86 s + 15.14, T240 ≈ 91 s + 49 stud/s (fitted brackets: s1 T120 in (42.5, 45.8], s2 (73.7, 75.2], s4 (134.1, 137.3], s8 (251.8, 255.1]; T240 s1 (139.0, 142.3], s2 (230.5, 231.3]);
  - a body slowing down keeps its rate (thrown up at 80 it stayed at 60 Hz past 75);
  - each switch up adds one kick of the new rate (60 to 120 adds g/120, 120 to 240 adds g/240). That kick is the "half frame" Milestone 1 couldn't explain: FreeFall's vy at 30 frames is -99.735, not -98.1.
  - Anything in contact or held by a joint constraint steps at 240 Hz. `PhysicsSteppingMethod = Fixed` turns all of it off.
  - Not explained yet: thrown sideways at 60 stud/s, a 2 stud cube switched to 120 Hz at 65.5, earlier than straight down.
- Waits resume on the first frame strictly past their time: `task.wait(t)` takes floor(60 t) + 1 frames (WaitRounding golden: 0.05 → 4, 0.1 → 7, 0.5 → 31, 1 → 61, 2 → 121), `task.wait(0)` 1, legacy `wait()` 2 (its 0.03 s minimum). `task.delay` follows the same rule; at exact frame boundaries Roblox is a coin flip between two frames. The real server's frames are a hair under 1/60 s (121 beats sum to 2.01658 s), so elapsed times differ in the fourth decimal.

### Signals

- Deferred is now what a live server does with `SignalBehavior` left at `Default`: `Fire` and a property write queue their handlers, which run at the end of the resumption point, Fire's before the write's. Golden (recorded 2026-11, place version 537): `after_fire after_set handler changed`. In Milestone 1 (version 535, a few weeks earlier) the same test printed the immediate order, so Roblox flipped the default for live servers in between. rbx_sim follows: `Default` means Deferred on the server host; Studio's edit runtime keeps Immediate. Our deferred queue runs a resumption point's queued handlers before the instance events of its writes, which is right for this golden but not for a write followed by a `Fire`.
- Handlers run newest connection first, and a `:Wait()` counts as a connection. Golden: connect A, B, then Wait W, then C, fire: `C W B A`.
- `Workspace.SignalBehavior` is `NotScriptable`: reading it errors on a real server, so it does in ours. A place file that stores Immediate or Deferred gets that.

### Physics

| Roblox | rbx_sim | Notes |
|---|---|---|
| Block | cuboid | exact |
| Ball | ball, diameter = smallest side | exact for a cube part |
| Cylinder | cylinder along X | exact |
| Wedge | convex hull, vertical face at +Z | exact |
| CornerWedge | convex hull, apex above +X -Z | vertex layout read off the docs, not yet checked against Studio |
| Capsule (Shape) | capsule along X | the docs don't give the axis, X like the cylinder is a guess |
| MeshPart | per `CollisionFidelity`: Box = the mesh's bounding box, Hull = convex hull, Default = V-HACD (resolution 32, up to 16 hulls), PreciseConvexDecomposition = V-HACD (resolution 64, concavity 0.0025, up to 64 hulls) | geometry from an `EditableMesh` (`AssetService:CreateEditableMesh` / `CreateMeshPartAsync`) or `MeshId` from the local asset cache, never the network; without geometry it's its `Size` box. Roblox's own decomposition parameters aren't documented, so ours are picked to keep the MeshCollision golden's gap (Default and Precise leave the gap between two cubes open, Box and Hull fill it) |
| UnionOperation, Terrain | `Size` box / not simulated | union geometry decoding lives inside `rbx_viewer` (Milestone 3); terrain from `rbx_terrain` (phase 3) |
| HingeConstraint (free, Motor, Servo, limits) | Rapier revolute joint on the attachments' X axes, velocity motor, 60 Hz softness | a servo turns at `AngularSpeed` straight to its target (HingeServo golden) |
| PrismaticConstraint | Rapier prismatic joint, velocity motor | |
| BallSocketConstraint | Rapier spherical joint, 60 Hz softness | stretches 0.01 stud at the bottom of a swing where Roblox's stretches 0.046 |
| RopeConstraint | Rapier rope joint | the body falls on adaptive steps while the rope is slack |
| SpringConstraint | Hooke's law plus damping between the attachments, every substep | |
| AlignPosition, AlignOrientation | critically damped pull, natural frequency `Responsiveness`/2, holds against gravity | fitted to the goldens (both follow the same curve); `RigidityEnabled` stands in as Responsiveness 200 |
| LinearVelocity, AngularVelocity | reach the target within a substep as far as `MaxForce`/`MaxTorque` allow | LinearVelocity holds against gravity |
| VectorForce, Torque | a force every substep; on a free body on adaptive steps, VectorForce pushes with the gravity kicks (same rate, same switch kick) | VectorForce golden: 10.167 after 30 frames, as the clean passes |
| Attachment | `WorldPosition`, `WorldCFrame`, `Axis`, `SecondaryAxis` and the rest read and write | |
| Restitution | Rapier's, but an impact (approach above 1.5 stud/s on several contact points) is solved at one point, so a flat box bounces straight. A free body (on adaptive steps) goes through what it meets for that frame (inert contacts, Touched still fires), then is pushed back out along its way in and, above 6 stud/s, takes 0.75 of its free frame's kick before it bounces | BoxBounce golden: e ≈ 0.5; TouchedOrder bounce for bounce |
| Angular damping | 0.052 /s, exponential | Damping golden: 10 rad/s → 9.019 after 2 s; no linear damping |
| Anchored | fixed body | |
| Spatial queries | `Raycast` (RaycastParams: filter Include/Exclude, `RespectCanCollide`, `CollisionGroup`), `Blockcast`, `Spherecast`, `Shapecast`, `GetPartBoundsInBox`, `GetPartBoundsInRadius`, `GetPartsInPart` (OverlapParams), legacy `FindPartOnRay*` | against the same shapes physics collides with. Measured: a ray ignores the part it starts in, `CanQuery` false is never hit, casts ignore parts they start inside, the bounds queries test a part's box (a ball counts as its cube), `GetPartsInPart` is exact |
| CanCollide false | sensor collider | still fires Touched when CanTouch |
| CanTouch | collision events on or off | both parts need it, as documented |
| CollisionGroups | 32 groups as Rapier interaction bitmasks | `PhysicsService` API, Default always there |
| Welds, WeldConstraint, Motor6D, other JointInstances | one rigid body per assembly | union-find over joints, root part = biggest volume. Motor6D is rigid (no animation yet) |
| Material, CustomPhysicalProperties | density, friction, elasticity and their weights | weighted mix in a contact hook, Roblox's documented formula; two weights of 0 give 0 (SurfaceMix) |
| Contacts | Rapier's springs, 25 Hz between two moving bodies | a stack of five sinks like Roblox's (BoxStack) |
| Anchored part's AssemblyLinearVelocity | a conveyor: contacts on it get that tangent velocity | Conveyor golden |
| Mass, AssemblyMass, GetMass | from density × volume, read fresh from the DOM | DensityMass golden |
| AssemblyLinearVelocity, AssemblyAngularVelocity (and Velocity, RotVelocity) | read and write | |
| ApplyImpulse, ApplyAngularImpulse | applied before the next step | |
| Workspace.Gravity | yes | default 196.2 |
| FallenPartsDestroyHeight | parts below it are destroyed (Destroying fires) | default -500 |
| Network ownership | no-ops | one server, no clients |

## Conformance

The suite is 74 Luau tests in `crates/rbx_sim/conformance/tests/` (shared helpers in `Harness.luau`). The same modules run in a real Roblox server and in rbx_sim and print one JSON line each. Each test starts right after a Heartbeat, at its own origin (`2000 + 300 i, 300, 2000`) so tests never touch each other. Tests with a client half (Milestone 5) call `Harness.client(name, ...)`, which runs the module `name` from `conformance/client/` on the player's client (a LocalScript in StarterPlayerScripts answers a RemoteFunction) and returns what it returns; a test that needs real input prints `CONFORMANCE_GESTURE wheel in 1` / `drag 100 0` / `key LeftShift`, which the recorder turns into XTest input on the Sober window and rbx_sim into Session input.

Recording goldens (`cargo run -p rbx_sim --example record_goldens -- <universe> <place> <base.rbxl> <cloudflared> [--passes N] [--only A,B]`) adds the suite to a base place, publishes it to the sandbox place, restarts its servers, joins with Sober on Xephyr, collects the server's prints through the `rbx_play` probe, and writes `goldens/*.json`: the median, the spread across passes and every raw pass. rbx_sim runs the same passes and takes the median.

The real server isn't deterministic. On the Milestone 2 recording (place version 537, five passes) passes 1 to 3 usually agree with each other and passes 4 and 5 with each other, and the difference is the first frames of a test: the early passes take extra substeps there (six or seven instead of four), the later ones don't. rbx_sim models the clean behaviour. So a row passes when it's within the test's tolerance or Roblox's own spread of the median, or, marked `ok (pass N)` in the table, within tolerance of one recorded pass. Fields starting with `_` are traces kept for working things out (per-frame positions, frame times) and aren't compared.

`cargo test -p rbx_sim` checks every recorded golden. A golden can carry a `known_gap` explaining why it's off; that's reported, not failed. `cargo run -p rbx_sim --example conformance -- --write docs/research/local-run/CONFORMANCE.md` writes the full table (`CONFORMANCE.md`).

What changed in Milestone 2, besides the 25 new tests:

| Test | Milestone 1 | Milestone 2 |
|---|---|---|
| FreeFall vy | ok on spread only, a steady 1.6 stud/s (half a frame of g) behind | matches the clean passes: the half frame is the 60 → 120 Hz switch kick |
| TaskTiming / WaitRounding | task.wait(0.5) one frame short | strict "first frame past the time" rule, every wait from 0.05 to 2 s exact in frames |
| TouchedOrder | matches to the first bounce, then the block spins | bounces straight; still a frame early per bounce (soft contacts, see known gaps) |
| SignalBehavior | immediate | the server now defers by default, and so do we |
| RestingContact, FrictionSlide, ElasticBounce, RunServiceOrder | ok | ok |

Known gaps (the details are in each golden's `known_gap` and in CONFORMANCE.md):

- **Soft contacts.** Roblox's contacts give: a stack of five sinks 0.16 to 0.25 stud at the top, a bouncing block stays in contact a frame longer. Rapier holds every contact at the same tiny penetration and resolves an impact within the frame. That's BoxStack and the one-frame-per-bounce drift in TouchedOrder.
- **Soft joints.** Roblox's BallSocket stretches about 0.03 stud on a 4 stud arm and its swing drifts from ours after 40 frames; HingeFree is 0.14 stud off in Y at 30 frames. Ours are rigid.
- **VectorForce** gets one extra frame of push on Roblox (vx 10.167 where 30 frames of 20 stud/s² give 10.0).
- **Sideways adaptive switch.** A 2 stud cube thrown sideways at 60 stud/s switches to 120 Hz earlier than the speed rule says (FreeFallRates `side60`, inside tolerance today).

What Milestone 3 added: 18 tests (17 humanoid ones, PlayersCharacter, AnimationTrack), recorded with five passes on place version 540. Every field that matches is in CONFORMANCE.md; what doesn't is below, all written into the goldens' `known_gap`:

| Test | Field | Roblox | ours | delta |
|---|---|---|---|---|
| HumanoidTurn | back_1 / back_3 / back_5 (degrees) | 72.43 / 31.31 / -0.46 | 64.65 / 25.62 / -3.55 | -7.8 / -5.7 / -3.1 |
| HumanoidJump | high_apex (JumpHeight 15) | 15.479 | 15.278 | -0.20 |
| HumanoidJump | high_vy1 | 75.09 | 74.27 | -0.82 |
| HumanoidSlope | s30_v45 | 0,7.84,13.86 | 0.02,7.67,13.86 | y -0.17 |
| HumanoidSlope | s60_rise / s60_dz | 12.78 / 16.08 | 7.98 / 16.93 | -4.8 / +0.85 |
| HumanoidSlope | s60m_v45 (MaxSlopeAngle 45) | 0,-1.72,-3.55 | 0.56,0.38,1.09 | slides back vs creeps up |
| HumanoidWalkScale | heavy_vx1 | 15.68 | 12.17 | -3.5 |
| HumanoidWalkScale | heavy_yaw1 / ws50_yaw1 | 8.28 | 12.18 | +3.9 |
| HumanoidClimb | r15_states | Climbing@15 | Climbing@16 | +1 frame |
| HumanoidClimb | r6_states | Climbing@18 | Climbing@19, then a Freefall frame every few frames | flicker |
| HumanoidSeat | states (landing after the jump off) | Landed@48 | Landed@49 | +1 frame |

No regressions in the Milestone 2 tests: the same five carry their known gaps (TouchedOrder, HingeFree, BallSocket, VectorForce, BoxStack).

How the humanoid works (`crates/rbx_sim/src/humanoid.rs`, `physics/character.rs`): the root is a dynamic Rapier body stepped at 240 Hz whatever the adaptive rate. A thin box cast down from the root (60% of its footprint, `FOOTPRINT`) finds the floor; a spring (`HIP_SPRING` 3500, damping 110, fitted to HumanoidStand and HumanoidDrop) holds the root at HipHeight plus half its height, with gravity off once it has settled, so it rests instead of chattering. Walking steps the velocity towards `WalkSpeed` along the floor by 0.625 of the gap per substep, capped by friction (fitted from HumanoidWalk, HumanoidWalkScale); turning is 3.57% of the angle per substep. The state machine (17 states, the enable flags, `StateChanged` and the per-state signals) runs before physics: Freefall 7 frames after the floor goes, Landed for 2, the floor sensor reaches 1.1 studs past standing height. `MoveTo` arrives within 1.04 studs, checked before physics, and gives up after 480 frames. Health, `TakeDamage` (a ForceField blocks it), `Died`, `BreakJointsOnDeath`, `RequiresNeck` and our own Health regen script are there; respawn waits `Players.RespawnTime`. Seats weld on `Sit` or touch, a jump gets off, and a seat doesn't take back someone who just left it for a second.

## Coverage

`docs/research/local-run/COVERAGE.md` is generated (`cargo run -p rbx_sim --example coverage -- docs/research/local-run/COVERAGE.md`): a server script in a fresh Local Run makes an instance of every class and indexes every member a game script may touch. Today: 372 of 1385 functions (87 after Milestone 1, 190 before Milestone 12, 281 before Milestone 13), 134 of 355 events, 1949 of 2510 properties. The property number flatters us: any property the reflection database knows reads back as data, whether or not we act on it. The physics table above is what's really simulated.

## Roblox updates every week

- **API surface.** Everything class-shaped is generated from the API dump, which syncs nightly to dev: reflection defaults, `NotScriptable`, services, and `COVERAGE.md`. A new property reads and writes as data the day it lands. Regenerate COVERAGE.md after a dump bump and the diff says what's new.
- **Luau.** We get it through mlua and `luau0-src`. A Luau release is a dependency bump, then `cargo test -p rbx_lua -p rbx_sim`.
- **Behaviour drift.** Roblox changes behaviour without telling anyone (frame order, signal order, solver tweaks). We re-record the goldens weekly with `record_goldens` against the sandbox place, and the diff in `goldens/*.json` is the drift. Then `cargo test -p rbx_sim` says if we still match.

## What we reuse

- `rbx_lua`: the whole script engine (instances, datatypes, signals, scheduler, require, services, output). Local Run is a host on top of it.
- `rbx_materials`: the material physics table moved here from Studio's computed properties, so Studio and the sim read one copy.
- `rbx_reflection`, `rbx_dom`, `rbx_binary`, `rbx_xml`: loading and the instance tree.
- `rbx_viewer` (phase 2): the renderer for a live Local Run viewport.
- `rbx_terrain` (phase 3): smooth terrain voxels into a Rapier collider.
- `rbx_animation`: Animator and AnimationTracks driving Motor6D transforms (Milestone 3).
- `rbx_studio::shell::rig`: the avatar import (Insert › Rig) builds the signed-in player's character for Play (local).
- `rbx_play`: the probe and the tunnel that record goldens.

## Phases

Every phase ends with acceptance tests. "Golden" means recorded from a real server and checked by `cargo test -p rbx_sim`.

### Phase 0: foundations (done)

Headless sim crate, server VM, conformance harness with real goldens, coverage generator, beta switch.

Acceptance:
- `cargo run -p rbx_sim --example run -- place.rbxl --seconds 5` loads a place, runs its server scripts and physics, prints output and where the unanchored parts ended up. Done.
- `cargo test -p rbx_lua` and `-p rbx_sim` pass, Command Bar and plugin host tests included. Done.
- Goldens recorded from a real server for all eight tests. Done.

### Phase 1: server scripts and rigid physics (done, with known gaps)

Acceptance:
- RunService frame order matches the golden. Done.
- task.wait / wait / delay / defer / spawn timings match the golden within one frame. Done.
- Free fall, resting contact, friction slide and bounce within tolerance. Done.
- Touched/TouchEnded order matches the golden all the way through. The bounce spin is fixed; a bounce still ends a frame early (soft contacts), Milestone 3.
- Raycast, Blockcast, Spherecast, Shapecast, GetPartBoundsInBox/Radius, GetPartsInPart, OverlapParams: results match a golden scene. Done (Raycast, Shapecast, Overlap goldens).
- Constraints (HingeConstraint free/motor/servo, BallSocket, Rope, Spring, Prismatic, AlignPosition, AlignOrientation, LinearVelocity, AngularVelocity, VectorForce): a golden per constraint. Done; BallSocket at 40 frames, HingeFree's Y and VectorForce are known gaps.
- MeshPart collision per CollisionFidelity. Done (MeshCollision golden). Unions are still their Size box, Milestone 3.

### Phase 2: in Studio (started)

Acceptance:
- Run (local) in the ribbon runs the open place live in the viewport through `rbx_viewer`, with Stop restoring the edit DOM untouched (golden: a place saved before and after Run is byte-identical). Done behind the `local-run` beta: Test › Run, Pause, Stop (and Model › Run (local)). The unit test compares the whole tree, properties included, before the run and after Stop; checked live on Xephyr (screenshots in `rbx-native-targets/local-run-m2-shots/`).
- Output, Explorer and Properties show the running DOM while it runs, like Studio's Run. Done: the Output streams script prints, the Explorer shows parts scripts add.
- Pause, step one frame, resume. Pause and resume done; step one frame is Milestone 3.

How it works (`crates/rbx_studio/src/shell/local_run.rs`): Run clones the edited DOM, starts `rbx_sim::Simulation` on the clone and swaps a second clone into `Shell::dom`, stashing the edited place, its undo history and selection (the pattern Play's live views use). A foreground timer steps the sim on wall time at 60 Hz (at most 4 frames a tick). Each frame's changes (`Simulation::take_view_changes`: what scripts logged plus a CFrame write for every moving part) are mirrored into the shown DOM by referent and handed to the viewport, Properties and Explorer. Edits made during the run go into the sim's DOM as logged changes. Saving during a run saves the stashed place. Stop drops the sim, sends the viewport the changes back to the edited place and restores DOM, history and selection.

### Phase 3: characters and the world

Acceptance:
- Humanoid: spawn on a SpawnLocation, walk, jump, fall, die, respawn. Goldens for jump height, walk speed and time to land.
- Smooth terrain collision from `rbx_terrain`, golden: a ball rolling down a terrain slope.
- Animator and AnimationTrack driving Motor6D from `rbx_animation`, golden: a track's joint transforms at fixed times.
- PathfindingService on Recast, golden: path waypoints over a test map within a stud.

### Phase 4: clients

Acceptance:
- A local client VM running LocalScripts against a replicated copy, RemoteEvent/RemoteFunction, golden round trip order.
- Our own clean-room PlayerModule-equivalent (camera and controls written from the docs, not Roblox's scripts): WASD, jump, mouse look.
- Play (local) with one player, then several on one machine.

### Phase 5: the long tail

Acceptance:
- DataStoreService and MemoryStoreService backed by a local file, matching documented limits and errors.
- Sound, particles and GUI in the viewport.
- Coverage: 50% of functions and events, tracked weekly in COVERAGE.md.

## Milestone 2 (done)

1. Restitution: impacts are solved at one point, so a flat box bounces straight (BoxBounce golden); TouchedOrder matches the bounces' flights but each bounce ends a frame early.
2. The FreeFall velocity half frame is the adaptive rate switch kick; adaptive stepping is now 60/120/240 Hz with size-dependent speed thresholds (FreeFallTrace, FreeFallRates). The long `task.wait` rounding is "first frame strictly past the time" (WaitRounding).
3. Spatial queries: Raycast, Blockcast, Spherecast, Shapecast, GetPartBoundsInBox/Radius, GetPartsInPart with RaycastParams/OverlapParams (Raycast, Shapecast, Overlap goldens).
4. Constraints and movers with a golden each, attachments and their world frames (Attachments golden).
5. MeshPart collision per CollisionFidelity (MeshCollision golden, meshes from `AssetService:CreateEditableMesh` or the asset cache).
6. More goldens: SleepWake, BoxStack, Damping, RampSlide.
7. Run (local) live in the editor viewport, behind the `local-run` beta.
8. Physics in f64 (`rapier3d-f64`): at the test origins, thousands of studs out, f32 positions lost about 6% of a moving part's travel.
9. Server signals default to Deferred, as the real server now does.

## Milestone 3 (done)

1. Humanoid physics with goldens from a real server: standing (HipHeight, the floor sensor, step-up), walking (WalkSpeed, acceleration, `Move`, `MoveTo`, `MoveToFinished` and its 8 s timeout), AutoRotate turning, jumping (JumpPower, JumpHeight, UseJumpPower), freefall and landing, climbing TrussParts, Seat and VehicleSeat, the state machine with `StateChanged` and the enable flags, Health, MaxHealth, `TakeDamage`, `Died`, BreakJointsOnDeath, RequiresNeck, R6 and R15, PlatformStand, Sit. Swimming waits for terrain water in the sim (Milestone 4).
2. The Players service with a local player: CharacterAutoLoads, `LoadCharacter`, RespawnTime, SpawnLocation choice (enabled, neutral or the team's), the CharacterRemoving / CharacterAdded / CharacterAppearanceLoaded order, StarterCharacter, StarterCharacterScripts and StarterPlayerScripts copied (PlayersCharacter golden).
3. Avatars. Signed in (the editor's Open Cloud key names the user): the user's avatar from the public avatar endpoint, dressed on our own R15 rig by the same code as Insert › Rig. Not signed in: the noob, our own R6 blocks in bright yellow, bright blue and yellowish green. Offline: the avatar last cached under `cache_root()/avatars/` (`<user>/avatar.json`, `<user>/user.json`, `assets/<id>.rbxm`, `last_user`), else the noob. `Players:GetHumanoidDescriptionFromUserId`, `Humanoid:ApplyDescription` (accessories, clothing and body colours are swapped in place) and `GetAppliedDescription` work through the same avatars.
4. Animation at runtime: Animator and AnimationTrack (`Play`, `Stop`, `AdjustSpeed`, `AdjustWeight`, linear fades, priorities, `Looped`, `GetMarkerReachedSignal`, `KeyframeReached`, `DidLoop`, `Stopped`, `Ended` and their order, `GetPlayingAnimationTracks`, `GetTimeOfKeyframe`) driving `Motor6D.Transform` (AnimationTrack golden). Our own Animate script (moved to `rbx_sim::character`, shared with Studio's rigs) loads the default ids at runtime from the asset delivery or the asset cache.
5. Play (local) next to Run, behind the `local-run` beta: WASD (ZQSD on AZERTY) and the arrows walk relative to the camera, Space jumps, right-drag orbits the classic third-person camera and the wheel zooms (0.5 to 400 studs). Stop puts the place and the edit camera back. The Output's Local Run rows are game rows now: the "Local Run" tag on the left, the message beside it, like real Play's Server and Client rows.
6. Checked live on Xephyr with the built binary (`rbx-native-targets/local-run-m3-shots/`): signed in, noob (`RBX_STUDIO_LOCAL_PLAYER=noob`) and offline from the cache (every request sent to a dead proxy), each standing, walking, mid-jump, orbited and zoomed, and after Stop.

What's rough: the avatar is our approximation of Roblox's (layered clothing fitted cage to cage, accessories welded where the import puts them), so it doesn't look exactly like the user's in Roblox; the noob and the walk, jump and idle animations look right.

## Milestone 4 (done): avatar, input and camera, offline

1. Avatar fidelity. The cached avatar we build was already right in what it wears: the test account wears an R6-style blocky bundle on R15, a Headless head, a flat cap, face accessories, a floatie and a butterfly aura, which is what Roblox's own thumbnail shows (`local-run-m4-shots/real-thumbnail-avatar.png`). The real bug was accessories left floating in the T-pose while the body animated: the layout after physics now follows every joint (Motor6D with its Transform, Weld, ManualWeld, Snap, Glue, Motor), so handles move with the limbs. `ApplyDescription` now gives the body the description's shape as well as its clothes: body-part meshes, sizes, attachments, joint offsets and the humanoid's scales (BodyType, proportions, height, width, depth, HeadScale) and HipHeight come from a character built from the description. `Players:CreateHumanoidModelFromDescription` returns a new character (rig type argument not honoured yet). `LocalTransparencyModifier` fades parts in the renderer.
2. Input and camera, all ours. Play (local) runs the sim as a client too: `RunService:IsClient()` is true, `UserInputService` (InputBegan/Changed/Ended with gameProcessedEvent, IsKeyDown, GetKeysPressed, IsMouseButtonPressed, GetMouseDelta with MouseDeltaSensitivity, GetMouseLocation, MouseBehavior, GetLastInputType), `ContextActionService` (BindAction, BindActionAtPriority, Unbind*, GetBoundActionInfo, GetAllBoundActionInfo; priority order, newest first among equals, Sink/Pass) and `RunService:BindToRenderStep` work for LocalScripts. Each player gets our own `PlayerModule` (controls bound through ContextActionService at Medium priority, so a game's higher binding sinks them; a camera at RenderPriority.Camera) unless the place ships one. The camera follows from 0.5 to the player's CameraMaxZoomDistance (StarterPlayer limits copied to the Player), zooms with the wheel and I/O, turns on right-drag, is pulled in by walls (raycasts that skip the character, see-through and non-colliding parts), goes first person at minimum zoom with the body faded out, and does shift lock (Shift, when EnableMouseLockOption) with the shoulder offset and the character facing the camera. The editor holds the pointer whenever the game's MouseBehavior asks. Gamepad and touch input are not in: nothing in the tree reads a gamepad (rbx_play only writes one through uinput).
3. Offline mode for the whole app (`crates/rbx_net`). One status: online, detected (a probe of apis.roblox.com at start, every 2 minutes, every 15 s while down, and whenever a request fails to connect) or forced (Settings › Account › Offline mode, or `RBX_NATIVE_OFFLINE=1`). Every HTTP agent in the app is built from `rbx_net::config()`, whose middleware refuses requests to anything but this machine at once while offline (`offline: <reason>` in the error) instead of waiting on a dead network; the relay WebSocket checks the same gate. An "Offline" pill sits in the title bar (hover for why, click for the setting). Publish, Version History, Data Stores, Game Settings and Play (real client) say why they can't open, once, in Output; the Play tile is greyed with "use Play (local)". Assets that aren't cached are summed up in one line instead of one warning each. Local Run works offline with the cached avatar ("Offline: playing as … from the cache").
4. Checked live on Xephyr with the built binary (`rbx-native-targets/local-run-m4-shots/`): walking with the camera following, the wall pull-in, first person, shift lock, the editor in a network namespace with only loopback (detected offline: pill, refusals, Play (local) as the noob because the keyring can't be read from there), and forced offline (Play (local) with the cached avatar, the setting).

What's still open from M4: the camera constants (start distance 12.5, pitch -20°, 0.5° a pixel, the 1.75 stud shift-lock offset, the wheel's 15% a notch, the wall margin) are ours, not measured against the real client; the real-client avatar side by side needs a sandbox variant that loads appearance (the sandbox place spawns a blank R6 in Sober); no shift-lock cursor icon; touch, gamepad and VR; layered clothing still sits as the import puts it; the plugin store and Toolbox show their own fetch errors offline rather than a greyed state; the M3 humanoid leftovers (slope 60°, turn, heavy WalkScale, the Climb/Seat one-frame offsets, swimming) weren't reached.

## Milestone 5 (done): client/server split

1. A server and a client per local player, each its own VM and DataModel in one process (`crates/rbx_sim/src/session.rs`). `Session::join` adds the player on the server and starts their client from a copy of the place's services with only `ReplicatedFirst` in them; its LocalScripts run before the rest arrives on the next frame, then `game.Loaded` fires and the client makes its own `PlayerScripts` (StarterPlayerScripts copied, plus our PlayerModule), which the server never has. A frame is: what the clients sent arrives at the server, the server steps, then each client gets the server's frame and steps. `Simulation` has a role (`Server`, `Client`, or `Solo` for the coverage probe); the client runs LocalScripts under its player and character, in ReplicatedFirst, and RunContext.Client scripts in Workspace and ReplicatedStorage; the server never runs either. `Players.LocalPlayer` is nil on the server; `LoadCharacter` and `ApplyDescription` refuse on a client; `PlayerAdded`, `CharacterAdded`, `CharacterRemoving` and `CharacterAppearanceLoaded` fire on each client from what it sees replicated; `game:IsLoaded()` and `game.Loaded` exist.
2. Replication, our own design (no Roblox protocol), to the rules the goldens measured (ReplicationOrder): the server's changes go out at the end of its frame in the order they were made, each property once per frame where it first changed with the value at the end of the frame; a new instance arrives with its properties and then its children one at a time, each firing its events before the next; an instance made and destroyed in the same frame still arrives and goes; ServerStorage, ServerScriptService and other players' PlayerGui never replicate, and an instance moved into them leaves the client as Parent nil and comes back as the same object. Nothing a client changes replicates, except: the physics of the parts it owns, its humanoids' states, its Animator tracks (Load/Play/Stop/AdjustSpeed/AdjustWeight and track property writes, mirrored on the server and to the other clients). Sound playback doesn't replicate yet.
3. Network ownership (`ownership.rs`, Ownership golden): `SetNetworkOwner`, `GetNetworkOwner`, `GetNetworkOwnershipAuto`, `SetNetworkOwnershipAuto`, `CanSetNetworkOwnership` with Roblox's errors (anchored, welded to anchored, called from a client). A character belongs to its player; auto gives other assemblies to the nearest player within 1000 studs once they've existed two frames, else the server; an explicit owner covers the assembly the part is in when it's called. The owner simulates an assembly; every other peer has it as a kinematic body moved where the owner says (so `Touched` still fires there). The server leaves an owned character's humanoid to its client (state, walk, jump, `Move`/`MoveTo` forwarded to the owner) but keeps health.
4. Remotes (`crates/rbx_lua/src/server/remote.rs`, RemoteOrder and RemoteArgs goldens): RemoteEvent, UnreliableRemoteEvent and RemoteFunction both ways, unreliable events after the frame's reliable traffic, packets interleaved with the replicated changes in the order they were made, `InvokeServer`/`InvokeClient` yielding for the reply. Arguments as Roblox sends them: a table with `[1]` is an array of its positive whole keys (holes stay holes, other keys dropped), any other table a dictionary with string keys (numbers whole, `<Instance> (Workspace)`, `<Vector3> (1, 2, 3)`), a boolean key is "Invalid table key type used", functions and metatables dropped, cycles refused, NaN, inf, -0, 2^53+1, embedded NULs, buffers and datatypes kept, trailing nils counted, an instance the receiver doesn't have arrives as nil. Each side's half errors like Roblox's, and the wrong side's Invoke never returns.
5. Several local players: Test › Players (1 to 4) in the ribbon; you, then Player2.. as noobs. Test › View cycles the server and each player's client; the Explorer, Properties and viewport show that peer's DataModel, and the keys and camera go to the client in view. Output rows carry Server or Client.
6. Camera and input measured on the real client (CameraInput golden, XTest gestures on Sober): start 12.5 studs at -15° (ours said -20°), focus 1.5 above the root, a wheel notch out takes the distance d to 1.5 d + 1 and in the reverse (ours was 15% a notch), 0.5562° of yaw a pixel of right-drag (ours 0.5°), the camera stops 0.1 stud past a wall's face (ours 0.25 before it). PlayerModule uses these now.
7. Polish: the Play tile's offline tooltip is a sentence with no dash; the Account key card says you're offline instead of "Couldn't reach Roblox"; plain app warnings keep their layout (the source column, then the message), the same as every other `push_warning` row on dev.
8. Checked live on Xephyr with the built binary (`rbx-native-targets/local-run-m5-shots/`): two local players on a RemoteEvent demo place, Output printing on both sides with Server and Client tags (`remote-event-both-sides.png`); the same session from the server (`two-players-server-view.png`: Player2 has Backpack and PlayerGui, no PlayerScripts), from Player2's client (`two-players-player2-client-view.png`: its PlayerScripts, no ServerScriptService contents) and from yours (`two-players-your-client-view.png`); the second player stands on the first, as `Model:MoveTo` puts a character on an occupied spawn. The avatar against the real client from the same default camera (`avatar-real-client-sober.png`, `avatar-ours-play-local.png`, `avatar-real-vs-ours-closeup.png`, the place's Lighting at the engine defaults for both). The polish: `play-tooltip-offline.png`, `key-card-offline.png`, `offline-toggle-on-live.png`.
9. The avatar against the real client: the accessories were dull because a textured mesh took the part's colour too (a default-grey handle multiplied its image by 0.64: the floatie's white drew at 133 against the real 210). A MeshPart's image now draws as it is and a SpecialMesh's only takes `VertexColor`; the floatie reads 210,198,203 against 210,207,214. The floatie and the neck/face accessories sit where the real client puts them; the hair and shirt match. What still differs is the place, not the avatar: the real baseplate has the Plastic studs texture, ours is flat.

Gaps from M5 (also in CONFORMANCE.md):

| Test | Field | Roblox | ours |
|---|---|---|---|
| CameraInput | start_distance after a respawn | 11.03 (the camera eases out to 12.5 after a respawn) | 12.5 at once (passes within Roblox's spread) |
| CameraInput | shift lock | not in the golden: the press takes only now and then under Xephyr and the locked mouse turns the camera on its own; where it took, the eye sat 1.75 studs right of the root, mouse LockCenter, which ours does | same |
| CameraInput | pitch per pixel | not recorded: vertical XTest drags gave 0, 0.53 and 1.3° a pixel in three recordings | yaw's rate |
| Replication | Sound playback from a client | replicates unless SoundService.RespectFilteringEnabled | doesn't |
| Replication | physics between peers | interpolated, with the owner's velocity | the owner's position each frame, no velocity |

What's still open from M5: a MeshPart's transparent texels (the real client shows the part's colour through them; ours are see-through, unmeasured); the ownership API's assembly is the one the DOM's joints make when it's called (a later weld doesn't carry the pin); a client's `GetHumanoidDescriptionFromUserId` has no avatar source; the M2/M3 humanoid leftovers (slope 60°, turn, heavy WalkScale, the Climb/Seat one-frame offsets, swimming, soft contacts, the VectorForce frame, union collision) weren't reached again.

## Milestone 6 (done): GUI, tools, teams, players leaving

1. GUI at run time (`crates/rbx_sim/src/gui.rs`, `gui/layout.rs`; GuiRuntime and CoreGuiApi goldens). StarterGui is copied into the PlayerGui each time a character spawns; a `ScreenGui` with `ResetOnSpawn` off is copied only on the first spawn and kept after. The client lays out its screens each frame (`AbsolutePosition`/`AbsoluteSize` under the 58 px top bar unless `IgnoreGuiInset` or `ScreenInsets` None/DeviceSafeInsets, the way `GuiService:GetGuiInset()` reads it), and the editor draws the viewed player's PlayerGui, not StarterGui, while a player is in view. The GUI takes the mouse before the game: GuiButton `MouseButton1Down`/`Up`/`Click`, `Activated`, `MouseEnter`/`MouseLeave`, `InputBegan`/`Ended` on the button, and UserInputService's events after them with `gameProcessedEvent` true over a GUI (ContextActionService doesn't see a sunk click). TextBox `CaptureFocus`, `ReleaseFocus`, `IsFocused`, `Focused`, `FocusLost(enterPressed)`, typing with Shift, Backspace, `ClearTextOnFocus`, `UserInputService:GetFocusedTextBox()`; keys typed into a box are `gameProcessedEvent` true. `StarterGui:SetCoreGuiEnabled`/`GetCoreGuiEnabled`, `SetCore`/`GetCore` (`SendNotification`, `TopbarEnabled`, `ResetButtonCallback`, `ChatActive`, `PointsNotificationsActive`, `BadgesNotificationsActive`), `PlayerGui:SetTopbarTransparency`/`GetTopbarTransparency`, `GuiService:GetGuiInset()` and `IsTenFootInterface()`, with Roblox's errors and a server's `GetCore` waiting forever. Our own core GUI, drawn by the editor over the view (`workspace_view/core_gui.rs`): health pill top right, the player list with team colours, the hotbar (click a slot to equip), notifications, each hidden by `SetCoreGuiEnabled`.
2. Tools (`tools.rs`, ToolEquip golden). StarterPack and the player's StarterGear go into a new Backpack at each spawn. Equipping is the tool's parent: in the character it's equipped (`Equipped(mouse)` after `AncestryChanged`), back in the Backpack `Unequipped` first. The server makes or removes the `RightGrip` weld (to `RightHand` or `Right Arm`, C0 one stud down and turned, C1 the tool's `Grip`); `RequiresHandle` tools without a handle don't equip. `Humanoid:EquipTool`, `UnequipTools`, `Tool:Activate`/`Deactivate`, `Activated`/`Deactivated` from a click in the world, number keys 1 to 9 and 0 (the same key again puts it away), Backspace drops a `CanBeDropped` tool into Workspace. A client's equip, unequip and activation go to the server, which checks the tool is that player's.
3. Players and peers. `Player:Kick(message)` (a client may only kick its own player; "Cannot kick a non-local Player from a LocalScript") removes the player: `PlayerRemoving` on the server and every client, the character and the Player gone; the editor shows the kicked player's view a "Disconnected" card with the message, the Output says who. Test › Players during a run reads "Players: N" and makes the last player leave. Teams (`teams.rs`, Teams golden): `Player.Team`, `TeamColor` and `Neutral` kept in step, `Team.PlayerAdded`/`PlayerRemoved` in Roblox's order, `Team:GetPlayers`, auto-assignable teams filled evenly at join, spawning on a SpawnLocation of the player's team. A client's `Sound:Play`/`Stop` reaches the server (and the other clients) unless `SoundService.RespectFilteringEnabled`; a property it writes doesn't; `IsPlaying`, `Played`/`Stopped`/`Paused`/`Resumed`. The owner's velocities travel with its positions (`AssemblyLinearVelocity` on the server reads 16 while the client walks, 0 once it stops: ClientReplication `server_speed`, `server_stopped_speed`); auto ownership takes a player's `SimulationRadius` (capped by `MaximumSimulationRadius`); a server script's `PivotTo` of a client-owned character reaches the owner; `MoveToFinished` fires on the server for a `MoveTo` the owner walked.
4. Engine fixes the new goldens showed: a reparenting's events fire before the line after it, in Roblox's order (ReparentOrder: old ancestors' `DescendantRemoving`, `ChildRemoved`, new ancestors' `DescendantAdded`, `ChildAdded`, then `AncestryChanged` deepest first, then `Changed`), and `Destroy` likewise; `Enum.KeyCode.Unknown` and other legacy enum names resolve; `Humanoid:Move(zero)` from the default controls no longer cancels a `MoveTo` (a server's `MoveTo` of a player's character walked 0 before).
5. The editor: Step (the Run row while paused) runs one frame; the axis widget hides over a player's view; a first mouse motion over the view reaches the game (a button under the pointer); a letter typed with Shift and the same letter after it both arrive.
6. Checked live on Xephyr with the built binary (`rbx-native-targets/local-run-m6-shots/`): a ScreenGui button counting clicks (`screengui-button-clicked.png`) and a TextBox (`textbox-typed.png`), the core GUI (`core-gui-health-hotbar-playerlist.png`), the torch equipped by key 2 in the right hand (`tool-equipped-in-hand.png`) and recoloured by a click (`tool-activated-recoloured.png`), Player2 walking seen from your client (`peer-movement-sequence.png`, frames in `peer-movement/`), red and blue teams on their spawns (`teams-red-and-blue-spawns.png`), the kick card (`kick-message.png`), Player2 leaving (`player-left.png`), Step (`step-one-frame-before-after-15.png`).

Gaps from M6 (also in CONFORMANCE.md where a golden measures them):

| Area | Roblox | ours |
|---|---|---|
| Holding a tool | the right arm raised forward (`toolnone` in the default Animate) | the arm stays down; our Animate has no tool pose |
| Core GUI | Roblox's own topbar, chat, backpack inventory grid, leaderstats columns, emotes, the Escape menu | health, hotbar, player list (no leaderstats), notifications |
| BillboardGui / SurfaceGui input | clickable | drawn, not hit-tested |
| Peer physics | interpolated with the owner's velocity, ownership handed over after a delay, the server checking client positions | the owner's position and velocity each frame (in process, no lag to hide), handover at once, nothing checked |
| Sound | audio | state only, no sound |
| GuiService | `SelectedObject`, `MenuIsOpen`, gamepad selection | not in |

Still open from earlier milestones: the humanoid leftovers (slope 60°, the 180° turn, heavy WalkScale, the Climb/Seat one-frame offsets, swimming), soft contacts (BoxStack, HingeFree, BallSocket), the VectorForce extra frame, union collision, a MeshPart's transparent texels; the camera's pitch and shift lock measured on a desktop X server.

## Milestone 7 (done): tweens, sound, effects, chat, the menu

Five new goldens, recorded on the sandbox place with Sober (5 passes): TweenCurves, TweenRuntime, SoundPlayback, TextChat, ToolPose.

1. TweenService at run time (`crates/rbx_lua/src/server/tween.luau`, `tween.rs`). Every `EasingStyle` and direction as `GetValue` measures them (TweenCurves: 33 curves of 101 samples each, all within 1e-6 of Roblox, plotted in `easing-ours-vs-roblox.png`). Elastic's period is 4/13 with `s = p/4`; `InOut` is built from `In`; `GetValue` clamps alpha outside 0..1 and refuses `Spring`/`Bezier` with Roblox's message. Tweens step at the start of the frame (before `PreAnimation`; on a client after the render step), `Completed` fires on the step that arrives, `Cancel` fires `Completed(Cancelled)` at once, `Delay` comes before every repeat and its last frame doesn't move, `Reverses` turns round without a jump, `Play` after `Pause` resumes and after `Cancel` starts again from where things are. A newer tween takes the properties it shares with an older one at the next step and an older one left with none is cancelled; every tweenable type lerps linearly (bools flip on the first step), `NumberRange`/`NumberSequence`/strings are refused with Roblox's errors. GUI and parts alike; a server tween replicates as property changes.
2. Sound (`crates/rbx_sim/src/sound.rs`, `audio.rs`; the editor's mix in `crates/rbx_studio/src/shell/local_run/sound.rs`). `SoundId` goes through the asset fetcher (offline: the cache), decoded with symphonia (an unmodified MPL dependency); `TimeLength` is the Ogg's last granule. `Play`/`Stop`/`Pause`/`Resume`, `Played`/`Paused`/`Resumed`/`Stopped` at once with the id, `Ended` on the server with `IsPlaying` still true in the handler, `TimePosition` back to 0 after the end and on `Stop`, `DidLoop`, `Loaded` only for a sound that wasn't cached at first sight, the clamps on `Volume` (0..10) and `PlaybackSpeed` (≥ 0). A client's `Ended` lags its server by the frame the play took to arrive, as measured. The editor mixes what the viewed peer hears into one stereo stream: volume through the `SoundGroup` chain, `RollOffMode` distance from the camera with `SoundService.RolloffScale`, equal-power pan, resampled by `PlaybackSpeed`, following the sim's clock and jumping only when it drifts; silent while paused or watching the server; the menu's volume bar; no device means silence, not an error. `PULSE_SINK` picks the sink (`local-run-audio-null-sink.wav` was recorded from a null sink).
3. Effects from scripts. `ParticleEmitter:Emit(n)` (16 by default) and `Clear()` go to the renderer through `rbx_viewer::particle_burst` (a queue the particle pass drains, the only renderer addition). A disabled emitter now stays planned at rate 0, so what it spawned lives out its life and `Emit` works while it's off, as in Roblox. `Enabled`, `Rate`, Trail and Beam properties (`CurveSize0/1` too) and Fire/Smoke/Sparkles `Enabled` were already re-planned from the run's property changes; nothing in the shaders or `gpu.rs` changed.
4. M6 follow-ups.
   - The tool pose: our Animate plays `toolnone` while a tool is held, the Right Shoulder at 89° as Roblox has it (ToolPose). The fix was the respawned character's LocalScripts not starting when `Player.Character` named a model that was already there.
   - leaderstats as columns in the player list (any `ValueBase`, whole numbers without a fraction).
   - The Escape menu: Resume, Reset character (R), Leave game (L, ends the run), volume. Reset follows `ResetButtonCallback`: `false` turns it off, a `BindableEvent` is fired instead, otherwise the server sets the character's health to 0.
   - TextChatService, clean-room (`crates/rbx_lua/src/server/chat.luau`, `chat.rs`, `crates/rbx_sim/src/chat.rs`): `TextChannels` with `RBXGeneral`/`RBXSystem`, the four configurations, a `TextSource` per player, `SendAsync` (client only, the server's error otherwise), `OnIncomingMessage` on the service and the channel, `SendingMessage`, `ShouldDeliverCallback` once per source on the server, `MessageReceived` on the service then the channel, `DisplaySystemMessage` wrapped the way Roblox wraps it, `AddUserAsync`. Our own chat window (top left, `/` to type) and bubbles over heads. Offline there is no filtering service, so nothing is filtered, ever: what a player types is what everyone sees.
   - A `TextBox` cuts its text (and placeholder) at its box; a text object with `ClipsDescendants` cuts its own text too (unless rotated, like its children).
   - Signals: a handler disconnected by an earlier handler of the same firing no longer runs (the TweenRuntime `client_phase` order, where a script woken by `Heartbeat:Wait()` disconnects its other `Heartbeat` handlers).
5. Test places are built from the Baseplate template (Home › New templates, the stud texture); the live check used one with a tween, a fountain that `Emit`s, a toggling Fire, a looped Sound, leaderstats, a tool and two long-placeholder TextBoxes.
6. Checked live on Xephyr with the built binary (`rbx-native-targets/local-run-m7-shots/`): `tween-part-and-gui-mid-flight.png`, `easing-ours-vs-roblox.png`, `particles-emit-from-script.png`, `particles-fire-toggle-and-emit.png`, `tool-held-arm-raised.png`, `leaderstats-player-list.png`, `escape-menu.png`, `reset-health-zero.png`, `chat-window-and-bubble.png`, `textbox-placeholder-clipped.png`, `local-run-audio-null-sink.wav`.

Gaps from M7 (also in CONFORMANCE.md where a golden measures them):

| Area | Roblox | ours |
|---|---|---|
| `TextBounds`, `TextFits` | measured from the font | not computed in the sim (the renderer measures, scripts can't read it) |
| TextBox overflow | where the visible part of an overflowing centred placeholder sits was not measured (no pixel goldens) | centred, cut at both edges |
| Chat | filtering, whispers, `TextChatCommands` (`/e`, `/whisper`), bubble settings from `BubbleChatConfiguration`, channel tabs | none of these; `/e` emotes go through `Player.Chatted` only |
| Sound | `AudioPlayer`/`AudioEmitter` (the new audio API), `SoundEffect`s, `PlaybackRegion`, `RollOffMinDistance` curves from a real listener | `Sound` only, listener at the camera, no effects |
| Particles | `Emit` on a client replicates nowhere; a server `Emit` reaches every client | both shown on the viewed client, not replicated as such |
| Escape menu | Roblox's settings, report, record tabs | ours: Resume, Reset, Leave, volume |

Still open from earlier milestones: BillboardGui/SurfaceGui input, `ScrollingFrame` dragging, gamepad selection, peers on a simulated network, the humanoid and contact leftovers.

## Milestone 8 (done): rendering fidelity

Measured against the real client, not eyeballed. Every number below came from Sober on Xephyr at 1280x720, on the sandbox place only; nothing of Roblox's (shaders, binaries, textures) went in, only what its pictures show.

1. The harness (`crates/rbx_viewer/src/fidelity.rs`, `fidelity/scenes.rs`, `examples/fidelity.rs`, `crates/rbx_sim/examples/record_render_goldens.rs`). 41 scenes, each a place with a fixed camera. The recorder publishes one place per place-level Lighting group (`Technology`, `LightingStyle`, `PrioritizeLightingQuality` can't be set from a script) and shoots each scene in Sober; the reference PNGs live in `crates/rbx_viewer/fidelity/refs/` (1.5 MB, 320x180), the raw captures stay out of git. `fidelity` draws ours offscreen, compares at 320x180 (mean and p90 CIEDE2000, SSIM), writes the side-by-sides (real | before | after | heatmap) and a `report.json` with every probe: named 3D points (a lit top, a shaded side, the sky, a floor at 4 studs from a lamp...) whose real and drawn colours are what the fits below were made from. `goldens.json` holds each scene's numbers with a tolerance (0.6 ΔE, 0.02 SSIM); `tests/fidelity.rs` fails a scene that drifts past it (skips without a GPU).
2. Lighting, per `Technology`:
   - ShadowMap and Future draw through a filmic curve, fitted to a 16-step grey ramp under 13 lamp setups: `x(2.4536x + 0.0353) / (x(2.1029x + 0.7171) + 0.1414)`. The sun is 0.387 per unit of `Brightness`, ambient, sky light and reflections 0.757 of their value.
   - Voxel and Compatibility clamp: sun 0.528 per unit, ambient as is.
   - What a place leaves out is the `Lighting` class default (Voxel, 127 grey, 14:00 at 41.7°, no shadows), not Studio's template; a place with no `Lighting` at all still gets the template.
   - The sun crosses air: grey `exp(-0.033 (m - 1))` with the Kasten-Young air mass `m`, and blue and green lost below 15° (5° measured at (0.77, 0.77, 0.28) of full light).
   - The sky darkens past dusk to 0.029 of its day value with a warm tint; the moon is white, 0.155 of the sun.
   - Environment reflection goes through Fresnel (0.1 facing, 0.4 grazing).
3. Sky: with no `Sky` the default sun, moon and stars are drawn (they weren't). Fog and Atmosphere were measured but not refitted (see gaps).
4. Shadows: the shadowed/lit ratio matches within 3 ΔE on the stage scenes; softness and bias were not refitted. Local lights outside Future are the voxel grid's, measured per class: no Lambert term (`saturate(2 N·L + 1)`), 1.3x the range, `min(1, 1.1 (1 - u²)²)` falloff, point 1.13, spot 0.043, surface 0.86 of the radiance scale, the spot cone from its centre with a 0.1 soft edge, no shadows.
5. Materials: every `Enum.Material` in two scenes plus glass, neon, force field, reflectance and transparency. Only the shared lighting moved them (8.0 → 6.0 ΔE); per-material texture brightness is the largest gap left.
6. Post effects: `SunRays` is an additive glow `(2.6, 0.97, 0) exp(-d / (0.81 Spread))` times the open sky along a 24-tap march of the depth buffer toward the sun (21.7 → 8.8 ΔE). Bloom, ColorCorrection, Blur and DepthOfField moved with the lighting only (9.3 → 4.8, 7.0 → 3.4, 7.4 → 3.4).
7. The M7 respawn camera: with the Escape menu up, mouse moves, right drags and the wheel turned the camera (into the ground, and the popper into the head), and a reset from the menu left it there. `GuiService.MenuIsOpen` now follows our menu (`Sim::set_menu_open`); our PlayerModule frees the mouse and turns nothing while it's up, and a new humanoid takes `CameraSubject` from the dead one. Test: `the_menu_holds_the_camera_still_and_a_reset_from_it_lands_behind_the_new_character` (fails before the fix). Checked live in the built editor.

| scene | ΔE before | ΔE after | SSIM after |
|---|---|---|---|
| default_lighting | 16.45 | 1.62 | 0.952 |
| studio_baseplate | 7.62 | 2.86 | 0.973 |
| studio_lighting | 7.05 | 3.39 | 0.932 |
| morning | 10.08 | 4.72 | 0.912 |
| evening | 26.90 | 8.06 | 0.943 |
| night | 10.93 | 3.32 | 0.898 |
| latitude | 6.33 | 3.32 | 0.941 |
| ambient_colour | 8.49 | 5.89 | 0.935 |
| exposure | 10.99 | 2.97 | 0.959 |
| realistic_style | 7.05 | 3.39 | 0.932 |
| fog | 8.01 | 3.16 | 0.921 |
| atmosphere | 8.96 | 6.56 | 0.840 |
| sun_rays | 21.66 | 8.83 | 0.909 |
| custom_sky | 5.17 | 0.90 | 0.995 |
| shadows_hard | 7.38 | 4.70 | 0.966 |
| shadows_soft | 6.39 | 6.07 | 0.945 |
| local_lights | 15.59 | 6.28 | 0.873 |
| materials_a | 8.06 | 6.03 | 0.932 |
| materials_b | 7.90 | 5.81 | 0.934 |
| special_materials | 7.25 | 3.90 | 0.911 |
| bloom_colour_correction | 9.25 | 4.76 | 0.895 |
| blur | 7.01 | 3.37 | 0.946 |
| depth_of_field | 7.43 | 3.40 | 0.934 |

The 14 `tone_*` scenes (2.7 to 4.1) and 4 `light_*` falloff scenes (1.0 to 2.5) were added while fitting and have no before. Frame time didn't move (`bench`, median ms at quality 1/11/21: TestPlace 0.23/0.29/0.77 → 0.21/0.29/0.75, marked.rbxl 0.78/0.30/0.45 → 0.33/0.30/0.43, DEMO_LIGHTING_MATERIALS 0.23/0.30/0.75 → 0.24/0.30/0.76); the software adapter (llvmpipe on Vulkan, lean textures) draws the four scenes tried within 0.1 ΔE of the GPU. Pictures in `rbx-native-targets/local-run-m8-shots/`.

Gaps from M8:

| Area | real client | ours |
|---|---|---|
| Material textures | each material's own albedo | Rock, Sandstone too bright, Marble, Basalt too dark (up to 15 ΔE on a slab) |
| Lit floor in Studio's template | 143 grey | 166 |
| Evening | warm sky near the sun | too blue, sky and faces |
| Night | moonlight from higher up | front faces too bright |
| Sun highlight | a small one at `EnvironmentSpecularScale` 0 | none |
| SurfaceLight | lights its own face, near-wall floor | off near the wall |
| SunRays | smooth shafts | 24-tap banding |
| Studs | surface studs | not drawn |
| Future local lights, `ShadowSoftness`, Atmosphere haze and glare | | not measured or refitted |

## Milestone 9 (done): materials and the last lighting gaps

Same harness and rules as M8 (Sober on Xephyr at 1280x720, sandbox place only, nothing of Roblox's in the tree but what its pictures show). Nine scenes were added: `studs`, `text_boxes`, `text_sizes`, `shadows_half`, and each `light_*` falloff scene and `local_lights` again under `Future`. The fidelity scenes can now carry screens (`Builder::screen`), drawn under the top bar the way the player sees them.

1. Materials: each 2022 pack's colour map is regraded once, on upload, by a per-material gain and contrast fitted from the material scenes (`crates/rbx_materials/src/grade.rs`, two numbers a material, no texture data; Rock and Sandstone down to 0.46 and 0.45, Marble and Basalt up to 1.3 and 1.4). With the sun highlight below, the two material scenes went 6.0 → 3.4 and 5.8 → 3.2. `MaterialVariant` and per-material roughness were not done.
2. Sun highlight: the real client's is broad and about half the reflection's strength (shininess 15, 0.5 of `spec_strength`), goes through the same Fresnel curve as the reflection (the streak a low sun leaves on a floor), and keeps 0.15 of it at `EnvironmentSpecularScale` 0. This is what fixed the template floor (lit floor 160 → 150 against 142) and moved nearly every stage scene by 0.5 to 1.7 ΔE.
3. Dusk: the sky, its bounce and its haze are tinted (1.12, 0.8, 0.6) with the sun under 2 degrees up, fading out by 5 (`lighting/sky.rs`); evening 8.1 → 2.8.
4. Night: the moon is where the real client's `GetMoonDirection` puts it (the sun's direction with X and Y turned round, not opposite the sun), so front faces are no longer moonlit, and it is 0.21 of the sun, not 0.155; night 3.3 → 1.6.
5. Surface studs: Studs, Inlet, Universal, Weld and Glue drawn on a block's faces by `material.wgsl`'s `surface_relief` (a rounded square per stud raised or sunk, the checker, the diagonal grooves), packed three bits a face into the instance's material word. SunRays march from a per-pixel offset (no banding), and anything past 30 studs lets the rays through as the sky does (8.8 → 6.5).
6. Future local lights: Lambert from the light's own position, flat to 0.7 of `Range` then gone at `Range` (`future_falloff`), a spot's cone full to 0.3 of its half-angle and a squared smoothstep to its edge, per-class strengths; every `light_*_future` scene is now 0.9 to 1.2 ΔE. `ShadowSoftness`: the kernel at 1 is 2 texels, not 8 (the wide one ghosted the shadow and striped the floor); shadows_soft 6.1 → 3.0. Atmosphere: the haze is a third `Color` and two thirds the sky it scatters (6.6 → 5.6).
7. SurfaceLight under voxel lighting: the grid keeps it as a glow the shape of its face, half its `Range` out in front, reaching 0.8 of it, with no cone (the floor in front of the wall peaks eight studs out); 2.5 → 1.9.
8. TextBox: a resting `TextBox`'s text and placeholder spill past the box like a label's, as the real client draws them (the "ge to see whe" slice was our clip); only the box being typed in is cut to its field. `text_boxes` has a golden now.
9. Goldens: every scene is recorded, each with a tolerance of 0.15 of its own ΔE, between 0.25 and 0.6, so a closed scene is held tighter than one still far off.

| scene | ΔE M8 | ΔE M9 | SSIM M9 |
|---|---|---|---|
| default_lighting | 1.62 | 1.43 | 0.953 |
| studio_baseplate | 2.86 | 2.15 | 0.974 |
| studio_lighting | 3.39 | 2.80 | 0.935 |
| morning | 4.72 | 4.53 | 0.910 |
| evening | 8.06 | 2.83 | 0.953 |
| night | 3.32 | 1.58 | 0.973 |
| latitude | 3.32 | 3.00 | 0.940 |
| ambient_colour | 5.89 | 5.82 | 0.928 |
| exposure | 2.97 | 2.73 | 0.959 |
| realistic_style | 3.39 | 2.80 | 0.935 |
| fog | 3.16 | 3.02 | 0.917 |
| atmosphere | 6.56 | 5.62 | 0.842 |
| sun_rays | 8.83 | 6.45 | 0.929 |
| custom_sky | 0.90 | 0.76 | 0.995 |
| shadows_hard | 4.70 | 2.95 | 0.978 |
| shadows_soft | 6.07 | 2.98 | 0.970 |
| local_lights | 6.28 | 4.65 | 0.916 |
| light_surface | 2.46 | 1.89 | 0.722 |
| materials_a | 6.03 | 3.38 | 0.941 |
| materials_b | 5.81 | 3.21 | 0.953 |
| special_materials | 3.90 | 3.37 | 0.911 |
| bloom_colour_correction | 4.76 | 4.20 | 0.897 |
| blur | 3.37 | 2.77 | 0.949 |
| depth_of_field | 3.40 | 2.91 | 0.937 |

New scenes, no before: studs 3.15, text_boxes 3.58, text_sizes 2.40, shadows_half 3.02, local_lights_future 4.29, light_point_1/4, light_spot, light_surface under Future 0.99, 1.10, 0.95, 1.24. The tone scenes and the voxel point and spot falloffs didn't move (within 0.05). Over the 41 scenes with an M8 number the mean went 3.72 → 3.03. Pictures (real | M8 | M9 | heatmap) in `rbx-native-targets/local-run-m9-shots/after/`.

Gaps from M9:

| Area | real client | ours |
|---|---|---|
| GUI text in Sober | about 1.15x the size Studio's `GetTextSize` gives (both `Legacy` and Source Sans, `text_sizes`) | Studio's size; Sober runs the Android client, so this needs a desktop reference before anything changes |
| Atmosphere, SunRays | a white veil round a low sun over sky and floor, a narrow sun streak on the floor | dimmer, broader |
| Morning | lit shadows under a low sun | shadows too dark (floor shadow 55 against 72) |
| Coloured ambient | | 5.8, untouched |
| Local lights near walls | the voxel grid darkens light next to solid cells | no occlusion at all under voxel lighting |
| Materials | `MaterialVariant`, per-material roughness | colour regrade only |

## Milestone 10 (done): physics and humanoid conformance

Every row that was off at the start of M10 was worked on, and seven cheap tests were added (SurfaceMix, DensityMass, Conveyor, HumanoidPlatform, HumanoidPosture, ContactSink, HumanoidTurnRates), recorded with 5 passes (humanoid set) and 12 (physics set) on the sandbox place. Several old tests gained per-frame traces (TouchedOrder, BallSocket, HingeFree, BoxStack, HumanoidClimb, HumanoidSeat). The table went from 25 rows off out of 1044 to 12 out of 1128.

1. VectorForce: on a free body (adaptive steps) the force is applied with the gravity kicks, at the same rate and with the same extra kick when the rate switches up. That extra kick is Roblox's "extra frame" (10.167, exact on the clean passes).
2. Impacts: a free body goes through what it meets for its frame (the contacts are inert, so Touched still fires on time), and the next frame it's pushed back out along its way in and, if it hit faster than 6 stud/s, takes 0.75 of its free frame's gravity kick before it bounces. Roblox's block ends the falling frame 0.9 stud deep in the floor; TouchedOrder now matches the clean pass bounce for bounce (21, 42, 53, 59, 62). In contact the mix rules: two weights of 0 give 0, not the plain mean.
3. Contacts between two moving bodies are softer (25 Hz instead of 30), so a stack of five sinks 0.16 at the top, inside Roblox's 0.16 to 0.25 (BoxStack).
4. Joints: hinges and ball sockets are 60 Hz springs, not rigid. HingeFree is within 0.05 stud at 30 frames; BallSocket's swing still drifts apart by 40 frames (see the gaps).
5. Conveyors: an anchored part's `AssemblyLinearVelocity` moves what's on it (contact tangent velocity) and a character standing on it, and a character on a moving body (a raft on a LinearVelocity) is carried by the floor's velocity at its feet.
6. AutoRotate is a lagged spin (gain 8.4, lag 0.45) instead of a fixed fraction per substep: reversals, heavy characters and fast WalkSpeeds turn at Roblox's rate within 2 degrees (HumanoidTurn, HumanoidWalkScale, HumanoidTurnRates). A target straight behind turns left, as Roblox's does.
7. Walk grip scales with the root's density (`walk_cap`), so a density 10 character accelerates like Roblox's (heavy_vx1 12.17 to 15.68).
8. HipHeight changes move the stand height every frame (HumanoidPosture); PlatformStand and Ragdoll states match.
9. Climbing starts when the truss is within 0.25 stud of an R15's front (0.1 for an R6) and walks the last bit in, so both rigs start on Roblox's frame and an R6 no longer flickers to Freefall between rungs. A jump off a seat lifts 0.945 stud and loses three substeps of gravity, so the landing is on Roblox's frame.
10. Side note: the rbx_viewer lib tests now open their wgpu adapters and devices one at a time (a test-only lock); the NVIDIA 580 segfault in parallel runs has not been seen since.

| Test | Field | before M10 | after M10 | Roblox |
|---|---|---|---|---|
| TouchedOrder | events | a frame early from the 3rd bounce | matches the clean pass | |
| VectorForce | vx_30 / x_30 | 10.0 / 2.505 | ok (10.167 / 2.588) | 10.25 / 2.688 (median) |
| BoxStack | top_dy | -0.116 | ok (-0.162) | -0.217 |
| HingeFree | y_30 | -0.904 | ok (-1.091) | -1.045 |
| BallSocket | x_40 / y_40 / z_40 | -3.13 / -1.8 / -1.71 | -3.04 / -2.04 / -1.62 | -1.97 / -3.33 / -1.11 |
| HumanoidTurn | back_1 / back_3 / back_5 | 64.65 / 25.62 / -3.55 | ok | 72.43 / 31.31 / -0.46 |
| HumanoidWalkScale | heavy_vx1 | 12.17 | ok | 15.68 |
| HumanoidWalkScale | heavy_yaw1/2, ws50_yaw1/2 | 12.18 / 22.71 | ok | 8.28 / 19.09 |
| HumanoidClimb | r15_states / r6_states | a frame late; R6 flickers | ok | |
| HumanoidSeat | states | landing a frame late | ok | |
| HumanoidJump | high_apex / high_vy1 | 15.278 / 74.27 | unchanged | 15.479 / 75.09 |
| HumanoidSlope | s60_rise / s60m_v45 | 8.58 / creeps up | unchanged | 12.78 / slides back |

New rows still off: HumanoidPlatform passenger_dy (-3.17 against -2.27, a character on an anchored lift moved by CFrame every frame) and ContactSink heavytop_1/2 (a dense block on a light one: the light one sinks 0.015 then 0.098 against 0.031 and 0.082).

Already covered and not added again: MoveTo's 8 second give-up (HumanoidMoveTo), AutoRotate off (HumanoidTurn `fixed_yaw`), and network ownership (Ownership). What ownership does to server readings of a client-owned part was not recorded.

Gaps from M10:

| Area | Roblox | ours |
|---|---|---|
| Ball socket swing | a 4 stud arm stretches to 4.046 | 4.01; the period drifts and by 40 frames the bob is 1.3 studs away. A softer spring helps BallSocket and breaks HingeFree |
| 60 degree slopes | stutters at the top edge and launches; with MaxSlopeAngle 45 slides back | walks up and off; refusing to walk up sends ours off the far side |
| JumpHeight | 75.09 for 15 studs | 74.27 |
| CFrame-moved floors | keeps the character on the lift | ours drops 0.9 stud behind |
| Mass-ratio contacts | a light block under a dense one sinks twice as far at first | one spring for every pair |
| Unions | collide as their geometry | still their `Size` box (decoding lives in rbx_viewer) |
| Bounce spin | | a flat impact is still solved at one point, so no spin from an off-centre bounce |

## Milestone 11 (done): terrain physics, water and swimming

Smooth terrain is live at run time: scripts edit it, parts and characters collide with it, rays and casts hit it, parts float in its water and characters swim. Seven tests were added (TerrainFill, TerrainRaycast, TerrainPhysics, TerrainHumanoid, TerrainProfile, Buoyancy, Swim), recorded on the sandbox place with 5 passes (2 for TerrainProfile). Before M11 none of them could run: `workspace.Terrain` had no methods and terrain didn't collide.

1. Terrain API (`rbx_lua/src/server/terrain.rs` → `rbx_sim/src/terrain.rs`): `FillBlock`, `FillBall`, `FillCylinder`, `FillWedge`, `FillRegion`, `ReadVoxels`, `WriteVoxels`, `ReplaceMaterial`, `Clear`, `CellCenterToWorld`, `WorldToCell` (`WorldToCellPreferSolid`/`PreferEmpty`), `GetMaterialColor`/`SetMaterialColor`. The fill rules are measured, not guessed: a cell's fill is 0.5 − d/4 for d its centre's distance outside the shape (min over the box's axes, from the ball's radius, a cylinder's radius with its height + 2), stored as round(f × 255); a wedge fills like a block up to the slope's height at the cell's low edge; a fill never empties a cell, Air carves what's past it, and a full cell of solid takes over a water cell while less leaves it water. Edits are written back into `SmoothGrid`/`MaterialColors`, so the viewer redraws them and the client gets them by replication. Terrain whose `MaterialColors` was never set uses the server's colours (Grass 106,127,63), which differ from what Studio writes into a new place. TerrainFill: 98 of 98 fields match. COVERAGE.md now probes `workspace.Terrain`: 16 of its 29 functions (not yet: `CopyRegion`/`PasteRegion`, `CountCells`, the material slots, `Read`/`WriteVoxelChannels` and the `*Async_beta` voxel calls).
2. Collision: per 32³ chunk, a surface net over "solid > 0" (each crossing the fraction of the way from the solid voxel's centre toward the empty one, which puts a slab filled to a cell boundary 2 studs above it, as Roblox's is), one trimesh per material so each contact gets the material's friction and elasticity through the same mixing as parts. Chunks are re-meshed lazily once a frame after an edit; a part whose centre ends up inside terrain is lifted out. `FloorMaterial` reads the terrain material under the feet.
3. Queries: `Raycast` (with `RaycastResult.Material`, `IgnoreWater`, Include/Exclude of `Terrain`), `Spherecast` and `Blockcast` hit terrain; a ray from inside terrain finds nothing (back faces are culled), as Roblox's doesn't. Water is hit at its surface by a march on the water fraction.
4. Water: every free part takes the weight of the water it displaces where it displaces it (water density 1, sampled every half stud) plus a linear drag of 5.7 per stud³ per stud/s, as forces integrated with gravity each substep. Equilibrium depths match by density (0.3 floats at +0.4, 0.7 at −0.38, 0.95 at −0.93, a 0.5 plank at 0); a 1.5 block sinks at Roblox's 17 stud/s and a 0.95 one rises at its 1.7. `WaterWaveSize`/`WaterWaveSpeed` have no physical effect (the calm block in Buoyancy settles where the wavy one does). The viewer already drew the water properties (wave size and speed, colour, transparency, reflectance).
5. Swimming: Freefall becomes Swimming once the root is under the surface and falling; the water holds the character up (an R6 left to the water bobs exactly as Roblox's does; an R15's bob is damped, 4/s); swim speed is 0.74 × WalkSpeed for an R15 and 0.875 × for an R6; `Humanoid.Swimming` fires with the speed (0 on the way in); a jump from Swimming goes Jumping › Freefall › Swimming; touching the bottom gets an R15 up and Running (GettingUp › Running). Our Animate already played Swim/SwimIdle on that signal.
6. A client gets Terrain with the place, voxels and all; it now loads it then, so the character it simulates stands on terrain a server script built (a session test).

| Test | Fields | before M11 | after M11 |
|---|---|---|---|
| TerrainFill | 98 | none ran | 98 ok |
| TerrainRaycast | 16 | none ran | 12 ok (3 normals on the ball, `ignore_water` tilt) |
| TerrainPhysics | 15 | none ran | 10 ok |
| TerrainHumanoid | 18 | none ran | 12 ok (the 60 degree rows) |
| TerrainProfile | 4 | none ran | 2 ok (the voxels; the surface heights differ) |
| Buoyancy | 58 | none ran | 41 ok |
| Swim | 30 | none ran | 25 ok (R6 on the shelf and after the jump) |

Over the whole table 51 rows are off out of 1367: the 12 left from M10 and 39 terrain/water ones. Checked live in the built editor, Play (local), on a place whose script builds a grass field, a hill and a lake at run time and drops three crates of density 0.3, 0.6 and 0.9 in it: walking up the hill, swimming, the crates floating at three depths, a jump out of the water, Stop back to the untouched place (`rbx-native-targets/local-run-m11-shots/`, the place is `TerrainLake.rbxlx` there).

Gaps from M11:

| Area | Roblox | ours |
|---|---|---|
| Terrain surface | its mesher's own triangulation: a 60 degree wedge is a 68 degree face then a long fall (TerrainProfile) | a surface net, smoother and symmetric; a character climbs our 60 degree bump where Roblox's stops at dz 11 |
| Mesh normals | a ray near a vertex gets Roblox's split | ours split the quad the other way (3 normals on the ball) |
| Ice | a Plastic block decelerates at friction 0.19 first, then 0.06 (stops at 73 frames) | one mix, 0.09 (v15 15.6 against 10.7) |
| Rolling | a ball reaches 28.5 stud/s at 30 frames down the 30 degree wedge | 34.5 |
| Water entry | dips less (a 0.3 block to −0.15) yet bobs longer | dips 0.3 to 0.5 deeper; the plank bobs where Roblox's is still by 30 frames |
| Lake floor | rests 0.36 lower under water (−21.36) | at the dry surface (−21.0) |
| R6 swimming | stays Swimming on the shelf (FloorMaterial Air); jumps 7.84 | stands up on it; jumps 6.77 |
| Swim pose | the body lies along the swim direction | upright (the Swim animation plays, the root doesn't tilt) |
| A ball falling into terrain | goes in (−15.8) | rests on it (2.999) |

## Milestone 12 (done): API breadth, part 1

The services a game's server leans on now run in Local Run, offline: DataStores, MemoryStores, messaging, HTTP and JSON, test purchases, tags and pathfinding. Each is a Luau module (`rbx_lua/src/server/services/*.luau`) over a small Rust host (`server/services.rs`), loaded the first time a script touches the service. Eight tests were added (DataStoreLimits, DataStoreValues, MemoryStore, HttpJson, Messaging, CollectionTags, Marketplace, Pathfinding) and recorded on the sandbox place, with real DataStore writes only under `localrun_m12_` keys, which the tests delete.

1. DataStoreService (`services/datastore.luau`, `services/store.rs`): Get/Set/Update/Increment/RemoveAsync, `GetVersionAsync`/`GetVersionAtTimeAsync`/`ListVersionsAsync`/`RemoveVersionAsync`, `ListKeysAsync` (prefix, AllScopes, deleted keys), `ListDataStoresAsync`, `DataStoreSetOptions`/`IncrementOptions` metadata and user ids, `DataStoreKeyInfo`, OrderedDataStore `GetSortedAsync` pages, the request budgets (`GetRequestBudgetForRequestType`) and their throttling, and the error codes and messages a live server gives (key 50 and name 50 characters, value 4 MB, JSON-only values; codes 101–107, 502 and 511–513 as recorded). The data is one JSON file per place, `<config>/local-run/<place>/datastores.json` (the place id once linked, else a hash of its path), written after every change. Test › Data cycles Kept (kept between runs), Fresh (empty each run, never written) and Off (Studio's "Enable Studio Access to API Services" off: every call fails with Studio's message). DataStoreLimits: 58 of 58. DataStoreValues: 61 of 67, the 6 off are a live server's eventual consistency (a key just written isn't listed yet, a version just made isn't readable), which we don't imitate.
2. MemoryStoreService (`services/memorystore.luau`): HashMap (Get/Set/Update/Remove, `ListItemsAsync` pages), SortedMap (Get/Set/Update/Remove, `GetRangeAsync` with bounds, `GetSizeAsync`) and Queue (Add with priority, Read with count, all-or-nothing and wait, Remove by id, `GetSizeAsync`, invisibility timeouts), with expirations, the size, key and expiration limits and their messages. MemoryStore: 50 of 51 (a queue read's wait: the live server timed it with `os.clock`, which runs ahead of game time in a busy frame).
3. HttpService: `JSONEncode`/`JSONDecode` rewritten to write exactly what a live server writes (JS number layout, `\u00XX` escapes, NaN and inf as tagged objects, how mixed and sparse tables, cycles and depth fail), `GenerateGUID` (lowercase), `UrlEncode` (only alphanumerics kept). `RequestAsync`/`GetAsync`/`PostAsync` through `rbx_net` on a worker thread: HttpEnabled off gives "Http requests are not enabled. Enable via game settings", offline or unreachable gives `HttpError: ConnectFail`, a timeout `HttpError: Timedout`. HttpJson: 100 of 101 (`1e10/3`'s last digit: Roblox's formatter rounds the 17th digit differently). A session test talks to a local test server and checks the offline error.
4. MessagingService (`services/messaging.luau`): `PublishAsync`/`SubscribeAsync` between every server of the same universe in the process, deferred as a live server delivers them, with the topic and message size limits and the `Data`/`Sent` message table. Messaging: 18 of 18.
5. MarketplaceService (`services/marketplace.luau`): the four `Prompt*Purchase` calls and `PromptSubscriptionPurchase` open our own test purchase dialog over the Play (local) view (Buy (test) or Cancel, nothing charged); the answer fires the `Prompt*PurchaseFinished` events on the server and the buyer's client, records passes, assets, bundles and subscriptions in the place's ownership table (`UserOwnsGamePassAsync`, `PlayerOwnsAsset(Async)`, `PlayerOwnsBundle(Async)`, `GetUserSubscriptionStatusAsync` read it), and hands a developer product to `BindReceiptHandler`'s handler (filtered by product) or `ProcessReceipt`, retried until it answers PurchaseGranted (every 10 s, when the callback is set again, when the buyer comes back). `GetProductInfo(Async)` asks Roblox's public catalog APIs and keeps the answer under the asset cache, so an offline run still has it; with nothing kept it fails with "…failed because the app is offline and it isn't cached". Marketplace: 16 of 16 (the argument errors).
6. CollectionService: every tag call (`tag_arg`'s coercions and messages), `GetAllTags`, `TagAdded`/`TagRemoved`, `GetTagged` limited to what's under a service, and the signals' order measured on a live server (InstanceAdded/Removed immediately, TagAdded/Removed deferred on a tag's first and last use, parenting in and out, `Destroy`, clones). Tags replicate as the `Tags` property, and a client's CollectionService fires for them (a session test). Also: `Debris:AddItem`, and the Players, Player and RunService helpers that were missing (`GetNameFromUserIdAsync`/`GetUserIdFromNameAsync` from the players in the game or Roblox's users API, kept like product info; `GetFriendsAsync` as an empty page; the `*Async` HumanoidDescription calls; `DistanceFromCharacter`, `GetJoinData`, `GetNetworkPing` 0, `Move`, `LoadCharacterAsync`, the friend and group checks false; `IsResimulating`, `GetPredictionStatus`, `BindToSimulation`). CollectionTags: 19 of 19.
7. PathfindingService (`server/pathfinding.rs`, `services/pathfinding.luau`): `CreatePath` with its agent parameters (radius, height, CanJump, CanClimb, WaypointSpacing, Costs) and their cast errors, `ComputeAsync`/`FindPathAsync`, `GetWaypoints`, `Status`, `CheckOcclusionAsync`, `Blocked`/`Unblocked`, `PathWaypoint`. The path is A* over a one-stud grid built per request from the physics world's rays (parts and terrain): floors an agent of its height fits under, walls and drops kept a stud away (so a 3 stud door passes as Roblox's does), steps up to 1.6 studs walked, gaps and ledges up to 6.5 studs jumped from the last open floor before the edge, `PathfindingLink`s as `Custom` waypoints, costs by material or `PathfindingModifier` label (`math.huge` forbids), characters and non-colliding parts ignored. The path is pulled tight, then cut every `WaypointSpacing` studs; each waypoint is labelled with the floor's material, and a jump or link waypoint (the landing, as Roblox puts it) with `Jump` or the link's label. Pathfinding: 219 of 229, the 10 off are the PassThrough door: the live server gave NoPath for a modifier added just before `ComputeAsync` (its navmesh lags behind), ours passes at once.

| Test | Fields | ok |
|---|---|---|
| DataStoreLimits | 58 | 58 |
| DataStoreValues | 67 | 61 (eventual consistency) |
| MemoryStore | 51 | 50 (`os.clock` against game time) |
| HttpJson | 101 | 100 (one digit of `1e10/3`) |
| Messaging | 18 | 18 |
| CollectionTags | 19 | 19 |
| Marketplace | 16 | 16 |
| Pathfinding | 229 | 219 (the PassThrough door) |

Every off row is listed as a `known_gap` in its golden. Over the whole table 69 rows are off out of 1926: the 51 left from M11 and the 18 above.

COVERAGE.md now reaches the classes only a service hands out (stores, pages, key infos, memory store structures, `Path`) through a server, which it couldn't before. Functions went from 190 to 281 of 1385 (14% to 20%), events from 107 to 118 of 355 (30% to 33%), properties from 1935 to 1949 of 2510 (77% to 78%).

Checked live in the built editor with Play (local) on `M12Services.rbxlx` (in `rbx-native-targets/local-run-m12-shots/` with the screenshots): a server script counts the place's runs with `UpdateAsync` and writes the count on a sign, a LocalScript prompts a game pass, and an R15 NPC walks to a goal around a wall and back on `ComputeAsync`'s waypoints (marked with neon balls). Four runs in a row read Run number 1, 2, 3, 4 (the DataStore kept between runs, editor restarts included); Buy (test) printed bought true and owns true on the server and the client, Cancel bought false (and owns stayed true from the earlier run); every path came back Success and the NPC reached the goal each time; Test › Data set to Off made the sign read `StudioAccessToApisNotAllowed: Studio access to APIs is not allowed.`; Stop left the edited place untouched. A store object a script held showed as a root in the Explorer during the run; that's fixed (host-made objects are parented to nothing, as `Instance.new`'s are) and covered by a session test, after the live check.

Gaps from M12:

| Area | Roblox | ours |
|---|---|---|
| DataStore consistency | a write shows in `ListKeysAsync` and versions after a delay | at once |
| `GlobalDataStore:BatchGetAsync` | new | not there |
| Navmesh | Roblox's own, rebuilt behind changes (a fresh modifier isn't seen yet) | a grid per request: exact at once, slower on big maps (the search region is capped at 384 studs a side) |
| `AgentCanClimb` | climbs `TrussPart`s | ignored |
| `Path.Blocked` | fires when something lands on the path (never in 300 frames in the golden) | never fires |
| MessagingService | between servers of a universe across machines | between servers in one process |
| Purchases | Roblox's prompt, real ownership | our dialog, a local ledger per place |
| `GetProductInfo` offline | — | only what an earlier online run kept |

## Milestone 13 (done): API breadth, part 2

The services a game's client and its social side lean on, in the same shape as M12 (a Luau module per service in `rbx_lua/src/server/services/`, over `services.rs`), plus the editor's half: a teleport restarts the run, toasts, prompt dialogs, touch devices.

1. TeleportService (`services/teleport.luau`): `TeleportAsync` (with `TeleportOptions`' data, `ReservedServerId`/`ShouldReserveServer`, the `TeleportAsyncResult`), `Teleport`, `TeleportToPlaceInstance`, `TeleportToSpawnByName`, `TeleportToPrivateServer`, `TeleportPartyAsync`, `ReserveServer` (codes this run made), `GetPlayerPlaceInstanceAsync`, `SetTeleportGui`, `Get/SetTeleportSetting`. A run has one place, so a teleport lands only where the run can reload into: the running place, or a place file mapped to the target place id in the place's Local Run settings (`teleport_places` in `<config>/local-run/<place>/settings.json`). The host hands the request to the editor (`Session::take_teleports`, with the teleport GUI copied out of the asking peer's DataModel), which stops the run and starts it again there with each player's `TeleportData`, GUI and source place (`Services::arrivals`, `Session::set_arriving_gui`): `GetLocalPlayerTeleportData`, `GetArrivingTeleportGui`, `LocalPlayerArrivedFromTeleport` and `Player:GetJoinData()` read them. Anywhere else fires `TeleportInitFailed` with `Enum.TeleportResult.Failure` and a message saying why.
2. BadgeService (`services/badges.luau`): `AwardBadgeAsync` (server only) into the place's local ledger (the purchases' ownership table), `UserHasBadgeAsync`, `CheckUserBadgesAsync` (10 ids at most), `GetBadgeInfoAsync` from the public badges API, kept for offline runs (`host.web`, a generic kept GET/POST for Roblox's public APIs). Offline with nothing kept, an award still lands under a stand-in name. The player gets a "Badge Awarded" toast (`Notice`, `Session::take_notices`).
3. GroupService (`GetGroupInfoAsync` with roles, `GetGroupsAsync`, `GetRolesInGroupAsync` from the public groups API, kept; allies and enemies as an empty page; `PromptJoinAsync` through the editor's dialog), UserService (`GetUserInfosByUserIdsAsync`: the run's players here, the rest from the users API), SocialService (no friends to invite: `CanSend*InviteAsync` false, the prompts toast and fire their Closed events), PolicyService (an adult's policy without extra rules: ads allowed, the usual link references, not subject to China policies), VoiceChatService (voice off), AvatarEditorService (every `Prompt*` opens the editor's dialog, Allow (test) or Cancel, and fires its `*Completed` with `Success` or `PermissionDenied`; `GetAccessoryType`).
4. LocalizationService, LocalizationTable and Translator (`services/localization.luau`): a table's entries are its saved `Contents` JSON, read and written through the host (`GetEntries`, `SetEntries`, `SetEntryValue`/`Key`/`Source`/`Context`/`Example`, `RemoveEntry`, `RemoveEntryValue`, `RemoveTargetLocale`, `GetTranslator`); `GetTranslatorForPlayerAsync`/`GetTranslatorForLocaleAsync` give a Translator over every table under the service; `Translate` by source text, `FormatByKey` by key with `{1}`/`{name}` arguments (`:int`, `:fixed`), a regional locale falling back to its language, then to the source. Automatic translation of GUI text stays off.
5. TextService and the legacy Chat service (`services/text.luau`): `FilterStringAsync`/`FilterAndTranslateStringAsync` (server only) give a `TextFilterResult` whose `GetNonChatStringForBroadcastAsync`/`GetNonChatStringForUserAsync`/`GetChatForUserAsync` hand the text back unchanged (offline there is no filter to ask); `Chat:FilterStringAsync`/`FilterStringForBroadcast`, `Chat`, `Chatted`, chat callbacks, `CanUser(s)ChatAsync`. `GetTextSize`/`GetTextBoundsAsync` measure with `rbx_viewer::measure_text`: a typesetter fed the faces the renderer adopts (installed by the editor through `rbx_lua::server::set_text_measure`; headless runs keep the old per-character estimate).
6. ContextActionService on a touch device makes real buttons: an `ImageButton` with an `ActionTitle` label per touch binding in `PlayerGui.ContextActionGui.ContextButtonFrame` (our own look: round, dark, up the right side), which `SetTitle`/`SetImage`/`SetDescription`/`SetPosition` style and `GetButton` returns; pressing one runs the action. A touch device is the Device Simulator's phone or tablet (Test › Device); `UserInputService.TouchEnabled`/`KeyboardEnabled`/`MouseEnabled`/`GamepadEnabled` report it (`Services::device`). `GetStringForKeyCode` gives a US keyboard's characters. GuiService on a client: `Select`, `SelectedObject` firing `SelectionLost`/`SelectionGained`, `MenuOpened`/`MenuClosed` with our Escape menu, the emotes/inspect/paused-notification toggles, `SendNotification`/`DismissNotification`, `GetInsetArea`, `InspectPlayerFromUserId`/`InspectPlayerFromHumanoidDescription` as a toast (no inspect menu). StarterGui: `AvatarSwitcher` and `ExperienceShop` join the core GUI types, `ChatMakeSystemMessage` writes a system line in our chat window, `PromptSendFriendRequest`/`PromptUnfriend`/`PromptBlockPlayer`/`PromptUnblockPlayer` toast.
7. The editor: the test purchase dialog doubles as the other prompts' (Allow (test) for them), notices join the core GUI's toasts, `RBX_STUDIO_LOCAL_RUN=play|run` starts a run at launch (a screenshot aid).
8. Fixed: the `rbx_viewer` lib tests' occasional SIGSEGV on the NVIDIA 580 driver. Every GPU test made its own `wgpu::Instance` (with GL, so an EGL display per thread) and its own device, torn down concurrently; M10's lock only covered opening the adapter. Tests now share one instance per backend set (GL left out unless asked for alone) and one device per backend, opened under the lock and never dropped. Before: 1 crash in 30 runs at 16 threads; after: 20 in a row at default parallelism and 20 more at 16 threads, all 1696 tests passing. `luau_lsp::tests::a_spec_file_knows_testez_globals` asks for diagnostics again every 200 ms until the server has flagged the script (90 s deadline) instead of trusting one answer.

Goldens: six new tests (TeleportApi, BadgeApi, TextFilter, Localization, SocialPolicy and the client half ClientApis), run by rbx_sim, are **unrecorded**: recording publishes the suite over the sandbox place, which needs the Open Cloud key, and this milestone ran without keyring access (the key isn't in the config file or the environment). They're written to be recorded as they are (no awards, no teleports, no purchases: argument errors, result shapes, a reserved code, a TeleportOptions round trip, a LocalizationTable built and read in the test). Until then the messages for the argument errors, `TeleportInitFailed` and the policy table are ours, not measured. Three session tests cover the run-time paths (a teleport into the same place carrying data and GUI, a badge award with its toast, a touch button pressed).

COVERAGE.md: functions 281 to 372 of 1385 (20% to 27%), events 118 to 134 of 355 (33% to 38%), properties 1949 of 2510 (78%, unchanged). Coverage indexes members on a server, so the client-only GuiService additions and the UserInputService device flags aren't counted.

Checked live in the built editor with Play (local) on `M13Services.rbxlx` (in `rbx-native-targets/local-run-m13-shots/` with the screenshots), on the Device Simulator's iPhone 15 Pro: a LocalizationTable gives a French greeting and teleport counter (`GetTranslatorForLocaleAsync("fr")`, `FormatByKey` with named and numbered arguments), a SetCore `SendNotification` toast shows, a server script awards a badge three seconds in (the "Badge Awarded" toast with the name from the badges API; `AwardBadgeAsync` true the first time, false afterwards, `UserHasBadgeAsync` true), and a `BindAction` touch button titled with `SetTitle` teleports into the same place with `{hops = n + 1}` and a loading ScreenGui. The run restarted on its own (hops 1), then again from the button (hops 2): each arrival read the incremented `TeleportData` on the client and in `Player:GetJoinData()` on the server, and `GetArrivingTeleportGui` returned the loading GUI. The live check found three bugs, fixed and checked again: the teleport GUI copied into the next run showed as a root in the Explorer (the shown tree now skips anything under a parentless instance), and a press on a touch button missed on a simulated phone twice over: the pointer was measured in the view's pixels while the GUI lays out at the device's resolution and is drawn scaled, and the game's GUI layout ignored the phone's safe-area insets that the renderer keeps screens clear of (`Sim::set_safe_insets`, covered by the touch session test).

Gaps from M13:

| Area | Roblox | ours |
|---|---|---|
| Teleports | any place of any universe, a new server | the running place or a mapped place file; the run restarts |
| `TeleportInitFailed` messages | Roblox's | ours (no golden: recording needs the key) |
| Teleport GUI for a server-started teleport | the client's `SetTeleportGui` | only a client's own call carries one |
| Text filtering | Roblox's filter | none (offline pass-through) |
| `GetTextSize` headless | — | a per-character estimate (the editor measures with the renderer's faces) |
| Gamepad UI navigation | selection moves with the D-pad | `Select` and `SelectedObject` only |
| Inspect menu, friends, invites, voice | Roblox's | toasts and empty answers |
| Badge info offline | — | only what an earlier online run kept |

## Milestone 14 (done): newer systems

The engine systems Roblox shipped in the last two years, each against a golden recorded on the sandbox place (AudioApi, EditableApi, DragDetectorApi, InputActions, StyleRuntime), with the M13 goldens recorded alongside (the keyring fallback found the key this time). Goldens may now carry per-field `known_gaps` (a value no two runs share, a desktop-only string) instead of a whole-test gap.

1. The Audio API (`rbx_sim/src/audio_api.rs`, `audio_graph.rs`, `services/audio.luau`): `AudioPlayer`, `AudioDeviceOutput`, `AudioEmitter`/`AudioListener` (distance and angle attenuation curves), `AudioAnalyzer`, `AudioFader`, `Wire` with its pins and `Connected`, and the effects (`AudioReverb`, `AudioEqualizer`, `AudioEcho`, `AudioDistortion`, `AudioChorus`, `AudioFlanger`, `AudioTremolo`, `AudioPitchShifter`, `AudioCompressor`, `AudioFilter`), rendered on each client into the run's mix. Only `AudioFilter` (and so `GetGainAt`) is fitted to a real server's numbers; the reverb, chorus, flanger, distortion, tremolo and equalizer parameters are **our own guesses** from the creator docs' descriptions, not measured.
2. `EditableImage` and `EditableMesh` (`rbx_lua/src/server/editable_image.rs`, `meshes.rs`, `rbx_dom/src/editable.rs`): `AssetService:CreateEditableImage`/`CreateEditableMesh`/`CreateEditable*Async`, drawing with the measured blends (`DrawRectangle`, `DrawCircle`, `DrawLine`, `DrawImage`, `DrawImageTransformed`, `DrawImageProjected`, `WritePixelsBuffer`/`ReadPixelsBuffer`), meshes built and queried (vertices, faces, normals, UVs, colours, raycasts, `FindClosest*`), `Content.fromObject`, the budget errors. An EditableImage bound to an `ImageLabel`/`ImageButton` draws in the viewport and repaints when a script paints.
3. `DragDetector` and `UIDragDetector` (`services/drag.luau`): the axes kept as the detector's `Orientation` (Axis its X, SecondaryAxis its Y, the World ones through the reference frame, which a server reads as identity while nothing drags), angles clamped to ±180 and forces to 0 as the golden showed; on a client a left press on a part or model holding a detector starts a drag (`Camera` rays through the cursor, our own `ViewportPointToRay`/`ScreenPointToRay`/`WorldToViewportPoint`/`WorldToScreenPoint`/`GetRoll`/`SetRoll` now too), the drag styles (`TranslateLine`, `TranslatePlane`, `TranslatePlaneOrLine`/`TranslateLineOrPlane` with the mode-switch key, `TranslateViewPlane`, `RotateAxis`, `RotateTrackball`, `Scriptable` through `SetDragStyleFunction`, `BestForDevice` as a plane), translation and angle limits (a component limits when its min is below its max), `AddConstraintFunction` in priority order, `PermissionPolicy` with `SetPermissionPolicyFunction`, `RunLocally`. Unless it runs locally the client's cursor ray goes to the server (`Packet::Drag`), which moves the part and fires `DragStart`/`DragContinue`/`DragEnd` with the player; the client fires its own too. Responses: `Geometric` pivots, `Physical` drives an unanchored assembly by velocity with `Responsiveness`, `MaxForce`/`MaxTorque` (the server holds it while it drags), anchored ones pivot, `Custom` only fires. `UIDragDetector` drags its GUI object (`TranslatePlane`, `TranslateLine` along `DragAxis`, `Rotate`, `Scriptable`), `Offset`/`Scale`/`CustomOffset`/`CustomScale`, limits, `BoundingUI`, constraints, `DragUDim2`/`DragRotation`, `GetReferencePosition`/`GetReferenceRotation`. `ClickDetector.MouseClick`/`RightMouseClick` fire on a press and release over the part. Game scripts may no longer write a `RobloxScriptSecurity` property (`Highlight.LineThickness`), and `UDim2` prints as Roblox's `{0, 0}, {0, 0}`.
4. The Input Action System (`services/input_actions.luau`): an action's state starts at its type's zero, `Fire` checks the type (Roblox's message for a Bool), a Bool's `Pressed`/`Released` fire ahead of `StateChanged`, changing `Type` starts the state over; `GetInputBindings`/`GetInputActions` are `RobloxScript` only. On a client every key and mouse button goes to the enabled `InputContext`s, highest `Priority` first, and a context with `Sink` that has a binding for it keeps it from the lower ones; bindings read `KeyCode`, the `Up`/`Down`/`Left`/`Right`/`Forward`/`Backward` composites (Direction1D/2D/3D with `Scale`, `Vector2Scale`, `Vector3Scale`, `ClampMagnitudeToOne`) and the two modifiers.
5. UI styling at run time (`rbx_lua/src/server/styles.rs` over `rbx_dom::style`, the renderer's own cascade moved out of `rbx_viewer` so both share it): `GetStyled`, `GetStyledPropertyChangedSignal`, `StyledPropertiesChanged` (one per changed property, checked once a frame), `StyleRule:SetProperty/GetProperty/GetProperties/SetProperties`, `StyleBase:GetStyleRules/InsertStyleRule/SetStyleRules`, `StyleSheet:GetDerives/SetDerives`, tokens from attributes, derives, selectors by class, tag, name and child. A property a script set wins over every rule, and reading a property gives the unstyled value while `GetStyled` gives the styled one (the golden).
6. `Highlight` from scripts at run time: the renderer's silhouette pass reads the run's DOM, so adornee, colour and depth-mode changes show in Play (local).

Goldens: DragDetectorApi, InputActions and StyleRuntime are all ok (with AudioApi and EditableApi from earlier in the milestone); CONFORMANCE.md regenerated. A session test drags a DragDetector crate from a client (the server sees `DragStart` and the moved part) and a UIDragDetector frame, and checks the camera's projections.

COVERAGE.md: functions 372 to 480 of 1385 (27% to 35%), events 134 to 172 of 355 (38% to 48%), properties 1949 to 1950 of 2510 (78%).

Checked live in the built editor with Play (local) on `M14Systems.rbxlx` (in `rbx-native-targets/local-run-m14-shots/` with the screenshots), audio into a null sink: a StyleSheet-themed panel (frame colours, a `$Accent` token on the labels and on a tagged slider knob), a UIDragDetector slider (`TranslateLine`, `BoundingUI` on its track) dragged to 53, an EditableImage painted at run time on an ImageLabel (circles drawn once, squares repainted every half second), two DragDetector crates dragged with the mouse (the anchored Geometric one pivoted, the unanchored Physical one driven across the floor, each `DragStart`/`DragEnd` on the server with the player), a Highlight (`Occluded`) moved by the server onto whichever crate was dragged last, an AudioPlayer wired into an AudioEmitter heard by an AudioListener on the camera into an AudioDeviceOutput (`IsPlaying` and `Wire.Connected` true), and an InputAction on E changing the label. The Explorer listed no runtime-created objects as roots during play. Not verified by ear: the sink is null, so only the graph's state was checked.

Gaps from M14:

| Area | Roblox | ours |
|---|---|---|
| Audio effects | Roblox's DSP | `AudioFilter` fitted; reverb, chorus, flanger, distortion, tremolo, equalizer parameters guessed |
| EditableMesh on a MeshPart, EditableImage as a MeshPart/Decal texture | drawn | not drawn: the viewer caches meshes and textures by asset key in the streaming loader and GPU caches; needs versioned keys |
| Styles in the renderer | a script-set property beats the rules | `GetStyled` is right, but the viewport still draws the rule's value |
| Style pseudo-instances, `:Hover`-style states, transitions | applied | not applied at run time (`GetPropertyTransitions` and friends missing) |
| DragDetector VR and gamepad modes, trackball roll | Roblox's | mouse only; trackball turns by the angle between the two cursor points |
| UIDragDetector `DragSpace`/`DragRelativity` other than the defaults, rotation-aware hit tests | Roblox's | parent space, absolute, axis-aligned hit boxes |
| InputBinding `UIButton`, analog thresholds, pointer bindings | Roblox's | keys, mouse buttons and key composites only |

## Milestone 15 (done): look and feel next to the real client

The aim was a side-by-side with real Roblox an ordinary player couldn't tell apart. A place made from Home → New → Baseplate, plus a Rig Builder rig, leaderstats and two tools, ran in Play (local) and in the real client (Sober, published to the sandbox place by `rbx_sim/examples/record_look.rs`), with the same scripted camera views on F8. The paired screenshots are in `rbx-native-targets/local-run-m15-shots/` (`roblox-*` and `ours-*`, 1266×844 game views).

1. Baseplate template (`home/template.rs`). Studio's Baseplate has every surface Smooth and `SpawnLocation.Duration` 0, read in Studio's command bar. Ours left them out, so the real client filled in the class defaults: studs on the spawn and a ForceField bubble on the player. Both are now written. The grid `Texture`, the spawn `Decal`, plastic and the sky render alike on both sides.
2. Studio's newer MeshParts save their mesh and texture only as `MeshContent`/`TextureContent`. The viewer and the physics read `MeshId`/`TextureID`, so a Rig Builder rig drew as boxes with a ball head. Now they fall back to the `Content` twin (`textures::content_property`, `physics/part.rs`).
3. A MeshPart's texture now goes over its `Color` where it is see-through, so a dynamic head's face sits on grey skin. This is done by baking an opaque copy in `filemesh::over_color`, and the patch path asks for a full resolve until that copy exists. Before this, the transparent texels drew white.
4. Default animations:
   - Fixed: the first character's `Animate` never ran when the local player arrived after its character (`sim.rs` `start_scripts_under`).
   - Idle, walk/run, jump, fall and climb play on the signed-in avatar, the noob and the cached avatar.
   - `/e dance` (and wave, point, laugh, cheer, dance2/3) fires `Player.Chatted` on the client and is not sent as a message. The R6 Animate now has those states too.
5. Noob = the Rig Builder's R15 Masculine Block rig, read from one Studio built:
   - 2021 block meshes and cages, the dynamic head (`FaceControls`, no face decal), every part grey 127, HipHeight 2.
   - Built with Motor6Ds so our Animator drives it. Studio's own rig uses AnimationConstraints, which the sim does not drive yet.
   - The meshes come from Roblox or the asset cache. Offline with them missing, it falls back to the blocky R6 noob.
   - `CharacterAutoLoads` and `StarterCharacter` keep their M4 rules: a `StarterCharacter` replaces the noob or avatar.
6. In-game UI rewritten from captures of the current client (`workspace_view/core_gui.rs`, own SVG icons in `core_gui/icons.rs`, Builder Sans read from the Roblox content already on disk, the app's font otherwise):
   - Top bar: round menu button and a pill with the menu and chat toggles.
   - People list: top right, with a close X, Tab toggles it, leaderstats columns, display names.
   - Chat:
     - The window is hidden until toggled or `/`, with Here/Global/Friends tabs, the greeting line and the round bar with its hint.
     - The background fades after 8 s, leaving the lines.
     - Bubbles are grey pills with a tail.
   - Hotbar: 60 px translucent squares with a blue outline when equipped.
   - Health: the pill only once hurt, green through yellow to red.
   - Death: a red edge on the screen.
   - Escape menu:
     - People/Settings/Gallery/Report/Help tabs; Settings holds the volume.
     - Invite Friends and In this server rows.
     - Leave (L), Respawn (R) and Resume (Esc) buttons, with key caps.
     - Respawn and Leave each ask for confirmation first.
     - Chat and the People list hide while it is open.
   - `SetCoreGuiEnabled` toggles each part. `SetCore("TopbarEnabled", false)` hides the top bar and every core GUI, `ChatActive` false hides chat, and `ChatBarDisabled` stops typing (`CoreGui.chat/chat_bar/topbar`).
7. The editor's "studs/s" flight-speed label no longer shows during play.

Tests:
- Rig Builder rig structure (`shell/rig/tests.rs`).
- Template surfaces and Duration.
- Animate: the first character idles, walking and jumping, the emote command.
- CoreGui toggles, `TopbarEnabled` (`rbx_sim/tests/core.rs`).
- Core GUI key flows: chat, respawn and leave confirmations, Tab.
- The `Content` twin fallback.
- Every icon rasterizes.

Gaps from M15:

| Area | Roblox | ours |
|---|---|---|
| Rig Builder joints | `AnimationConstraint` + `BallSocketConstraint` | Motor6D (our Animator drives Motor6D only) |
| Dynamic head expressions | `FaceControls` animated (blinks, mood) | the face texture only, FACS not driven at run time |
| Humanoid name labels | a model with a Humanoid shows its name over the head ("Rig") | not drawn |
| Avatar headshots | the menu's player rows and the leaderboard show the player's thumbnail and membership/verified badges | an initial in a circle; no badges |
| Menu tabs | Settings has the full settings list; Gallery, Report, Help are real pages | volume only; the other three are one line each |
| Chat channels | Global/Friends tabs filter messages; emoji picker, mic | one channel shown under Here; the icons are decorative |
| Plastic grain | Roblox's plastic is nearly flat at a distance | our plastic noise is stronger |
| Health bar colours | measured at 60% (yellow-green) | the 100% and 0–30% ends are by eye |
| Death vignette | Roblox's red edge | a gradient on each edge, its strength by eye |
| `script.Enabled` | reads true for a running script | reads false (getter default) |

Not verified: the offline cached avatar was not re-captured this milestone; the real client was compared on Linux (Sober) only.

## Milestone 16 (done): the M15 differences, gameplay goldens, newer APIs

The M15 side-by-side gaps were closed first, then gameplay that M10–M11 hadn't recorded was checked against a live server, then the newest script APIs. New captures are in `rbx-native-targets/local-run-m16-shots/` (`sbs-*`: real client left, ours right, the real side from M15).

1. Hotbar (`gui.rs` `HotbarSlot`, `core_gui.rs`):
   - A tool's `TextureId` is fetched on a thread (`rbx_viewer::asset_bytes`) and drawn in its slot instead of the name.
   - Slots 3 and 4 in the M15 real capture were the player's own gear, which the experience allows. They aren't in the place.
2. Humanoid name labels (`gui/labels.rs`): a Humanoid model other than your own character shows its name, or `DisplayName`, 1 stud over the head. It's 16 px white with a (51,58,64) edge, as measured. A health bar shows under it.
   - It follows `DisplayDistanceType` (Viewer, Subject, None), `NameDisplayDistance`/`HealthDisplayDistance`, `HealthDisplayType` and `NameOcclusion`.
   - Occlusion is a raycast from the camera.
3. Shadows: a soft edge never narrows below 2 shadow texels (`lighting.rs` `kernel_radius`). At low softness the penumbra used to step.
4. Plastic: the grain normal map is mixed at 40% (`material.wgsl` `PLASTIC_GRAIN`). Studs keep their relief.
5. `script.Enabled` reads and writes `not Disabled` on any `BaseScript`.
6. Flaky `the_first_character_idles`: the idle crossfade could catch both idle animations playing at once (2 runs in 30). That state is accepted now.
7. The escape menu hides the hotbar. Re-captured (`sbs-7`).
8. Gameplay goldens, 3 passes each on a live server:

   | Test | Measures | Result |
   |---|---|---|
   | `VehicleSeat` | sit, throttle, steer, jump off (`Seated>Jumping@25`, `Landed@53`) | all match |
   | `SwordTouch` | `Touched` from a walking character into a blade | `touch_beats` 34 vs 35, touched by Head, walked 9 |
   | `CameraLimits` | wheel zoom against `CameraMin/MaxZoomDistance`, changing the limits mid-zoom | all match |
   | `HumanoidDeath` | `Died` timing, Motor6Ds left, `CanCollide` after death, head and hand 15/60 beats later, with and without `BreakJointsOnDeath` | match, except the broken body's collapse (known gap) |
   | `NewApis` | the newest API surface below | all match |

   - Camera: the wheel goal is kept unclamped and clamped where it's used. Roblox does the same: after twelve notches in at minimum 6, widening the limits shows 3.33.
   - Death: limbs collide again after `Died` when `AvatarCollisionRules.BodyPartsCollidableOnDeath` is true (the default). So an unbroken body stays where it fell, as in Roblox.
   - Death: an undriven root (dead, `PlatformStand`) falls under gravity again. A settled root used to stay weightless.
9. Newer APIs, each checked live before it was built (`NewApis`):
   - `RunService:BindToSimulationParallel`/`BindToAnimationParallel` error with Roblox's message outside an `Actor`. Inside one they run on `PreSimulation`/`PreAnimation` at the given `StepFrequency`, serially.
   - `AvatarCollisionRules` defaults: `HitboxSize` 2,5.5,1, `BodyPartsCollidableOnDeath` true, `TouchAndQueryDetectionMode` UseParts.
   - Scripts get Roblox's capability error for `RobloxScriptSecurity` reads (`HitAndTouchDetectionMode`, `LegacyCollisionMode`) and for `Workspace.NextGenerationPhysicsSolver` (PluginOrOpenCloud).
   - `AudioStreamReader`/`Writer`:
     - Defaults: Mono, Float32, 48000, Medium.
     - Pins: Reader `Input`, Writer `Output`.
     - `Read`/`Peek`/`Write`/`Space` fail with "… is not enabled", as live servers do today.
   - `Humanoid:GetCanCollideOnDeath` fails with "is not yet enabled", as on live servers.
   - `game.ServerRegion` reads `"Local"`.
   - `Humanoid.UsesAvatarCollider` is false.

Tests:
- `rbx_sim/tests/core.rs` `tool_icons_rig_labels_and_script_enabled`.
- Label rules (`gui/labels.rs`).
- Shadow kernel radius.
- The five conformance tests above.

Gaps from M16:

| Area | Roblox | ours |
|---|---|---|
| Broken body on death | the R15 stack topples: head on the floor by 60 beats, a hand 0.8 stud away at 15 | the torsos drop out and the rest stays balanced (head −0.17, hand 2.04) |
| Shadow colour | core (19,37,74), lit floor (121,129,147) | core (9,24,64), floor (132,137,152). Exposure left alone: the Studio-fitted fidelity goldens would regress |
| Spawn point | a random point on the SpawnLocation | its centre |
| Player gear | gear the experience allows fills hotbar slots | not loaded |
| Parallel Luau | runs in parallel inside Actors | serial |
| AudioStream read/write | disabled live too | disabled; nothing to compare yet |
| Hotbar icons | drawn | drawn, but not seen live: the look place's tools have no `TextureId` |

Not verified: hotbar icons on a live capture; the camera's shift lock and the truss/stairs/slope rows beyond M10/M11's goldens; the real client on Windows or macOS.

## Milestone 17 (done): real games, played next to the real client

Four of the maintainer's own published games, played from local copies in Play (local) and compared with the same games joined in the real client. The games were only read: nothing in them was changed, published or saved, and none of their files or script names are in the repo; the tests use minimal places of our own.

| Game | Before M17 | Now |
|---|---|---|
| Physics sandbox (rules card, tools, ragdolls) | ran | still runs; same spawn, rules card and hotbar |
| Rolling game (roll button, leaderstats) | ran | still runs; same spawn and roll |
| Camera-watching game, lobby place | menu only: no lobby dummy, "Waiting for the host", code 000000 | menu, lobby with its code, "Start game", the dummy dressed and placed |
| Camera-watching game, game place | stuck on "Initializing scripts", then kicked | loads, cameras switch, the guide opens and closes, anomalies fire on schedule |
| Swim game (plots, water, carrying) | 5 s of play in 212 s | spawns at the plot with its daily-rewards window, as live; 3 s in about 90 s once its meshes collide as meshes |

Fixed for them:
- Engine:
  - incremental world: parts and joints that come, go or change shape add, remove or refit only their own assemblies, in a sorted order;
  - the query index follows the change log;
  - humanoids are found from the change log;
  - joining no longer clones the whole DOM;
  - touch pairs come out older-part first (TouchedOrder);
  - a mesh whose convex decomposition comes out empty collides as its box instead of crashing.
- Movers: BodyPosition, BodyVelocity, BodyGyro, BodyAngularVelocity, BodyForce and BodyThrust.
- Avatars:
  - clients get the avatar provider;
  - a client may `ApplyDescription` on a humanoid it made itself;
  - `GetHumanoidDescriptionFromUserId` yields like the web call it is.
- Model and instance behaviour:
  - `Model:MoveTo`, rising out of what it would overlap;
  - a destroyed instance's properties stay readable (the newest 16k).
- Joining: the client's own character lands a frame after the join snapshot, so a ReplicatedFirst loader waiting on `CharacterAdded` sees it fire.
- APIs:
  - `ReplicatedFirst:RemoveDefaultLoadingScreen`, `SetDefaultLoadingScreen`, `IsDefaultLoadingScreen`, `IsFinishedReplicating`;
  - `SocialService:GetExperienceEventAsync`;
  - GuiObject's legacy `TweenPosition`/`TweenSize`/`TweenSizeAndPosition`;
  - `Humanoid.RootPart`;
  - default leaderstats values.
- Output:
  - error tracebacks name scripts only, with no engine chunks;
  - a kicked client's last lines are kept.
- Rendering: a size-0 blur and a collapsed UIScale draw nothing.

Tests: `rbx_sim/tests/corpus.rs` (BodyMovers, a BodyPosition holding a character, the query index following parts, `Model:MoveTo`, the ReplicatedFirst loader, destroyed-instance reads), `tests/avatars.rs` (client dummy), the empty-decomposition mesh test.

## Milestone 18 (done): what M17 left off

A fix milestone: the M17 side-by-sides reviewed by the maintainer, and the gaps M17 listed. The four games were again only read (local copies, plain joins in the real client); probes ran on our own places and the sandbox.

Speed (wall seconds per simulated second, headless `play`):

| Place | Before | Now |
|---|---|---|
| Swim game, steady state | ~20 (8 s took 220 s) | ~6.9 (12 s in 83 s) |
| Camera game, lobby place | 1.5 | 0.36 |
| 1200 resting mesh props (`bench_props`), settling | 131 ms a frame | 19 ms |
| the same, at rest | 0.4 ms | 0.5 ms |

What did it:
- rapier's `parallel` feature (the narrow phase on all cores), CCD off (Roblox has none; every golden unchanged, one HumanoidStep value closer);
- bodies asleep since the last frame are skipped by write-back, terrain and water forces, and the query index refreshes only what moved;
- client sims drop kinematic–kinematic pairs (remote assemblies) and ask contact pairs of remote parts only when a script listens to their touches;
- one shared convex decomposition per mesh asset instead of one per part;
- animation clips are cloned once when they load, not every frame (37% of the lobby's time);
- the pose step borrows joint names instead of allocating them.

Fixed:
- Lobby dummy: `Instance:Clone()` copied references verbatim, so a cloned rig's joints still pointed at the original's parts and the clone fell apart (the head dropped through the floor and the humanoid died). References inside the copied subtree now point at the copies, outside ones stay. This touches every cloned model with joints, welds, `PrimaryPart` or `ObjectValue`s.
- Name billboards adorned to a Model (or under one) sit on the model's bounds, so the dummy's name shows.
- People list:
  - Roblox's measured geometry: width 165 + 77 per stat, 62 from the top, 40 px rows, 13 px headings, 16 px names;
  - the owner and verified badges, drawn as our own SVGs; the owner comes from the experience's creator (a group's owner for a group game, plus the player when their key manages the experience), the verified flag from the user API, both cached in the avatar cache for offline runs;
  - names, headings and values cut with an ellipsis, never wrapped;
  - numbers as the real client prints them: commas, then K/M/B past 7 characters with tenths below 100 (`12.3K`, `1.2M`, `766,001`, `1,000B`), checked on the sandbox;
  - stats in added order, `IsPrimary` first, then `Priority` high to low; BoolValues not listed; four stats at most.
- Random spawn point: a random spot on the spawn's top, 3 studs in from each edge (6x6 spawns still land at the centre), as live on a 40x40 spawn.
- The query index sees anchored parts that animation posing moves (posing reports what it moved).
- Destroyed instances stay readable while a script holds them, with no 16k cap.
- Auto-Recovery no longer copies the DOM during a Local Run (its run-time GUI values made the `AbsolutePosition` warning).
- Perf regression guards: `tests/busy.rs` (256 settled mesh props fall asleep and stay put) and `examples/bench_props.rs` (1000+ props, mean and worst frame per second).

Part 2:

- Speed. Loose props come out of deep overlaps in the frame they're found (`DEEP_OVERLAP`, `RESTING_DEPTH`) and fall asleep one by one: a body still for 0.35 s under 0.6 stud/s freezes unless a fast, kinematic or driven partner touches it, and keeps its touches while frozen. A server-owned part a client script moves is put back from the server's pose next frame, so server sleep no longer drops replication. Place loads hash referents with Fx, parent in bulk and decompose each mesh once (in mesh space, scaled per part). The swim game: 8 s of simulation took 64 s, 30 s now take ~109 s, under 2x real time after its first 10 s (29–38 ms frames); first frame 17.3 s → ~12 s. `bench_props --pile` (1200 buried and overlapping props): 7 ms frames in the first second, 2.1 ms at rest; `busy.rs` checks the pile comes out and sleeps. The two load-flaky tests no longer read wall time (`Session::set_script_timeout`).
- Lighting. ColorGradingEffect `Retro` is the Voxel look under any Technology (ramps ΔE 11–14 → 2.9), which is most of the camera game's darkness. Under Voxel at night the moon is 0.14 of ours and ambient lights at 0.4 (`game_voxel_night` ΔE 4.64 → 0.68). New fidelity scenes and goldens: the Retro ramps, a lobby-like room, a midnight room three ways, a day path under a game's effect stack.
- Death. A dead character's root is a loose part, so a broken R15 topples (HumanoidDeath `breaks_head_dy_60` -3.78, live -3.86).
- Parallel Luau, run serially (`engine/parallel.rs`, `parallel.luau`): `task.desynchronize`/`synchronize`, `ConnectParallel`, `BindToSimulationParallel`/`BindToAnimationParallel` only under an Actor; in the parallel phase property writes and mutating calls fail with a live server's messages; `SharedTable`, `SharedTableRegistry`, `Instance:GetActor` (ParallelLuau golden, 49 keys).
- `Model:MoveTo` rises out of exact shapes and never out of terrain or water (ModelMoveTo golden).

Still off:

| Area | Roblox | ours | Why |
|---|---|---|---|
| Swim game in Play (local) | plays in seconds | the place opens in 3 s and the run starts within 40 s, but the game's own loading screen was still up after 90 s | not traced; its join-group prompt also still blocks until answered |
| Stat column order | added order when the folder is parented whole; other orders when stats are added one by one | always added order | no rule found; not probed again |
| Camera game lobby | wall 89, floor 92 (sRGB, M17 capture) | 79, 76 (was 64, 61) | closer after Retro; the M17 reference is a small window, so not a strict match |
| Day shadow under a heavy effect stack | path shadow (111,153,216) | (159,189,238) | `game_day_effects` ΔE 2.30; the in-game sample went the other way, so the effects' handling of shadow needs its own probe |
| Hard shadow core | (32,62,84) | (23,50,75) | `shadows_hard` |
| Broken R15 early fall | a hand 0.78 stud away by 15 beats | 2.02 | the pieces part quicker at first |
| R6 climb speed | 11.2 | 10.9 (10.89 once the world has had terrain) | truss contact drags it; known gap |
| Pile and stack sleep | pile still by frame 32; a 5-high stack creeps | 60–75; the stack freezes | SleepPile, BoxStack known gaps |
| Jump apex | 15.48 | 15.28 | live apexes scatter ±0.15 around √(2gh); no formula fits |
| Parallel Luau | runs in parallel | serial, same results and errors | one VM per peer |
| `AwardBadge` on an owned badge | unverified live | returns false | the sandbox universe has no badge to probe with |
| First-frame replicated nil, HD Admin `clean`, the camera game's "Level" column | — | not traced | |
| Slopes, buoyancy, swimming | as live | M10/M11 goldens only | the swim game's loading screen still blocks walking it |

Not verified: the swim game past its loading screen (walking, carrying, the sea), the camera game's anomaly reports, the real client on Windows or macOS.

Side-by-sides: `rbx-native-targets/local-run-m18-shots/<game>/` (real client and ours at 1166x632; part 2's are suffixed `-b`, ours cropped from a 1600x636 Play (local) view, so the 3D framing matches and the corner GUI is cut).

### M18 API pass: characters, avatars, animation, players

Every scriptable member of these classes answers as a live server's does: same return shapes, errors, yields and events, from two goldens recorded on the sandbox place (`AnimationApi`, `AvatarApi`) and `tests/avatar_api.rs`. Most of it is Luau (`services/avatar.luau`, `services/animation.luau`); curves are Rust (`server/curves.rs`, keys in `datatypes/extra/curve_key.rs`).

- Curves. `FloatCurveKey`, `RotationCurveKey` and `ValueCurveKey` are datatypes (interpolation defaults to Cubic, a right tangent on a key that isn't Cubic is refused, `tostring` is `{Linear, 1, 2, nil, nil}`). `FloatCurve`, `RotationCurve` and `MarkerCurve` keep their keys in `ValuesAndTimes`, so copies keep them and the Animator plays them: `InsertKey` answers `{inserted, index from 0}`, the other indices count from 1, `GetKeyIndicesAtTime` clamps to a segment, `SetKeys` sorts and drops repeats, unset tangents are flat. `Vector3Curve`/`EulerRotationCurve` read X, Y, Z (`X()` makes the channel). `ValueCurve` answers empty on a live server, so ours does.
- Keyframes and providers. `KeyframeSequence`/`Keyframe`/`Pose` add, list and remove children, silently ignoring the wrong class and nil; `GetPoses` lists Poses only. On a server `RegisterActiveKeyframeSequence`, `RegisterAnimationClip` and `RegisterActiveAnimationClip` fail with Roblox's usage errors; `GetKeyframeSequenceAsync` and `GetAnimationClipAsync` fetch the asset (cache, then the app's fetcher) and word a failure as Roblox does.
- Animator and tracks: `GetTrackByAnimationId` (a playing track only), `ApplyJointVelocities`, `StepAnimations` (capability error), `RegisterEvaluationParallelCallback` (not enabled); `AnimationTrack` parameters answer empty and its target members say "not available yet", as live.
- Rig descriptions: `HumanoidRigDescription` joint lists and joints by `Enum.RigLabel`; `DigitsRigDescription` reads zero; `IKControl` readings are empty without a solve; `AnimationNodeDefinition` pins (duplicates allowed, every call fires `InputPinsChanged`).
- Humanoid: `AddAccessory` (a matching attachment: the handle moves onto it, held by `AccessoryRigidConstraint` with an `OriginalSize`; none: a `HeadWeld` in the head by `AttachmentPoint`), `GetAccessories`, `RemoveAccessories`, `GetBodyPartR15`, `GetLimb`, `ReplaceBodyPartR15` (the new part joins where it stands, the old part and its joints go), `BuildRigFromAttachments`, `GetMoveVelocity`, `GetRelativeVelocityAtFloor`, `ApplyDescriptionAsync`/`ResetAsync` with `ApplyDescriptionFinished`, `ApplyAvatarRulesAsync`, `PlayEmoteAsync`, `Touched` (a character part's touch), `Strafing`. The physics joins a `RigidConstraint`'s parts like a weld.
- HumanoidDescription emotes (`Add/Remove/Get/SetEmotes`, `Get/SetEquippedEmotes`, their events) and accessories (`GetAccessories(includeRigid)`, `SetAccessories`), `AccessoryDescription:GetAppliedInstance`; the body colours default to black.
- Player: `GetCameraState` (needs Server Authority), `ClearCharacterAppearance`, `ClearCachedAvatarAppearance`, `PromptAgeCheck`, `LoadCharacterWithHumanoidDescriptionAsync`, `LoadCharacterWithAvatarRulesAsync`, `Idled`, `OnTeleport`, `EmotesLoaded`. Players: `BanAsync`/`UnbanAsync` validate exactly as live and keep a run-only ledger (a banned player present is kicked; nothing reaches Roblox), `GetBanHistoryAsync` answers that ledger as `BanHistoryPages`, `GetHumanoidDescriptionFromOutfitIdAsync` fetches the outfit through the avatar pipeline, `GetProfileConfigurationFromUserIdAsync`, `PlayerMembershipChanged`, `UserSubscriptionStatusChanged`. Teams `GetTeams`.
- AvatarEditorService: avatar rules, item and batch details, outfit details, bundles by asset, recommendations, `SearchCatalogAsync` (`CatalogPages` over the web catalog), `CheckApplyDefaultClothingAsync`, `ConformToAvatarRulesAsync`; inventory and outfits are client-only and head shapes 401 on a server, as live. AvatarCreationService `GetValidationRules`; its generation, creation prompts and UGC validation make the live server's first checks (a missing argument, an empty token list, preview images only on a client, an empty generation id) and past them say they need Roblox's services. `AnimationClipProvider:GetClipEvaluatorAsync` likewise.
- Coverage for these classes (functions there/total): Humanoid 11/24 → 24/24 (events 15/18 → 18/18), HumanoidDescription 0/8 → 8/8, Animator 2/6 → 6/6, Player 16/24 → 22/24 (the two left are friends lookups, routed elsewhere), Players 11/16 → 16/16, AvatarEditorService 9/22 → 22/22, AvatarCreationService 0/16 → 16/16, the curves, keyframe classes, providers, rig descriptions, IKControl and node pins 0 → all. Overall functions 516 → 661 of 1385.
- Enum lookups fail as `X is not a valid member of "Enum.Y"`.

Still off, and why:

| Member | Roblox | ours | Why |
|---|---|---|---|
| `BuildRigFromAttachments` | AnimationConstraints + BallSocketConstraints + NoCollisionConstraints (the sandbox place has `AvatarJointUpgrade` rolled out) | Motor6Ds, as a place without the upgrade | AnimationConstraint rigs aren't simulated; every other character path is Motor6D |
| `GetBanHistoryAsync` in Studio Play | docs: doesn't work in Studio | the run's ledger | the Studio wording isn't recorded; a live server answers empty pages |
| Jump apex (JumpHeight 15) | 15.48, vy1 75.09 | 15.28, vy1 74.27 | giving fast take-offs a substep of gravity back fixes vy1 but peaks 0.13 high and lands a frame late; the live per-frame rise runs ~0.3 stud/s under ours in every jump while vy1 reads equal, so it looks like in-frame position integration, not take-off speed |
| Slopes (HumanoidSlope 60°, MaxSlopeAngle) and R6 climb speed | as M10 | unchanged | not attempted again: the floor sensor's wall-and-edge behaviour on steep wedges and the truss drag are physics work outside this pass |
| `PlayEmoteAsync` with a description's emotes | plays through `Animate` | plays through `Animate`'s `PlayEmote` bindable when the rig has one | no emote recorded live (needs a published emote on a real avatar) |

## Milestone 19 (proposed): multi-client testing, then what M18 left off

1. Studio-style multi-client tests: Test → Clients and Servers, N player windows plus a server window, each with its own Explorer and Output, all fed by one `Session`.
2. Collaborators from our co-edit session (our own presence and p2p, never native Team Create) joining a Local Run as players.
3. The M18 leftovers above, the swim game's loading screen first.

Later, the old animation proposal:

1. Layered clothing at run time: `WrapLayer`/`WrapTarget` deformation on the avatar, layered in `Order`, with the auto-rig cages the editor already fits (`rbx_animation`), and `AutoSkin`.
2. FACS face animation: `FaceControls` properties driven by animation tracks and scripts, dynamic heads deforming in the viewport, `Animator` playing face curves.
3. Blending exactness: weights, fades, priorities (`Core` to `Action4`), `AdjustWeight`/`AdjustSpeed`, additive tracks, measured against goldens of Motor6D transforms on a real server.
4. `IKControl` (`Transform`, `Position`, `Rotation`, `LookAt` types, `Pole`, `Weight`, `SmoothTime`, `ChainRoot`) and `AnimationConstraint` driven by tracks.
5. `Bone`s and skinned meshes: bone transforms from animations deforming skinned `MeshPart`s in the viewport, `Bone.TransformedWorldCFrame`.
6. Goldens for each: Motor6D and bone transforms sampled per frame from a real client, face-control values, IK end-effector positions.

## Where things are

- `crates/rbx_lua/src/server.rs`, `server/hooks.rs`: the server VM.
- `crates/rbx_lua/src/server/query.rs`: spatial queries (Luau side). `server/meshes.rs`: `EditableMesh` and `CreateMeshPartAsync`.
- `crates/rbx_sim/src/session.rs`: a server and its clients, replication. `ownership.rs`: network ownership. `crates/rbx_lua/src/server/remote.rs`: remotes and their argument rules.
- `crates/rbx_sim/src/sim.rs`: the frame loop.
 `physics.rs`, `physics/part.rs`: Rapier world, shapes, joints, adaptive stepping. `physics/constraints.rs`: constraints and movers. `physics/mesh.rs`: MeshPart collision. `spatial.rs`: the query index. `scripts.rs`: which scripts run. `load.rs`: place files. `conformance.rs`: the suite.
- `crates/rbx_sim/examples/`: `run` (CLI), `conformance` (table), `record_goldens` (real server), `coverage` (COVERAGE.md).
- `crates/rbx_sim/src/terrain.rs`: terrain at run time (the Terrain API's edits, collision meshes, water, rays); `crates/rbx_lua/src/server/terrain.rs`: its Luau side. Water forces and terrain colliders are in `physics.rs` (`water_forces`, `sync_terrain`).
- `crates/rbx_sim/src/humanoid.rs`, `physics/character.rs`: humanoids. `players.rs`: the Players service and the local player. `character.rs`, `character/`: the noob, our Animate and Health scripts. `animation.rs`: Animator and AnimationTrack. `engine.rs`: the engine methods, signals and properties the VM asks for.
- `crates/rbx_studio/src/shell/local_run.rs`: the `local-run` beta: Run, Play (local), Pause, Stop live in the editor. `local_run/avatars.rs`: who plays and what they look like. `workspace_view/play_input.rs`: the keyboard and mouse handed to the game.
- `crates/rbx_sim/src/input.rs`, `crates/rbx_lua/src/server/input.rs`, `server/input.luau`: UserInputService, ContextActionService, BindToRenderStep. `crates/rbx_sim/src/character/PlayerModule.luau`: our default controls and camera.
- `crates/rbx_net`: offline mode, the reachability probe and the HTTP gate.
- `crates/rbx_sim/src/gui.rs`, `gui/layout.rs`: PlayerGui at run time, GUI input, `gui/labels.rs` Humanoid name labels, TextBox, StarterGui/GuiService core calls. `tools.rs`: Backpack and tools. `teams.rs`: teams. `sound.rs`, `audio.rs`: Sound playback, decoding and roll-off. `chat.rs`: TextChatService's default instances. `crates/rbx_studio/src/workspace_view/core_gui.rs` (icons in `core_gui/icons.rs`): our default top bar, health, hotbar, People list with leaderstats, chat window and bubbles, Escape menu with its confirmations, death edge and kick card. `crates/rbx_studio/src/shell/local_run/sound.rs`: the run's audio mix.
- `crates/rbx_lua/src/server/tween.luau`, `tween.rs`: TweenService. `server/chat.luau`, `chat.rs`: TextChatService messages.
- `crates/rbx_lua/src/server/services.rs`, `server/services/*.luau`: the web-backed and social services, a Luau module each (HttpService, DataStore, MemoryStore, Messaging, Marketplace, Pathfinding, Teleport, Badges, Group/User/Social/Policy/AvatarEditor/Voice, Text and Chat, Localization, GuiService), with the editor's half (teleport restarts, toasts, prompt dialogs) in `crates/rbx_studio/src/shell/local_run.rs`.
- `crates/rbx_sim/src/audio_api.rs`, `audio_graph.rs`, `rbx_lua/src/server/services/audio.luau`: the Audio API and its mixer. `rbx_lua/src/server/editable_image.rs`, `meshes.rs`: EditableImage and EditableMesh. `services/drag.luau`: DragDetector, UIDragDetector, ClickDetector clicks and the Camera's screen math. `services/input_actions.luau`: the Input Action System. `rbx_lua/src/server/styles.rs` with `rbx_dom/src/style.rs`: StyleSheets at run time.
