# Debugging scripts during Play

Studio stops a playtest's scripts at breakpoints: Watch, Call Stack, the
stepping buttons, and per-breakpoint **Trigger At** contexts (Client,
Server, Edit). This is how rbx-native does it against a real Roblox server
and client it cannot attach to.

## Constraints (Roblox docs, 2026-10-14)

- The in-game `debug` library has `info`, `traceback` and profiling calls.
  It has no `getlocal` and no hooks (`reference/engine/libraries/debug.yaml`).
  So nothing outside a script can read its locals or stop it between
  lines; the script itself has to cooperate.
- `loadstring` exists on the server only, behind
  `ServerScriptService.LoadStringEnabled` (which the probe turns on). It
  doesn't exist on the client.
- Breakpoint contexts: "Client (client‑side scripts), Server (server‑side
  scripts), or Edit (debugging plugin scripts)" (`studio/debugging.md`,
  "Trigger At").
- Studio pauses the whole session at a breakpoint. A game can't do that
  for itself.

## Design

**Instrumentation at publish** (`rbx_play::instrument`). Each script with
breakpoints is rewritten in the published copy only. The first statement
on each line gets, on that same line,

```lua
if __rbxdbg_B[N] or __rbxdbg_D.s then __rbxdbg_H(ID, N, "a b", {a, b}, COND, LOG, CONT) end
```

- Line numbers stay exact, so error messages, `debug.info` and the
  breakpoints themselves line up with the editor.
- The names and values are the locals in scope at that statement. They
  come from a full_moon scope walk covering blocks, function parameters,
  `self`, loop variables and closures nested in expressions, capped at the
  60 innermost names. The value table is only built when the line
  actually stops.
- `COND` and `LOG` are closures compiled from the breakpoint's condition
  and log message, so they see the same locals with real Luau semantics.
- A prefix on the first statement's line (after any `--!` directives)
  requires the probe's `__rbxnative_debug` module. It passes each side's
  breakpoints that are active from the start, so code that runs at
  startup stops before the editor's first message arrives.
- Each hooked script is compile-checked with Luau (`rbx_lua::compile_check`).
  A script that fails, e.g. with too many upvalues, is published as it was,
  with a warning in Output.

**The probe's `debug` module** (`probe/debug.luau`, used on both sides):

- **Stopping.** A hook stops when its line is active (`B[N]`) and its
  condition holds, or when the thread is stepping and the step fires:
  - Step Into: the next hooked line at any depth;
  - Step Over: the next hooked line at the same or a shallower depth;
  - Step Out: the next hooked line at a shallower depth.

  Depth is the `debug.info` level count.
- **Pausing.** The probe sends `Paused` with the frames (`debug.info(level,
  "sln")`, its own frames left out) and the formatted locals. It then waits
  with `task.wait()` for `DebugResume`. Only that thread waits: other
  scripts, physics and rendering go on. A thread that can't yield (inside
  a metamethod, say) warns and runs on.
- **Watches** (`DebugWatch`):
  - server: any Luau through `loadstring`, with `setfenv` over the locals
    and the script globals;
  - client: plain paths (`a.b[1]`).
- **Relay.** Client messages travel the existing RemoteEvent relay, ahead of
  tree chunks. Commands for the client wait in the server's `forLater` until
  the tester joins.

**Editor** (`shell/play/debug.rs`, `shell/debugging.rs`):

- The debugger's run has two backends, Edit (the local mlua session) and
  Play. They share the Watch and Call Stack docks, the gutter, F5/F10/F11
  and Shift+F5.
- A Play run lasts as long as the test.
- Pauses from several threads queue; Continue shows the next one.
- Selecting an outer frame moves the caret there. Only the stopped frame
  has variables.
- Stop (Shift+F5) stops the test, as Studio's does. Temporary breakpoints
  go when the test ends.
- Every breakpoint change ends in `sync_gutters`, which sends each side's
  active lines (`DebugSet`) when they changed. Lines are resolved to the
  next hooked line and filtered by Trigger At.
- Changing a condition or log message mid-test, or adding breakpoints to a
  script that had none, takes effect at the next Play; Output says so.

## Verified live (2026-10-14, Xephyr + Sober, sandbox place)

Test place: a server `Counter` with a loop calling a local `add`, and a
client `Hud` in StarterPlayerScripts.

- Breakpoints on `Counter` line 7 and `Hud` line 4, then Play. The server
  stopped at line 7 (`total = 0`, `add = function`, `i = 1`). The client's
  pause queued behind it and showed after the step (`me = Player
  Players.<tester>`).
- Step Over went from 7 to 8 without entering `add`. Continue came back to
  7 on the next iteration, Step Into went to line 3 of `add` (`n = 2`), and
  Step Out returned to line 8 (`total = 3`).
- A watch `total` read 3 from the paused server thread. A malformed
  expression came back as the server's Luau compile error.
- Disabling line 7 in the gutter mid-test let the loop run (ticks printed,
  no pause). Re-enabling it stopped the loop again within one iteration.
- A breakpoint inside `add`: the Call Stack showed `add` line 3 over
  `Counter` line 7. Selecting `Counter` moved the caret to line 7, and
  Variables said only the stopped frame has variables.
- Shift+F5 stopped the test; the place and breakpoints came back unchanged.

## Not done

- Pausing the whole session (physics, other scripts) as Studio does.
- Variables of outer frames. The hook only captures the frame it runs in.
- Live edits of conditions and log messages (they are compiled in), and
  breakpoints in scripts that had none when Play started.
- Client watch expressions beyond plain paths (no `loadstring` there).
