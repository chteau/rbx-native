# Local Run

Local Run is our own Roblox runtime. You hit Run and the place's scripts and physics go, on your machine, with no Roblox client, no Sober, no network. It's clean room: we write every line ourselves, from the docs and from watching the real client.

This file is the plan, the rules, the sources, and where we're at. Milestone 1 (phase 0 plus the core of phase 1) is done; what it didn't finish is listed at the end as Milestone 2.

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
| [Rapier](https://rapier.rs) `rapier3d` 0.36 | Apache-2.0 | Rigid bodies, contacts, joints |
| [parry3d](https://parry.rs) 0.31 | Apache-2.0 | Collision shapes (through Rapier) |
| glam / glamx | MIT OR Apache-2.0 | Math (through Rapier) |
| [rbx-dom](https://github.com/rojo-rbx/rbx-dom) (our `rbx_dom`, `rbx_binary`, `rbx_xml`, `rbx_reflection`) | MIT | Loading .rbxl/.rbxlx, the instance tree, reflection |
| [Lune](https://github.com/lune-org/lune) | MPL-2.0 | Prior art only. We looked at how it shapes a Roblox-like runtime; no code taken, not a dependency |
| Real Roblox server, observed via `rbx_play` | Observation | The conformance goldens (`crates/rbx_sim/conformance/goldens/`) |
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

- `crates/rbx_lua` already had the engine the Command Bar and the plugin host run on (instances, signals, the task scheduler, require, services). Local Run adds `rbx_lua::server`: a VM with a server host (sim clock, frame-based waits, newest-first signals), server hooks (RunService, Touched/TouchEnded, PhysicsService, BasePart mass and impulses, HttpService JSON, CollectionService, WaitForChild) and a tiny `Physics` trait the sim implements. The changes to the shared engine are opt-in host flags (`frame_clock`, `newest_first`) and one hook (`after_write`), so the Command Bar and plugin host behave as before, and their tests pass.
- `crates/rbx_sim` owns the loop. It reads the DOM change log after every bit of Lua and routes it: a structural change (size, anchored, a new weld) rebuilds the physics world before the next step, a CFrame write teleports a body, a velocity write sets it, a new Script in a server container starts.
- Physics writes go back to the DOM without logging changes, so moving parts don't fire `Changed` for every frame. That matches Roblox: physics doesn't fire `Changed` for CFrame either.

### Which scripts run

`Script` only (not LocalScript), not disabled. `RunContext.Legacy` runs under Workspace and ServerScriptService. `RunContext.Server` also runs under ReplicatedStorage. ModuleScripts run when required. That's straight from the RunContext docs.

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
- Adaptive stepping: the same docs say easy assemblies go on a 60 Hz island. Our FreeFall golden matches 60 Hz semi-implicit Euler, not 240 Hz. So in rbx_sim an assembly touching nothing gets a whole frame of gravity up front and coasts the substeps; anything in contact stays at 240 Hz. `PhysicsSteppingMethod = Fixed` turns that off.
- `task.wait(0)` resumes on the next frame, never the same one. `task.wait(t)` resumes on the first frame at or after `t`. Legacy `wait()` never takes less than 0.03 s, which is two frames (golden: 2 heartbeats).

### Signals

- Immediate mode (the place default as far as a game script can tell): `Fire` runs handlers before it returns, and a property write runs `Changed` / `GetPropertyChangedSignal` before the next line. Golden: `handler after_fire changed after_set`.
- Handlers run newest connection first, and a `:Wait()` counts as a connection. Golden: connect A, B, then Wait W, then C, fire: `C W B A`.
- `Workspace.SignalBehavior` is `NotScriptable`: reading it errors on a real server, so it does in ours. A place file can still say Deferred, and then we defer.

### Physics

| Roblox | rbx_sim | Notes |
|---|---|---|
| Block | cuboid | exact |
| Ball | ball, diameter = smallest side | exact for a cube part |
| Cylinder | cylinder along X | exact |
| Wedge | convex hull, vertical face at +Z | exact |
| CornerWedge | convex hull, apex above +X -Z | vertex layout read off the docs, not yet checked against Studio |
| Capsule (Shape) | capsule along X | the docs don't give the axis, X like the cylinder is a guess |
| MeshPart, Union, Terrain | not simulated | next milestone (hull from mesh data, terrain voxels from `rbx_terrain`) |
| Anchored | fixed body | |
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

The suite is eight Luau tests in `crates/rbx_sim/conformance/tests/`. The same modules run in a real Roblox server and in rbx_sim and print one JSON line each.

Recording goldens (`cargo run -p rbx_sim --example record_goldens -- <universe> <place> <base.rbxl> <cloudflared>`) adds the suite to a base place, publishes it to the sandbox place, restarts its servers, joins with Sober on Xephyr, collects the server's prints through the `rbx_play` probe, and writes `goldens/*.json`. It runs the suite five times in one server. The goldens keep the median and the spread across passes, and rbx_sim also runs five passes and takes the median, so we compare like with like. Pass 1 is always a bit off on a fresh server (it starts at a different point in the frame); passes 2 to 5 were identical.

`cargo test -p rbx_sim` checks every recorded golden. A golden can carry a `known_gap` explaining why it's off; that's reported, not failed. `cargo run -p rbx_sim --example conformance -- --write docs/research/local-run/CONFORMANCE.md` writes the table.

Current state, recorded on version 535 of the sandbox place:

| Test | Roblox | rbx_sim | Result |
|---|---|---|---|
| FreeFall | y after 15/30/60 frames -6.52 / -25.417 / -100.219 | -6.514 / -25.317 / -99.709 | ok, within 0.51 studs |
| FreeFall | vy at 30/60 frames -99.735 / -197.835 | -98.1 / -196.2 | ok on spread; passes 2 to 5 are a steady 1.6 stud/s (half a frame of g) ahead of ours |
| RestingContact | sink -0.0062 (placed) / -0.0101 (dropped), speed 0 | -0.001 / -0.001, speed 0 | ok |
| FrictionSlide | distance 3.234, speed after 15 frames 5.286, stops at frame 21 | 3.387, 5.285, 21 | ok |
| ElasticBounce | first bounce frame 27, peaks 4.867 / 1.317 | 27, 5.051 / 0.984 | ok |
| TouchedOrder | sensor in @14, out @18, floor in @21, out @22, then floor @42, @53, @59... | identical through @22, then the bounce differs | known gap, see below |
| TaskTiming | task.wait(0) 1 frame, (0.1) 6 frames, (0.5) 31 frames, wait() 2 frames, delay(0.25) 15 frames | 1, 6, 30, 2, 15 | ok (the 0.5 wait is one frame short, inside tolerance) |
| RunServiceOrder | see "The frame" | identical | ok |
| SignalBehavior | immediate Fire and Changed, newest first, property unreadable | identical | ok |

Known gaps:

- **Bounce spin.** Rapier does restitution once per contact at the end of the step. A box landing flat on four corners gets about 6 rad/s of spin from that, so its first bounce goes sideways and back through the sensor. Roblox's box bounces straight. Fixing it needs our own restitution pass. Solver iteration counts, warmstarting and friction don't change it, we tried.
- **The half frame in FreeFall velocity.** Roblox's vy runs half a frame of gravity ahead of position from frame 30 on. We match the position, not the velocity. We need a dedicated probe (sample on Stepped vs Heartbeat) before guessing.
- **task.wait(0.5)** takes 31 frames on Roblox and 30 for us. Probably the server rounds its resume time up a frame for longer waits. Needs more samples (1.0, 2.0) before we change the rule.

## Coverage

`docs/research/local-run/COVERAGE.md` is generated (`cargo run -p rbx_sim --example coverage -- docs/research/local-run/COVERAGE.md`): a server script in a fresh Local Run makes an instance of every class and indexes every member a game script may touch. Today: 87 of 1385 functions, 31 of 355 events, 1926 of 2510 properties. The property number flatters us: any property the reflection database knows reads back as data, whether or not we act on it. The physics table above is what's really simulated.

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
- `rbx_animation` (phase 3): Animator and AnimationTracks driving Motor6D transforms.
- `rbx_play`: the probe and the tunnel that record goldens.

## Phases

Every phase ends with acceptance tests. "Golden" means recorded from a real server and checked by `cargo test -p rbx_sim`.

### Phase 0: foundations (done)

Headless sim crate, server VM, conformance harness with real goldens, coverage generator, beta switch.

Acceptance:
- `cargo run -p rbx_sim --example run -- place.rbxl --seconds 5` loads a place, runs its server scripts and physics, prints output and where the unanchored parts ended up. Done.
- `cargo test -p rbx_lua` and `-p rbx_sim` pass, Command Bar and plugin host tests included. Done.
- Goldens recorded from a real server for all eight tests. Done.

### Phase 1: server scripts and rigid physics (core done)

Acceptance:
- RunService frame order matches the golden. Done.
- task.wait / wait / delay / defer / spawn timings match the golden within one frame. Done.
- Free fall, resting contact, friction slide and bounce within tolerance. Done.
- Touched/TouchEnded order matches the golden all the way through. Open (bounce spin).
- Raycast, Blockcast, Spherecast, Shapecast, GetPartBoundsInBox/Radius, GetPartsInPart, OverlapParams: results match a golden scene. Open.
- Constraints (HingeConstraint, BallSocket, Rope, Spring, Prismatic, AlignPosition, AlignOrientation, LinearVelocity, VectorForce): a golden per constraint. Open.
- MeshPart and Union collision (hull from mesh data, CollisionFidelity). Open.

### Phase 2: in Studio

Acceptance:
- Run (local) in the ribbon runs the open place live in the viewport through `rbx_viewer`, with Stop restoring the edit DOM untouched (golden: a place saved before and after Run is byte-identical).
- Output, Explorer and Properties show the running DOM while it runs, like Studio's Run.
- Pause, step one frame, resume.

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

## Milestone 2 (next)

What Milestone 1 didn't get to, in the order we'd do it:

1. Our own restitution pass (or a contact solver closer to Roblox's) so TouchedOrder matches past the first bounce.
2. The FreeFall velocity half frame and the long `task.wait` rounding: add probes, record, then fix.
3. Spatial queries: Raycast and the rest of WorldRoot, with goldens.
4. The common constraints, one golden each.
5. MeshPart/Union hulls and CollisionFidelity.
6. Phase 2 start: Run (local) live in the viewport with Stop restoring the edit DOM, replacing today's 5-second headless run that only prints to the Output.
7. More goldens: sleep and wake, stacked boxes settling, a spinning part's angular damping, `task.wait` of 1 s and 2 s.

## Where things are

- `crates/rbx_lua/src/server.rs`, `server/hooks.rs`: the server VM.
- `crates/rbx_sim/src/sim.rs`: the frame loop. `physics.rs`, `physics/part.rs`: Rapier world, shapes, joints. `scripts.rs`: which scripts run. `load.rs`: place files. `conformance.rs`: the suite.
- `crates/rbx_sim/examples/`: `run` (CLI), `conformance` (table), `record_goldens` (real server), `coverage` (COVERAGE.md).
- `crates/rbx_studio/src/shell/local_run.rs`: the `local-run` beta, Model › Run (local).
