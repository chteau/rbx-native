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

## The shared secret

The secret is a string attribute `PresenceSecret` (64 lower-case hex digits)
on the folder `RbxNativePresence` in `ServerStorage`. The widget's "Create key"
button makes it. The room and signing key are derived from it and the PlaceId.

Anyone who can open the place file can read the secret, so an uncopylocked
place leaks it. Treat the secret as team-wide, not as strong access control.

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
