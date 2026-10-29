# Animation Graph behaviour measured in real Studio

Measured 2026-10-28 in Roblox Studio 0.745 (Vinegar, Linux) for rbx-native.
This covers what `docs/research/animation.md` §6 could only infer from the
docs. Over, Add and Subtract were already held to Studio by
`crates/rbx_animation/tests/studio_graph.rs` (to 1e-3, Cartesian Blend2D
on a square too).

## How

`crates/rbx_animation/tests/studio_graph_probe.rs` (an ignored test,
`RBX_GRAPH_OUT=<dir>`) writes `probe.rbxl` and `probe.lua`. The place holds
a rig with four Motor6Ds `P0..P3/J` and local KeyframeSequences:

- `T0..T11`, each moving one joint by 1 stud along one axis, so a blend's
  joint positions are its input weights;
- `R0..R4`, half-second ramps at x = 1..5 with y rising 0 → 1;
- one `AnimationGraphDefinition` per question, written by our own graph
  writer.

The Lua runs from the command bar. It registers every clip and graph with
`KeyframeSequenceProvider:RegisterKeyframeSequence` (local only, nothing
uploaded), plays each graph on the Animator, and steps it with
`Animator:StepAnimations(1/30)`. Each sample is printed as `GP|case|i|...`,
which ends up in Studio's log (`appdata/Roblox/logs/*_Studio_*.log`). The
whole run was done twice and gave identical output.

## Results

**Blend2D.** The weights are the gradient-band scheme already
implemented, with one change. Each input's weight is the minimum over the
other inputs of `1 − (p·v)/|v|²`, clamped at 0 but **not at 1**, then
normalised. In Polar mode the pair vectors are `((|b|−|a|)/mean, Δangle·2)`
(angles wrapped to (−π, π]; a point at the origin takes its partner's
angle). The ×2 is measured: on the irregular layout `(0,0) (0,1) (1.2,2)
(2.5,0.7) (−1,1.5)`, α = 2 fits to 5e-6 and α = 1 is off by up to 0.16.
The clamp is measured too: inputs at angles 0, 1, 2.5 with the target at
4.5 rad give 0.48766 / 0 / 0.51234 in Studio. Clamping at 1 would give
0.5 / 0 / 0.5. In all, 57 targets over six layouts match to 1e-5. Samples:

| layout | target | Studio weights |
|---|---|---|
| centre + compass at 1 | (π/4, 0.5) | 1/3, 1/3, 1/3, 0, 0 |
| centre + compass at 1 | (0.3, 0.2) | 0.67171, 0.16793, 0.16036, 0, 0 |
| compass at 1 and 2 | (π/4, 0) | 0.25 on each inner input |
| irregular (above) | (1, 0.4) | 0.47039, 0.29563, 0.21952, 0.01446, 0 |
| Cartesian, 5 irregular | (2, 2) | 0, 0.12752, 0.87248, 0, 0 |

**RandomSequence.** We recorded 40 picks for each of Seeds 0, 1, 2, 3,
7, 42, 1000 and 123456, over 2, 3 and 5 inputs, plus 1:2:3 weights at
Seed 7. For one Seed, every input count and weighting is consistent with a
single shared sequence `u₀, u₁, …` in [0, 1), where each pick is the
input whose share of the summed weights holds `u`. A brute-force search
over generators found the sequence: `std::mt19937(seed)`, `u = x / 2³²`.
It matches all 320 picks. Other generators scored no better than chance:
Luau's `Random` (PCG32), PCG32 with other seedings, MSVC `rand`, minstd,
splitmix64, and the seed's float bits. Further facts:

- Seed 7.5 picks as 8 and −2 as 4294967294.
- −1 differs between runs.
- Renaming the node changes nothing.
- The same input can play twice in a row.
- The first pick is made when the track starts. Each later pick comes on
  the step where the previous input finishes.

Seed 7, 3 inputs: `0020122120101001001122201102022201122002`.

**Transitions.** CubicV2 over 1 s gives 0.00015, 0.00119, 0.004, 0.00948,
… 0.5 at 1/30 s steps, which is `4t³` up to the midpoint (cubic in-out).
Linear is linear. Interrupting an A→B blend at 0.5 with C makes A drop out
at once, and B fades into C over the full duration. There is no frozen
snapshot.

**Select / Priority Select.** With a Selection naming no pin, the empty
default included, nothing plays: the joints sit at rest, and Studio logs
"Selection 'Nope' is not an input pin name". Changing an active Select to
such a name fades the current input out to rest over the default
transition. A Priority Select whose Triggers are all off plays nothing.

