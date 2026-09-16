# Benchmarks

Recorded numbers for scene reload and rendering, so a change can be shown to
have helped instead of asserted to have. Produced by `scripts/bench.sh`
(`bench.ps1` on Windows), which builds and runs `crates/rbx_viewer/examples/bench`
— read that example's module doc for why it is a plain harness rather than a
criterion benchmark, and for exactly what each number covers.

Not part of `scripts/check.sh` on purpose: this needs a GPU, takes minutes, and
reports a number rather than a pass or a fail.

## How to reproduce

```sh
RBX_FIXTURES=../rbx-native-fixtures/places ./scripts/bench.sh                # with assets
RBX_FIXTURES=../rbx-native-fixtures/places ./scripts/bench.sh --no-textures  # without
```

Both write `target/bench/bench.json` alongside the table, with every individual
sample, the adapter, the commit and the iteration counts in it. `--help` lists
the knobs. A fixture that is not present is reported as skipped, so the run
still works with only the in-repo `assets/tests/TestPlace.rbxl`.

Release is enforced: the harness refuses to run under `debug_assertions` rather
than print a number nobody should record.

## Machine

| | |
| :--- | :--- |
| Recorded on | 2026-09-16 |
| Commit | `bd4308f` (`dev`), plus the harness itself — the JSON records these runs as `"dirty": true` for that reason, and nothing under measurement differed from `bd4308f` |
| GPU | NVIDIA GeForce RTX 4070, Vulkan, driver 580.173.02 |
| CPU | AMD Ryzen 7 7800X3D (8C/16T), `powersave` governor |
| RAM | 30 GB |
| OS | Linux 7.0.11-76070011-generic |
| Frame size | 1280x720 |
| Iterations | 5 cold loads, 25 reloads, 50 patches, 200 frames per level after 40 warmup |
| Fixtures | `assets/tests/TestPlace.rbxl` (81 instances), `marked.rbxl` (16 742 instances, 2.2 MB) |

Each table below is the median of three consecutive full runs; the rightmost
column is how far those three run medians spread, as a percentage of their own
median. Every timing is a median within its run — never a mean, see the `stats`
module for why.

`call` is wall clock around the `Headless` call alone; it returns with the GPU
work it queued still in flight, so it is a floor, not the cost. `readable` is
from that same instant through to the first frame whose pixels are back in
system memory, which is what someone waiting for the viewport actually waits
for. The two are never added together.

The operations, in the order the harness reports them:

| Operation | What it covers |
| :--- | :--- |
| `cold load` | `Headless::load` to the first drawable frame. Its assets are *not* in this: the load asks for them and returns, and they arrive afterwards. |
| `load complete` | The same load through to the frame after the last asset has landed and been swapped in — the finished picture. |
| `full reload` | The whole scene re-derived from a DOM already in memory, against a place whose assets are all resident. |
| `patch instance` | One `BasePart`'s `CFrame` moved, patched in place. |
| `edit: new mesh` / `new texture` | One `MeshPart` edited to name a `MeshId`/`TextureID` this session has never decoded: what the person typing waits for. Staged with `Headless::forget_asset` and a reload, so the fetch behind it is a disk-cache read rather than a download — see `examples/bench/streaming.rs`. |
| `mesh swapped` / `texture swapped` | The same edit through to the frame that actually shows the asset; `call` is the swap-in rebuild alone. |

## Baseline — `--no-textures`

The tracked number. Reproducible to under 7% run to run, so a regression in it
means something.

| Fixture | Operation | call med | readable med | readable p95 | run-to-run |
| :--- | :--- | ---: | ---: | ---: | ---: |
| TestPlace | cold load | 216.6 ms | 249.7 ms | 254.9 ms | 25.9% |
| TestPlace | full reload | 245.0 ms | 279.5 ms | 287.0 ms | 6.9% |
| TestPlace | patch instance | 0.01 ms | 0.89 ms | 1.04 ms | 29.0% |
| marked | cold load | 526.2 ms | 556.5 ms | 568.6 ms | 6.9% |
| **marked** | **full reload** | **377.4 ms** | **403.7 ms** | **414.8 ms** | **6.6%** |
| marked | patch instance | 0.00 ms | 0.80 ms | 0.98 ms | 5.7% |

Steady-state frame cost, view held still:

