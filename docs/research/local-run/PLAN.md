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

The suite is 56 Luau tests in `crates/rbx_sim/conformance/tests/` (shared helpers in `Harness.luau`). The same modules run in a real Roblox server and in rbx_sim and print one JSON line each. Each test starts right after a Heartbeat, at its own origin (`2000 + 300 i, 300, 2000`) so tests never touch each other. Tests with a client half (Milestone 5) call `Harness.client(name, ...)`, which runs the module `name` from `conformance/client/` on the player's client (a LocalScript in StarterPlayerScripts answers a RemoteFunction) and returns what it returns; a test that needs real input prints `CONFORMANCE_GESTURE wheel in 1` / `drag 100 0` / `key LeftShift`, which the recorder turns into XTest input on the Sober window and rbx_sim into Session input.

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

## Milestone 8 (proposed): rendering fidelity

Side-by-side pixel comparison against the real client: the same place, camera and time of day rendered by Sober on Xephyr and by Local Run, diffed per region, with the diff images kept as goldens.

1. A capture harness: one place per topic built from the Baseplate template, a fixed camera from a script, Sober and our viewer shot at the same resolution, a per-pixel and per-region (sky, floor, lit faces, shadowed faces) delta report beside CONFORMANCE.md.
2. Lighting: `Technology` (Future, ShadowMap, Voxel), `Ambient`/`OutdoorAmbient`, `Brightness`, `ExposureCompensation`, `EnvironmentDiffuseScale`/`SpecularScale`, `ClockTime` and `GeographicLatitude` sun path.
3. Sky and atmosphere: the default sky, `Sky` skyboxes, sun and moon, `Atmosphere` (density, offset, haze, glare, colour, decay).
4. Shadows: sun shadow softness and bias, point/spot/surface light shadows, `CastShadow`, transparency in shadows.
5. Materials: every `Enum.Material` and `MaterialVariant` at the default and 2022 materials, `Reflectance`, `Transparency` sorting, glass, neon bloom.
6. Post effects: `Bloom`, `ColorCorrection`, `DepthOfField`, `SunRays`, `Blur`, in their measured order.

## Where things are

- `crates/rbx_lua/src/server.rs`, `server/hooks.rs`: the server VM.
- `crates/rbx_lua/src/server/query.rs`: spatial queries (Luau side). `server/meshes.rs`: `EditableMesh` and `CreateMeshPartAsync`.
- `crates/rbx_sim/src/session.rs`: a server and its clients, replication. `ownership.rs`: network ownership. `crates/rbx_lua/src/server/remote.rs`: remotes and their argument rules.
- `crates/rbx_sim/src/sim.rs`: the frame loop.
 `physics.rs`, `physics/part.rs`: Rapier world, shapes, joints, adaptive stepping. `physics/constraints.rs`: constraints and movers. `physics/mesh.rs`: MeshPart collision. `spatial.rs`: the query index. `scripts.rs`: which scripts run. `load.rs`: place files. `conformance.rs`: the suite.
- `crates/rbx_sim/examples/`: `run` (CLI), `conformance` (table), `record_goldens` (real server), `coverage` (COVERAGE.md).
- `crates/rbx_sim/src/humanoid.rs`, `physics/character.rs`: humanoids. `players.rs`: the Players service and the local player. `character.rs`, `character/`: the noob, our Animate and Health scripts. `animation.rs`: Animator and AnimationTrack. `engine.rs`: the engine methods, signals and properties the VM asks for.
- `crates/rbx_studio/src/shell/local_run.rs`: the `local-run` beta: Run, Play (local), Pause, Stop live in the editor. `local_run/avatars.rs`: who plays and what they look like. `workspace_view/play_input.rs`: the keyboard and mouse handed to the game.
- `crates/rbx_sim/src/input.rs`, `crates/rbx_lua/src/server/input.rs`, `server/input.luau`: UserInputService, ContextActionService, BindToRenderStep. `crates/rbx_sim/src/character/PlayerModule.luau`: our default controls and camera.
- `crates/rbx_net`: offline mode, the reachability probe and the HTTP gate.
- `crates/rbx_sim/src/gui.rs`, `gui/layout.rs`: PlayerGui at run time, GUI input, TextBox, StarterGui/GuiService core calls. `tools.rs`: Backpack and tools. `teams.rs`: teams. `sound.rs`, `audio.rs`: Sound playback, decoding and roll-off. `chat.rs`: TextChatService's default instances. `crates/rbx_studio/src/workspace_view/core_gui.rs`: our default health, hotbar, player list with leaderstats, chat window and bubbles, Escape menu and kick card. `crates/rbx_studio/src/shell/local_run/sound.rs`: the run's audio mix.
- `crates/rbx_lua/src/server/tween.luau`, `tween.rs`: TweenService. `server/chat.luau`, `chat.rs`: TextChatService messages.
