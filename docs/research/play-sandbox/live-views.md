# Live Client/Server views during Play

Studio, while a test runs, shows the running game in the Explorer and
Properties: a **Client** view (the tester's own client DataModel) and a
**Server** view, toggled from the Test tab. Editing a property there changes
it in the game at once. This is how rbx-native does the same without Studio,
through the probe and the tunnel (`README.md`).

## Constraints (Roblox docs, 2026-10-14)

- `HttpService`: 500 requests/minute per server for non-Open-Cloud hosts;
  ports 80/443 only (`cloud-services/http-service.md`, "Limitations").
  So everything is batched: one ingest POST every 0.25 s at most, carrying
  an array of messages (≤ 240/min), plus the long-poll.
- `RemoteEvent:FireServer`: ~500 calls/s per client, shared by all remotes
  (`RemoteEvent.md`, "Throttling"). Tables cross by copy; don't mix array
  and dictionary keys; non-replicated Instances arrive as `nil`
  (`scripting/events/remote.md`). The client → server relay therefore
  sends plain tables of strings/numbers, ≤ 20 calls/s.
- `Changed` / `GetPropertyChangedSignal` do **not** fire for physics-driven
  changes (`CFrame`, `Position`, `Orientation`, assembly velocities) and may
  skip properties that change very often (`Object.md`, "Limitations"). So
  property values are **polled** for the few watched instances (every
  0.25 s, sending only what changed) instead of trusting signals.
- `ReflectionService` exists for game scripts, but the editor already has
  the reflection database, so the editor tells the probe which properties
  to read (`Watch`), and decodes values with their declared types.
- `GetDebugId` is Plugin security: ids are the probe's own (a weak-keyed
  table), separate per side.
- Script `Source` is not readable from game scripts; a pcall'd read just
  leaves it out.

## Message flow

```
editor ⇄ (tunnel, HTTPS) ⇄ server probe ⇄ (RemoteEvent) ⇄ client mirror
```

Messages are `rbx_play::protocol` (`Uplink::Tree/Diff/Props`,
`CommandBody::Watch/Set/SetAttribute/Destroy/Reparent/Create/Resync`), each
tagged with `side`. The server probe handles `side = "server"` itself and
relays `side = "client"` commands to the tester's client
(`remote:FireClient(player, command)`, where the player is the first one
that joined); the client mirror's messages come back over the same remote
and the server forwards them with `side = "client"`.

## The probe's Luau

Four scripts, injected by `rbx_play::inject`:

