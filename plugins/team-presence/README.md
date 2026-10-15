# Team Presence plugin

A Roblox Studio plugin that shares who is working where with rbx-native
users through a relay. Studio users see rbx-native collaborators as markers
in the viewport (camera, selection) and in a dock widget (open script, latest
edit).

## What it does and does not do

- Presence only exists while Studio is open with the plugin running and
  enabled. Closing Studio (or disabling the plugin) removes you from the room.
- Studio's own Team Create already shows Studio collaborators, so markers and
  the widget list show rbx-native users only.
- Nothing is edited or synchronised: presence is cameras, selections, the open
  script and recent property edits.

## How edits are attributed

Only changes made while a local `ChangeHistoryService` recording is open, or
within 250 ms either side of a local undo/redo, count as yours
(Studio does not document whether an undo's changes arrive before or after
its `OnUndo` signal).

- Studio tools or plugins that change things without a recording (legacy
  `SetWaypoint`) are not reported.
- A collaborator's replicated change to a selected instance that lands while a
  local recording is open, or within 250 ms of a local undo/redo, is still
  misattributed.
- Only selected instances are watched.
- Whether Studio's own Properties and dragger edits raise
  `OnRecordingStarted` is not verified.

## Colours

Colours are assigned per room from the user ids, so every client shows the
same colour for the same user.

## Zero-config connect

Collaborators do not configure anything: they open the plugin and click
**Connect**. Everything the connection needs lives in the place itself, on the
folder `RbxNativePresence` in `ServerStorage`:

- `PresenceSecret` — the shared secret (64 lower-case hex digits). The first
  person to connect mints it automatically; everyone else reads it. The room
  and signing key are derived from it and the `PlaceId`.
- `RelayKind` / `RelayUrl` — which relay the team uses (see below). The place
  **owner** sets these once in the widget's hidden "Owner setup" section; every
  collaborator reads them from the place.

Writing any of these is an undoable edit; it reaches other collaborators only
after a **Save** or **Publish**, and the widget says so.

Anyone who can open the place file can read the secret, so an uncopylocked
place leaks it. Treat the secret as team-wide, not as strong access control.

## Relays and transports

Two transports, both official:

- **worker** — the signed-WebSocket relay in `relay/` (a Cloudflare Worker),
  which keeps the room in memory and fans out snapshots. Self-hosted.
- **ntfy** — a plain [ntfy](https://ntfy.sh) pub/sub server, which needs no code
  of ours. Each client publishes its signed presence to a topic derived from
  the room (`rbxnp-<room>`) and rebuilds the room from the message stream.
  Peers trust each other by HMAC, never the server. The topic is unguessable
  (derived from the secret), so only holders of the place secret can read it.

If the place names no relay, both sides fall back to a **community default**
(a public ntfy instance): best effort, no uptime promise. Override it at build
time with `RBX_PRESENCE_COMMUNITY_URL`. The default is written into the plugin's
generated `Defaults` module from the one Rust constant
(`rbx_presence::relay`), so the plugin and the editor can never disagree.

Note: a public ntfy server caps message size (ntfy.sh ~4 KB), so a very large
selection may not publish there; a self-hosted worker or ntfy raises that.

## Paths

Selections and edits are sent as dotted paths from the service down (`game`
omitted). A name containing `.` or `\` is escaped. If siblings share a name,
the path resolves to the first child (`FindFirstChild`), so duplicate names are
ambiguous.

## Building

```
cargo run -p rbx_presence --bin build-presence-plugin
```

This produces a `.rbxmx` file.

## Installing

Copy the `.rbxmx` into the local Plugins folder:

- Windows: `%LOCALAPPDATA%\Roblox\Plugins`
- macOS: `~/Documents/Roblox/Plugins`

On first enable Studio prompts for HTTP permission for the relay URL; allow it,
otherwise the plugin cannot reach the relay.

## Tests

```
cargo test -p rbx_presence --test luau
```

This runs the pure Luau modules (hashing, signing, JSON, paths, schema) in a
plain Luau interpreter.