**Mask.** With a mask listing only `P0` at 0.5, P0 plays at 0.5, and P1,
which the mask does not list, plays at 0.

**Trim.** TrimStart 0.2 / TrimEnd 0 holds the pose at 0 s. TrimStart 0.3
/ TrimEnd 0.1 holds it at 0.1 s. A backwards trim freezes at TrimEnd.

**One Shot.** Re-triggering a shot while it plays restarts it from 0.

## Instance attributes in files

We loaded the Studio-saved `tests/fixtures/studio_state_machine.rbxm` into
an XML place, using `game:GetObjects("rbxasset://…")` from a copy in
Studio's content folder (deleted afterwards). Ctrl+S then saved the place
locally. `GetAttribute("EntryState")` returns the `State` ObjectValue.

- In a binary file the attribute sits in `AttributesSerialize` as type
  `0x25` + `u32` referent, as before.
- In XML, Studio leaves it out of the blob
  (`AttributesSerialize` = `AAAAAA==`, zero entries). It writes it as
  `<Ref name="__attrRef_EntryState">RBX…</Ref>` straight after the blob.

The graph subtree of that save is
`tests/fixtures/studio_state_machine.rbxmx`. `rbx_xml` now folds
`__attrRef_*` into the blob on reading and splits it back out on writing.

## Second pass: triggers, events, state machines, phase sync

Measured 2026-10-28/29 in the same Studio, with the same harness. The
graphs are in `crates/rbx_animation/tests/studio_graph_probe/unknowns.rs`.
Two clip sets were added: `MA`/`MB` hold P0/P1 at x = 1 for a one-second
loop, with `KeyframeMarker`s `Ev` "A0".."A3" at 0, 0.25, 0.5 and 0.75 s
and "B0".."B3" at 0.1, 0.35, 0.6 and 0.85 s. `L1`/`L2` loop P0/P1 from
x = 0 to 1 over one and two seconds, so a joint's x is its clip's phase.
Each sample now also prints the markers reached, as `value@step`, and every
parameter's `GetParameter` value.

Four things about the harness:

- Marker handlers are deferred, and `Workspace.SignalBehavior` cannot be
  read or set from the command bar. The event cases call `task.wait()`
  after each step so the handlers run before the next one. A frame with
  no step changes nothing in the graph; sample 4 of the trigger case
  checks this.
- A state machine with no `EntryState` plays nothing.
- Studio resolves an `Instance` attribute (type `0x25`) while it reads
  the holder's PROP chunk, against the instances whose INST chunks it has
  read so far. A target whose INST chunk comes later stays an unresolved
  `InstanceHandle` (`:Get()` returns nil, and Studio saves it back as
  referent `0xFFFFFFFF`), and the state machine plays nothing. Studio
  writes every INST chunk before the first PROP. `rbx_binary` used to
  write each class's PROPs right after its INST, and `AnimationNodeDefinition`
  is seen before `ObjectValue`, so every `EntryState` it wrote was lost.
  The referent numbers, the blob bytes and the PRNT order were not the
  cause: a file written with Studio's postorder PRNT still failed, and the
  same file with INST chunks first resolved all 32 state machines
  (measured 2026-10-29, Studio 0.745). `GetAttribute` returns an
  `InstanceHandle` for any `Instance` attribute, set from Lua or not;
  `:Get()` gives the instance.
- With no `EntryState`, an "(Any)" transition whose condition holds does
  not start a state either (`NoEntry`, three steps then `Go` on).

The whole run was done twice and gave identical output.

**Triggers.** A trigger state machine cycles T0 → T1 → T2 → T0, with one
hop per evaluation that sees `Go`. A trigger is seen by exactly one
evaluation, whether that is a `StepAnimations(1/30)` or a
`StepAnimations(0)`. `GetParameter` still reads true after that step, and
the next step clears it before it evaluates. If the trigger is fired again
while it still reads true, the second fire is lost. In the samples below,
"Go after" is what `GetParameter` returned:

| sample | action | state | Go after |
|---|---|---|---|
| 1 | fire, no step | T0 | true |
| 2 | 1 step | T1 | true |
| 3 | 1 step | T1 | false |
| 7 | fire, `StepAnimations(0)` | T0 | true |
| 8 | 1 step | T0 | false |
| 10 | fire, 2 steps | T1 | false |
| 11 | fire, 1 step | T2 | true |
| 12 | fire again, 1 step | T2 | false |

A Bool held on takes one hop per step. Before any `SetParameter`,
`GetParameter` returned each parameter's default (`false`, `""`, `0`,
and `2` for `NoEntry`'s `N`). The docs say it returns nil. `Player::get`
returns the default too.

