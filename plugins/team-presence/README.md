# Team Presence plugin

A Roblox Studio plugin that shares who is working where with rbx-native
users through a relay. Studio users see rbx-native collaborators as markers
in the viewport (camera, selection) and in a dock widget drawn in
rbx-native's own look: their Roblox headshot in a ring of their colour, open
script, latest edit, and a "While you were away" card whose **View changes**
opens a diff window like rbx-native's (who edited which property, and its
value in the place now).

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

Collaborators do not configure anything: they open the plugin and switch on
**Share my presence**. Everything the connection needs lives in the place itself, on the
folder `RbxNativePresence` in `ServerStorage`:

- `PresenceSecret` — the shared secret (64 lower-case hex digits). The first
  person to connect mints it automatically; everyone else reads it. The room
  and signing key are derived from it and the `PlaceId`.
- `RelayKind` / `RelayUrl` — which relay the team uses (see below). Unset
  means **Auto**, which needs nothing. A place **owner** who runs their own
  relay sets these once under "Show advanced"; every collaborator reads them
  from the place.

Writing any of these is an undoable edit; it reaches other collaborators only
after a **Save** or **Publish**, and the widget says so.

Anyone who can open the place file can read the secret, so an uncopylocked
place leaks it. Treat the secret as team-wide, not as strong access control.

## Relays and transports

Three transports, all official:

- **hosted** (Auto, the default) — no account and no server. The first
  rbx-native user sharing presence in the place runs the relay itself behind a
  Cloudflare quick tunnel (from the `cloudflared` bundled with rbx-native) and
  announces the tunnel's address, signed with the place key, on the room's
  ntfy.sh announce topic (`rbxnp-<room>-host`, a few messages a day). The
  plugin reads the newest announcement and connects. Studio cannot host, so
  with nobody in rbx-native the widget says it is waiting; Studio users see
  each other through Team Create anyway.
- **worker** — the signed-WebSocket relay in `relay/` (a Cloudflare Worker),
  which keeps the room in memory and fans out snapshots. Self-hosted.
- **ntfy** — a plain [ntfy](https://ntfy.sh) pub/sub server, which needs no code
  of ours. Each client publishes its signed presence to a topic derived from
  the room (`rbxnp-<room>`) and rebuilds the room from the message stream.
  Peers trust each other by HMAC, never the server. Only for a **self-hosted**
  ntfy: public ntfy.sh allows 250 messages a day per address, which live
  presence uses up in minutes.

The Auto announce server (ntfy.sh) can be changed at build time with
`RBX_PRESENCE_COMMUNITY_URL`; it is written into the plugin's generated
`Defaults` module from the one Rust constant (`rbx_presence::relay`), so the
plugin and the editor can never disagree. The plugin's colours come the same
way from rbx-native's `assets/themes/default/theme.json` (generated `Theme`
module).

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

Copy the `.rbxmx` into the local Plugins folder and restart Studio:

- Windows: `%LOCALAPPDATA%\Roblox\Plugins`
- macOS: `~/Documents/Roblox/Plugins`
- Linux (Vinegar): `~/.var/app/org.vinegarhq.Vinegar/data/vinegar/prefixes/studio/drive_c/users/<you>/AppData/Local/Roblox/Plugins`

Studio may prompt for HTTP permission for the relay's domain; allow it,
otherwise the plugin cannot reach the relay. (A local plugin was never asked
in testing; a quick tunnel's address is a new `*.trycloudflare.com` subdomain
each time, which may prompt again for a Creator Store install.)

## Tests

```
cargo test -p rbx_presence --test luau
```

This runs the pure Luau modules (hashing, signing, JSON, paths, schema,
catch-up, announcements) in a plain Luau interpreter against the shared golden
fixtures and the Rust encoders. `--test relay_e2e` and `--test hosted_e2e`
also drive the plugin's own `Relay.luau` against rbx-native's relay over HTTP.
