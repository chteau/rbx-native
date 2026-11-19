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
| HingeConstraint (free, Motor, Servo, limits) | Rapier revolute joint on the attachments' X axes, velocity motor | a servo turns at `AngularSpeed` straight to its target (HingeServo golden) |
| PrismaticConstraint | Rapier prismatic joint, velocity motor | |
| BallSocketConstraint | Rapier spherical joint | rigid; Roblox's stretches about 0.03 stud |
| RopeConstraint | Rapier rope joint | the body falls on adaptive steps while the rope is slack |
| SpringConstraint | Hooke's law plus damping between the attachments, every substep | |
| AlignPosition, AlignOrientation | critically damped pull, natural frequency `Responsiveness`/2, holds against gravity | fitted to the goldens (both follow the same curve); `RigidityEnabled` stands in as Responsiveness 200 |
| LinearVelocity, AngularVelocity | reach the target within a substep as far as `MaxForce`/`MaxTorque` allow | LinearVelocity holds against gravity |
| VectorForce, Torque | a force every substep | |
| Attachment | `WorldPosition`, `WorldCFrame`, `Axis`, `SecondaryAxis` and the rest read and write | |
| Restitution | Rapier's, but an impact (approach above 1.5 stud/s on several contact points) is solved at one point, so a flat box bounces straight | BoxBounce golden: e ≈ 0.5 |
| Angular damping | 0.052 /s, exponential | Damping golden: 10 rad/s → 9.019 after 2 s; no linear damping |
| Anchored | fixed body | |
| Spatial queries | `Raycast` (RaycastParams: filter Include/Exclude, `RespectCanCollide`, `CollisionGroup`), `Blockcast`, `Spherecast`, `Shapecast`, `GetPartBoundsInBox`, `GetPartBoundsInRadius`, `GetPartsInPart` (OverlapParams), legacy `FindPartOnRay*` | against the same shapes physics collides with. Measured: a ray ignores the part it starts in, `CanQuery` false is never hit, casts ignore parts they start inside, the bounds queries test a part's box (a ball counts as its cube), `GetPartsInPart` is exact |
| CanCollide false | sensor collider | still fires Touched when CanTouch |
| CanTouch | collision events on or off | both parts need it, as documented |
| CollisionGroups | 32 groups as Rapier interaction bitmasks | `PhysicsService` API, Default always there |
| Welds, WeldConstraint, Motor6D, other JointInstances | one rigid body per assembly | union-find over joints, root part = biggest volume. Motor6D is rigid (no animation yet) |
| Material, CustomPhysicalProperties | density, friction, elasticity and their weights | weighted mix in a contact hook, Roblox's documented formula |
| Mass, AssemblyMass, GetMass | from density × volume | |
| AssemblyLinearVelocity, AssemblyAngularVelocity (and Velocity, RotVelocity) | read and write | |
| ApplyImpulse, ApplyAngularImpulse | applied before the next step | |
| Workspace.Gravity | yes | default 196.2 |
| FallenPartsDestroyHeight | parts below it are destroyed (Destroying fires) | default -500 |
| Network ownership | no-ops | one server, no clients |

## Conformance

The suite is 33 Luau tests in `crates/rbx_sim/conformance/tests/` (shared helpers in `Harness.luau`). The same modules run in a real Roblox server and in rbx_sim and print one JSON line each. Each test starts right after a Heartbeat, at its own origin (`2000 + 300 i, 300, 2000`) so tests never touch each other.

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

`docs/research/local-run/COVERAGE.md` is generated (`cargo run -p rbx_sim --example coverage -- docs/research/local-run/COVERAGE.md`): a server script in a fresh Local Run makes an instance of every class and indexes every member a game script may touch. Today: 112 of 1385 functions (87 after Milestone 1, 96 after Milestone 2; the new ones are Humanoid, Players, Animator and AnimationTrack), 53 of 355 events (31 after Milestone 2), 1926 of 2510 properties. The property number flatters us: any property the reflection database knows reads back as data, whether or not we act on it. The physics table above is what's really simulated.

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

## Milestone 4 (proposed): input, clients, offline

1. Input and camera polish: shift lock, first person at minimum zoom, the camera pulled in by walls (popper), touch and gamepad through the device simulators, `UserInputService` and `ContextActionService` for game scripts, the mouse lock behaviour of real Play.
2. Client/server split and replication: a client VM per player running LocalScripts against a replicated copy, RemoteEvent and RemoteFunction with a golden for the round trip order, network ownership of the character, several local players.
3. Offline mode for the whole app: Home, the editor, the asset and avatar caches and Local Run usable with no network, with what needs Roblox clearly greyed out.
4. Leftovers from Milestone 3: the turn rate for reversals, JumpHeight's launch speed, steep slopes (the top edge and sliding back down a slope past MaxSlopeAngle), acceleration and turning for heavy and fast characters, climbing starting a frame late and the R6 truss flicker, the seat-jump landing frame, swimming (needs terrain water), body-part meshes and scales in `ApplyDescription`, `Players:CreateHumanoidModelFromDescription`.
5. Leftovers from Milestone 2: soft contacts, soft joints, the VectorForce extra frame, the sideways adaptive switch, union collision, and step one frame while paused.

## Where things are

- `crates/rbx_lua/src/server.rs`, `server/hooks.rs`: the server VM.
- `crates/rbx_lua/src/server/query.rs`: spatial queries (Luau side). `server/meshes.rs`: `EditableMesh` and `CreateMeshPartAsync`.
- `crates/rbx_sim/src/sim.rs`: the frame loop. `physics.rs`, `physics/part.rs`: Rapier world, shapes, joints, adaptive stepping. `physics/constraints.rs`: constraints and movers. `physics/mesh.rs`: MeshPart collision. `spatial.rs`: the query index. `scripts.rs`: which scripts run. `load.rs`: place files. `conformance.rs`: the suite.
- `crates/rbx_sim/examples/`: `run` (CLI), `conformance` (table), `record_goldens` (real server), `coverage` (COVERAGE.md).
- `crates/rbx_sim/src/humanoid.rs`, `physics/character.rs`: humanoids. `players.rs`: the Players service and the local player. `character.rs`, `character/`: the noob, our Animate and Health scripts. `animation.rs`: Animator and AnimationTrack. `engine.rs`: the engine methods, signals and properties the VM asks for.
- `crates/rbx_studio/src/shell/local_run.rs`: the `local-run` beta: Run, Play (local), Pause, Stop live in the editor. `local_run/avatars.rs`: who plays and what they look like. `workspace_view/play_input.rs`: the keys and the character camera.