| Fixture | Quality | frame med | frame p95 | draw med | readback med |
| :--- | :--- | ---: | ---: | ---: | ---: |
| TestPlace | Level01 | 0.22 ms | 0.44 ms | 0.05 ms | 0.17 ms |
| TestPlace | Level11 | 0.27 ms | 0.49 ms | 0.08 ms | 0.18 ms |
| TestPlace | Level21 | 0.54 ms | 0.87 ms | 0.09 ms | 0.45 ms |
| marked | Level01 | 0.25 ms | 0.55 ms | 0.11 ms | 0.13 ms |
| marked | Level11 | 0.30 ms | 0.53 ms | 0.16 ms | 0.14 ms |
| marked | Level21 | 0.41 ms | 0.66 ms | 0.17 ms | 0.24 ms |

## Baseline — with assets (default)

What the editor actually does. The honest user-facing figure, and the one the
target below is about — but see "Why the tracked number has assets off".

| Fixture | Operation | call med | readable med | readable p95 | run-to-run |
| :--- | :--- | ---: | ---: | ---: | ---: |
| TestPlace | cold load | 540.8 ms | 572.8 ms | 596.4 ms | 12.8% |
| TestPlace | full reload | 538.1 ms | 568.5 ms | 689.6 ms | 29.7% |
| TestPlace | patch instance | 0.01 ms | 1.22 ms | 1.36 ms | 12.5% |
| marked | cold load | 1022.2 ms | 1075.5 ms | 1242.3 ms | 16.4% |
| **marked** | **full reload** | **1042.7 ms** | **1092.0 ms** | **1112.3 ms** | **14.9%** |
| marked | patch instance | 0.00 ms | 0.95 ms | 1.14 ms | 6.8% |

| Fixture | Quality | frame med | frame p95 | draw med | readback med |
| :--- | :--- | ---: | ---: | ---: | ---: |
| TestPlace | Level01 | 0.24 ms | 0.45 ms | 0.07 ms | 0.16 ms |
| TestPlace | Level11 | 0.28 ms | 0.46 ms | 0.10 ms | 0.18 ms |
| TestPlace | Level21 | 1.31 ms | 1.66 ms | 0.12 ms | 1.19 ms |
| marked | Level01 | 0.43 ms | 0.59 ms | 0.30 ms | 0.12 ms |
| marked | Level11 | 0.36 ms | 0.58 ms | 0.24 ms | 0.11 ms |
| marked | Level21 | 0.49 ms | 0.77 ms | 0.26 ms | 0.20 ms |

This reproduces the one-off profile in `CHANGELOG.md` (a `marked.rbxl` reload at
0.9–1.15 s cold), which is the first evidence that the harness measures what
that profile measured.

## 2026-09-16 — assets off the render thread

Same machine and settings, on `perf/async-assets`. Assets are resolved on a
background pool and swapped into the picture as they land, so a cold load's
number splits in two and two new operations exist at all. `--load-iters 5`,
`--reload-iters 25`, `--patch-iters 50`, one full run rather than the median of
three the two baselines above are, so read these to two significant figures,
not to the last digit.

| Fixture | Operation | call med | readable med | readable p95 |
| :--- | :--- | ---: | ---: | ---: |
| TestPlace | cold load | 220.6 ms | 254.8 ms | 261.2 ms |
| TestPlace | load complete | 387.2 ms | 388.2 ms | 403.3 ms |
| TestPlace | full reload | 0.60 ms | 1.61 ms | 1.79 ms |
| TestPlace | patch instance | 0.01 ms | 1.01 ms | 1.08 ms |
| marked | cold load | 433.6 ms | 462.2 ms | 491.1 ms |
| marked | load complete | 977.0 ms | 977.8 ms | 992.5 ms |
| **marked** | **full reload** | **15.5 ms** | **16.8 ms** | **19.7 ms** |
| marked | patch instance | 0.01 ms | 0.89 ms | 1.01 ms |
| **marked** | **edit: new mesh** | **0.04 ms** | **0.90 ms** | **1.00 ms** |
| marked | mesh swapped | 1.28 ms | 3.10 ms | 3.58 ms |
| **marked** | **edit: new texture** | **0.04 ms** | **0.93 ms** | **1.06 ms** |
| marked | texture swapped | 1.39 ms | 3.31 ms | 3.66 ms |