| Name | Class | Parent | Role |
|---|---|---|---|
| `__rbxnative_codec` | ModuleScript | ReplicatedStorage | value ⇄ JSON (`rbx_play::value`'s table) |
| `__rbxnative_live` | ModuleScript | ReplicatedStorage | tree ids, snapshot, diffs, watch, edits — the same code on both sides |
| `__rbxnative_probe` | Script | ServerScriptService | HTTP, logs, commands, client relay |
| `__rbxnative_mirror` | LocalScript | StarterPlayerScripts | the client side of `live` over the remote |

### `live` module (`Live.new(side, send)`)

`send(message)` queues one protocol message (a Luau table with `type`,
`side`, …). The module never yields in signal handlers.

- **Tracked roots**: `game:GetService(name)` for the Studio-visible
  services: Workspace, Players, Lighting, MaterialService, ReplicatedFirst,
  ReplicatedStorage, ServerScriptService, ServerStorage, StarterGui,
  StarterPack, StarterPlayer, Teams, SoundService, Chat, TextChatService,
  LocalizationService, TestService (each in a pcall; missing ones skipped).
  A tracked root is a node with `parent = 0`.
- **Excluded**: any instance whose name starts with `__rbxnative_`, and all
  its descendants.
- **Ids**: `idOf[instance]` (weak keys) / `byId[id]` (weak values),
  counting from 1, never reused.
- **Snapshot** (`snapshot()`, at start and on `Resync`): breadth-first from
  the roots, parents before children, in `Tree` chunks of 1000 nodes
  (`reset = true` on the first, `done = true` on the last), yielding
  (`task.wait()`) between chunks.
- **Diffs**: per root `DescendantAdded` / `DescendantRemoving`, plus per
  tracked instance a `Name` and `Parent` `GetPropertyChangedSignal`
  (connections kept in a weak table, disconnected on removal; capped at
  50 000 instances, beyond which renames are not tracked). Changes are
  coalesced and flushed every 0.25 s as one `Diff` per side:
  - `added`: instances without an id that are now inside a tracked root
    (with their `parent` id), parents before children;
  - `removed`: instances that left every tracked root (checked at flush
    time, so a reparent between roots is a move, not remove + add);
  - `renamed` / `moved` for known ids.
- **Watch** (`watch(items)`): replaces the watch set. Every 0.25 s for each
  watched instance: read each property in a pcall, encode it with the codec,
  compare with the last encoded value (as a JSON string), and send `Props`
  with what changed; the first read sends all. `GetAttributes()` and
  `CollectionService:GetTags()` the same way (sent as `attributes` / `tags`
  when changed).
- **Edits** (each in a pcall; errors reported as an `Ack`-like log line):
  `set(target, prop, value)` decodes with the codec (the current value of
  the property guides enum and instance decoding) and assigns;
  `set_attribute`, `destroy`, `reparent(target, parent)` (`0` = nil),
  `create(class, name, parent)`.

### Server probe additions

- One outgoing queue of messages; flushed every 0.25 s as one
  `POST /v1/ingest` (split above ~400 KB of JSON), so logs, diffs and props
  share the 500/min budget.
- Commands from `/v1/poll`: `watch/set/set_attribute/destroy/reparent/
  create/resync` with `side = "server"` go to the server `Live`; with
  `side = "client"` they are forwarded to the tester's client.
- The remote carries `{kind = "msg", message = …}` from the client (relayed
  into the queue with `side = "client"`; only from the tester) and
  `{kind = "cmd", command = …}` to it. Existing client-log batches keep
  working.

### Client mirror additions

- `Live.new("client", send)` where `send` batches messages and fires
  `remote:FireServer({kind = "msg", message = m})` — at most 20 fires per
  second, a tree chunk of 300 nodes per fire.
- `remote.OnClientEvent` → `{kind = "cmd"}` → the same `Live` methods.

## Editor side

`rbx_play::live::LiveTree` per side; `shell::play::live` swaps the shown
side's tree into `Shell::dom` (Explorer and Properties read it as they read
any place), stashes the edited place's history and selection, forwards
edits made while live as commands, and keeps a `Watch` on the selection.
The 3D viewport stays on the edited place under the embedded game.
Auto-Recovery, the luau-lsp mirror and script tabs leave the live tree
alone; Save writes the stashed edited place.

## Popups over the embedded game

The game window is a native X child window, which the X server draws over
everything GPUI paints. Each floating layer (gpui-base popups, tooltips,
dialogs, and the editor's own `deferred` popups) records its painted bounds
(`gpui_base::occlusion`), and Play cuts those rects out of the game window
with the X Shape extension after every frame, so menus, dropdowns and
dialogs show above the game.

## Verified live (2026-10-14, Xephyr + Sober, sandbox place)

- Play opens on the **Client** view: Workspace showed Camera, Terrain, the
  tester's character, Baseplate and SpawnLocation. The ribbon tile switches
  to **Server**, which shows the server's own tree and order.
- Selecting an instance fills Properties from the game (`Watch` → `Props`).
- Client view: Transparency 0.5 on SpawnLocation turned it see-through in
  the game, and the client's `Props` echoed it back.
- Server view: Transparency 0.8 replicated to the client (both sides'
  `Props` reported it). Renaming (F2) and deleting from the Explorer
  renamed and destroyed the instances in the game; the client's tree
  reported the rename too.
- A 7.6 MB place (rbx_binary's `miners-haven.rbxl` benchmark) published,
  ran embedded, and its live tree (Workspace with dozens of models)
  arrived within ~15 s with the editor responsive.
- The View menu dropped over the game's left edge draws fully above it.
- Stop restores the edited place exactly (live edits never reach the
  file, Auto-Recovery or Argon).
- `RBX_PLAY_TRACE=1` prints every ingested batch and every command handed
  to the probe on stderr.

Not done: script source in live views (game scripts cannot read `Source`;
opening a script during a test says so); editing `Tags` live; a stats
panel for the `Stats` message.