**Transitions take effect within the step.** Setting the condition and
stepping once shows the new state, already blended. On a state
machine's 1 s switch from MA to MB, the first step shows P0 0.96667 and
P1 0.03333, the same as a Select's switch. `Player` used to take a
state-machine transition after the step that saw it.

**Events.** Studio fires both sides' markers during a 1 s transition, for
its whole length and however small a side's share. MA → MB, switched at
step 31, with Select and with a state machine:
`A0@1 A1@8 A2@16 A3@23 | A0@31 B0@34 A1@38 B1@41 A2@46 B2@49 A3@53 B3@56`.
A marker is taken where a step starts, not where it ends. The marker at
0.5 s fires on step 16 rather than step 15, which lands on it. A looping
clip's marker at 0 fires on the step after the wrap. A Blend1D at an
exact tie (0.5) passes the first input's events (MA's).

Once the blend ended, MB's next markers came one step earlier than a
steady clock gives: `B0@63 B1@70`, where 64 and 71 were expected. It
looks as if the incoming clip gains one extra step when the transition
completes. Nothing else we measured can confirm it, because every clip
that was blended into is either constant or had already reached its end.
The `Handover` and `EvHandover` cases in the probe (T1 → L1 and MA → MB
over 0.5 s, read every step) would pin it down. They are written but
have not been run yet: the Studio lock was held for hours by other work.
Until they run, our incoming clip keeps a steady clock (UNVERIFIED in
`nodes/pick.rs`).

**State machines.** In each case T0 plays, then both transitions out of it
become ready in the same step:

| case | transitions, in list order (priority) | Studio took |
|---|---|---|
| TieBC | T0→T1 (1), T0→T2 (1) | T1 |
| TieCB | T0→T2 (1), T0→T1 (1) | T2 |
| TieAny | (Any)→T2 (1), T0→T1 (1) | T1 |
| PrioUp | T0→T1 (1), T0→T2 (2) | T2 |
| PrioDown | T0→T1 (2), T0→T2 (1) | T1 |
| PrioAny | (Any)→T2 (2), T0→T1 (1) | T2 |

There is no randomness: eight runs of each tie, each with fresh GUIDs,
came out alike. The higher Priority wins. On a tie, the state's own
transition beats "(Any)", then the first listed wins. An "(Any)"
transition to the state already playing does nothing: with R0 at 0.2 s,
`Go` on, off and held left it running (y 0.46667, 0.53333, … 1.0). From
T1 the same transition restarts R0 (y 0.06667 after a step), and holding
`Go` on afterwards does not restart it again. With a 0.5 s length, the
blend from T1 reads x 0.06667 and y 0.93778 after one step.

**Priority Select.** Every input is OnceAndHold. The default transition is
0.3 s for T0/T1 and 0 for the R0/R1 ramps.

- When every Trigger turns off, the node fades to rest over the default
  transition: x 1 → 0.88889 → … → 0.11111 → 0.
- From rest, a newly triggered input cuts in with no fade. A Select
  switching from nothing to a pin also cuts in, with x 1 after one step
  despite its 0.3 s transition.
- The current input's Interruptible setting holds back only a move to a
  higher input. When R1 (below, Finished) was playing and A turned on,
  the node stayed on R1 until it finished: y 0.2 … 0.99997, then R0 at
  0.06667. It stayed even when B was turned off in the same step.
- When the current input's own Trigger turns off, a move down or to
  rest happens at once, whether it is set to Finished or to Trigger.

**Mask naming no mask.** A Mask whose `Mask` is empty or names no shared
mask passes everything through at full weight (P0 = P1 = 1 from `Both`).
`Invert` changes nothing. We used to zero every joint under Invert.

**Phase sync.** Blend1D over L1 (at 0) and L2 (at 1):

- Synced, at 0.5 / 0.5: both clips advance 1/1.5 of a cycle per second.
  After one step, P0 = P1 = 0.01111, which is half of phase 0.02222.
- Synced, at 0.75 / 0.25: both advance 1/1.25 of a cycle per second.
  So the virtual duration is the weighted arithmetic mean `Σ wᵢ·Dᵢ`.
- Synced, at weight 0: L2 kept L1's phase, and at 0.5 again both read
  0.33889.
- Unsynced: each clip runs at its own rate, and a weightless one keeps
  running. L2 read phase 0.05 at sample 22, after 63 steps (63/60
  wrapped). If it had paused, it would read 0.633. Before this, we
  paused weightless Unsynced inputs.

## Still not measured

- Whether the incoming clip gains a step as a transition completes (see
  Events; the `Handover` cases are ready to run).
- Blend2D phase sync was not probed. It is taken to follow the same rule
  as Blend1D.