`TestPlace.rbxl` has no `MeshPart` with two distinct `MeshId`s to swap
between, so the two edit phases report as skipped there rather than being
measured against something they are not about.

Against `dd04b1e` (the commit before, measured on the same machine in the same
session, `marked.rbxl`, assets on): cold load 954.4 / 1004.4 ms, full reload
19.6 / 29.0 ms, patch instance 0.00 / 0.91 ms. So the first drawable frame of
a cold load more than halved while the finished picture stayed where it was
(978 ms against 1004 ms), and a reload lost the asset-table walk it no longer
does. There is no `dd04b1e` number for the two edit phases: the harness stages
them with `Headless::forget_asset`, which does not exist there, and the edit
itself answers `Ok(false)` and becomes a full reload with a fetch in front of
it — so ~29 ms plus a decode is the honest comparison, not a measured one.

## Target

**Full reload of `marked.rbxl` under 0.5 s, first frame readable.**

- With assets: **1.09 s** — 2.2x over.
- Without assets: **0.40 s** — met.

The gap between those two is the whole problem, and the three configurations
above decompose it without touching the code:

| Component | Cost | Measured as |
| :--- | ---: | :--- |
| New GPU device and full pipeline rebuild, every reload | ~245 ms | TestPlace reload, no assets — 81 instances, nothing to resolve, so almost none of it is the place |
| Rebuilding a 16 742-instance scene and uploading it | ~132 ms | marked minus TestPlace, no assets |
| Re-resolving assets the previous scene already had | ~665 ms | marked with assets minus marked without |
| GPU work still queued when `reload` returns | ~26–49 ms | `readable` minus `call`, steady to within a few ms everywhere |

`Headless::reload` builds a whole new `Offscreen` — which opens its own wgpu
instance, adapter and device and rebuilds every pipeline — and re-resolves every
texture, material and file mesh from the on-disk cache, for a scene whose assets
are already resident. Those two together are roughly 900 ms of the 1.09 s.

## Why the tracked number has assets off

With assets on, the reload timing is not unimodal. On `marked.rbxl` it settles
into roughly 790–915 ms or roughly 1040–1130 ms and moves between the two within
a single process; a 40-sample run split 16/24 between them. Raising the sample
count from 15 to 25 did not fix it — more samples describe a two-humped
distribution better, they do not make its median stable — and neither did
pinning to physical cores only (`taskset -c 0-7`) or sending the renderer's
progress output to a file instead of `/dev/null`. The run-to-run spread stayed
at 15% on `marked` and 30% on `TestPlace`.

Turning assets off removes the variance entirely (6.6% and 6.9%), which locates
it: it is in the asset-resolution path — a scoped thread pool spawned per call,
re-reading and re-decoding a 271 MB on-disk cache — not in the scene rebuild,
not in the GPU upload, and not in the harness. So the assets-off run is what a
before/after comparison should be made on, and the assets-on run is what the
0.5 s target should be judged on. Record both.

## Caveats

- **"First frame readable" is the first frame, not a fully textured one.**
  `Renderer::draw` keeps a per-frame texture-upload budget and spreads a freshly
  built scene's remainder over the frames after it. `Offscreen::finish_loading`
  ends that early but is crate-private with no route to it through `Headless`,
  so the harness cannot time a completely drawn frame after a reload. The cost
  is visible rather than hidden: on `marked` the first dozen or so frames after
  a load run 6–30x their settled cost, which the patch phase drains (40 frames)
  before measuring so it is not charged for the load.
- **The frame numbers are readback-bound and the GPU is idle-clocked.** At
  1280x720 both fixtures draw in well under a millisecond, so the card never
  leaves its low power state (540 MHz of a 3120 MHz boost, 20 W) and the
  readback dominates the draw. They are a floor. Compare them only like for
  like, at the same size, on the same machine.
- **The CPU governor is `powersave`.** Left as found rather than changed for the
  benchmark, because that is how the machine renders the rest of the time.
- **The asset cache must be warm** (`~/.cache/rbx-native/assets`, 271 MB here)
  or an assets-on run measures the network instead. It was warm for all of the
  above; no run downloaded anything.
- **`patch instance`'s `call` column is sub-10 µs** and is at the resolution
  floor of what is worth reporting. The `readable` column is the real number,
  and it is essentially one frame: patching a single instance costs a redraw.
