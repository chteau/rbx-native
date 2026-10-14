# How Roblox Studio does presence and local playtests (static RE)

Target: `RobloxStudioBeta.exe`, `version-9b554450a0fc4e65` (Vinegar prefix), 214 MB PE32+,
image base `0x140000000`. Method: string extraction, RIP-relative LEA / `call [rip+x]`
xref scan, `objdump` of the referencing functions. No full Ghidra pass (too big), no
runtime capture. Addresses below are VAs in this build.

## 1. Presence "In Studio"

**Observed**

- Component `RBX::Studio::UserHeartbeats` (built from `UserActivityTracker` + `IUserContext`),
  plus `RBX::Studio::PlaceHeartbeats` (built from `UserHeartbeats` + `IPlaceSessionContext`).
- Two endpoints, both `POST` on the `apis` host (`apis.roblox.com`), built at
  `0x1465d6204` and `0x1465d5de4`:
  - `/user-heartbeats-api/pulse` (task name "User Heartbeats Pulse", timer "UserHeartbeats Timer")
  - `/user-heartbeats-api/action-report` (task names "UserHeartbeats ActionReport Login/Logout",
    "PlaceHeartbeats ActionReport Join/Leave")
- JSON serializer (`0x1465d6500`–`0x1465d7900`) field names:
  - pulse: `sessionInfo`, `locationInfo`, `clientSideTimestampEpochMs`, `ingestTimestampEpochMs`
  - action-report: `sessionInfo`, `actionInfo`, `clientSideTimestampEpochMs`, `ingestTimestampEpochMs`
  - `sessionInfo`: `sessionId`, `userId`, `platformTypeId`, `countryId`, `regionName`, `deviceType`
  - `locationInfo` is a oneof: `websiteLocationInfo` | `universalAppLocationInfo` |
    `studioLocationInfo` | `gameLocationInfo`
  - `actionInfo` oneof: `loginActionInfo`, `logoutActionInfo`,
    `universalAppActiveStateChangeActionInfo`, `studioJoinTestGameActionInfo`,
    `studioLeaveTestGameActionInfo`, `gameJoinActionInfo`, `gameLeaveActionInfo`,
    `teleportActionInfo`, `gameTeleportActionInfo`
  - leaf fields seen: `placeId`, `url`, `universeId`, `gameInstanceId`, `cloudEdit`,
    `pageName`, `activeState`, `testGame`, `fromGame`/`toGame`, `gameJoined`/`gameLeft`.
- Live PCStudioApp flags (clientsettingscdn, 2026-10-13): `FFlagUserHeartbeatsPulseApi=True`,
  `FIntUserHeartbeatsPulseIntervalSeconds=60`, `FIntUserHeartbeatsPulseRolloutPerMyriad=10000`,
  `FIntUserHeartbeatsActionReportRolloutPerMyriad=10000`,
  `FFlagStudioUsageTrackerDisableClientPresenceCall=True`,
  `FFlagStudioDataModelLoadUtilDisableClientPresenceCall=True`.

**Inferred**

- "In Studio" presence = a 60 s pulse to `user-heartbeats-api/pulse` whose `locationInfo` is
  `studioLocationInfo`, sent with the logged-in user's web auth (cookie / auth token, not
  an Open Cloud key). The older "client presence" call is switched off by the two
  `...DisableClientPresenceCall` flags.
- The four-field group `placeId, universeId, gameInstanceId, cloudEdit` is most likely
  `studioLocationInfo` (`cloudEdit` is a Team Create concept); exact field-to-struct
  mapping not proven.
- Starting/stopping a playtest sends `studioJoinTestGameActionInfo` /
  `studioLeaveTestGameActionInfo` (`testGame` field) — presence-only, no game-join.

**Unknown**: exact JSON value types/enums (`platformTypeId`, `deviceType`, `activeState`),
which auth header the request carries. A mitmproxy capture of one pulse would settle both.

## 2. Play / Test without RobloxPlayerBeta

**Observed**

- **Play Solo is in-process.** `DataModelLoader::loadPlaySoloServer` and
  `loadPlaySoloClient` (`0x1472319e8`, `0x147231604`) build two extra DataModels inside
  the Studio process; the client loader references `"localhost"`. Studio's own engine
  carries `NetworkServer` (RakNet + "RbxTransport", log "Started RbxTransport network
  server {}|{}") and the client replicator, so server and client talk over loopback.
- **Start Server / Clients spawns more copies of Studio itself**, not the player:
  `StartServerAction` launches task `StudioServer`, `StartClientsAction` launches
  `StudioPlayer` ×N (`StudioStartPlayerSleepTimeMs` between them).
  `PlaceSessionConfiguration` (`0x145c9df00`–`0x145c9e800`) builds the command line from:
  `-task`, `-placeId`, `-universeId`, `-port`, `-server`, `-rbxTransportPort`,
  `-rbxTransportToken`, `-numTestServerPlayersUponStartup`, `-playerEmulatorSerializedValues`,
  `-localPlaceFile`, `-creatorId`, `-creatorType`, `-placeVersion`, plus
  `executetestservice`/`studiotestservicemode`; a sibling builder (`0x141cc1c00`) adds
  `-parentPid`, `-parentSessionGuid`, `-baseUrl`, `-channel`, `-instanceId`,
  `-playTestSessionGuid`, `userid`. Debug flag `DebugStudioPlayerConnectToServerIPAddress`
  overrides the server address.
- Parent ↔ child coordination is a local socket protocol in `StudioApplicationManager`
  (`onNewConnection`/`onSocketReadyRead`): messages `rbxCheckServerExists`,
  `rbxRequestStartPlayer_…`, `rbxRequestGracefulClose`, replies `rbxResponseSuccess`/`Fail`.
- Process creation is plain `CreateProcessW` (call sites `0x1472db06e`, `0x147395c4f`
  "StudioProcessLauncher", `0x1429eecb2` "startNewProcess"). The import table has **no**
  `CreateJobObject`, `AssignProcessToJobObject`, `CreateRestrictedToken`,
  `CreateAppContainerProfile` or `CreateProcessAsUser`. The only `UpdateProcThreadAttribute`
  use is Crashpad's handler launch.
- Team Test is the exception: a real cloud RCC reached via matchmaking
  (`/v1/join-team-test`, `TeamTestJoinConfig`, `DebugLocalTeamTestRccHost/Port`), with the
  client in a Studio child process (`FFlagStudioHasTeamTestChildProcess2=True`).

**Inferred**

- "Sandbox" here is logical, not OS-level: the playtest server is Studio's own engine
  running a local `NetworkServer` on loopback, so it never calls game-join, never gets a
  `gameInstanceId` from Roblox, and therefore never counts as a second game session for
  the account. That is why playtesting does not kick you from a live server, while a
  second RobloxPlayerBeta/Sober join with the same account does (error 273, see
  `multi-instance.md`).
- Isolation from the edit DataModel comes from separate DataModels (Play Solo) or
  separate processes holding a serialized copy of the place (`-localPlaceFile`; "Failed to
  serialize DataModel for cloning with drafts"), not from a Windows sandbox.

**Unknown**: the `-task` value strings for each mode (only `StudioServer`/`StudioPlayer`
labels and `-task` itself were seen near the builder); how `-rbxTransportToken` is minted
and checked; whether the local server still hits any web API for the client's identity.

## What this means for rbx-native

- Presence: we can show "In Studio" by sending the same pulse every 60 s with
  `studioLocationInfo`, using the user's own session auth. Needs one captured request to
  copy the exact shape; Open Cloud keys will not cover it.
- Sandbox: the Studio route is "run our own server + client locally", which needs a
  Roblox engine we don't have. Sober/RobloxPlayerBeta can't host a server, so for us the
  options in `README.md` stand (published test place + alt account, or Luau Execution).
  Studio's approach is only reusable if we drive Studio itself (Vinegar) as the
  playtest host.

## 3. Live test: headless playtest without the editor (2026-10-13, Vinegar, Xephyr :7)

Exact command lines Studio uses (read from `/proc/<pid>/cmdline` after Test → Server & Clients, 1 player):

```
RobloxStudioBeta.exe -creatorId 0 -placeVersion 0 -task StartServer -localProjectFile C:/users/<u>/X.rbxl
  -placeId 0 -universeId 0 -port 0 -creatorType 0 -numTestServerPlayersUponStartup 1
  -edittargetid <guid> -editpid <pid> -userid <id> -parentPid <pid> -parentSessionGuid <GUID>
  -baseUrl https://www.roblox.com -channel <channel> -instanceId StudioServer -playTestSessionGuid <GUID>

RobloxStudioBeta.exe -task StartClient -rbxTransportToken bG9jYWxfdGVzdA== -localProjectFile C:/users/<u>/X.rbxl
  -universeId 0 -placeId 0 -port <raknet> -rbxTransportPort <transport> -numTestServerPlayersUponStartup 1
  -edittargetid … -editpid … -playerEmulatorSerializedValues {json} -userid <id> -parentPid <server pid>
  -parentSessionGuid … -baseUrl … -channel … -instanceId StudioPlayer_0 -playTestSessionGuid <same GUID>
```

Results:
- The **server runs standalone** (no editor, no `-parentPid/-editpid`) and, with
  `-numTestServerPlayersUponStartup N`, spawns its own clients.
- A **hand-launched client joins** a running server (`Player2` appeared). Needs `-port` and
  `-rbxTransportPort`; with `-port` only it never connects.
- `-rbxTransportToken` is the constant base64 `"local_test"`.
- The server accepts `-port` (RakNet, bound to 127.0.0.1) but **rejects `-rbxTransportPort`**
  ("Invalid Launch Intent! Too many arguments"). RbxTransport picks a random port and binds
  **0.0.0.0** — exposed on the LAN with a constant token; a launcher must firewall or accept it.
  `DFInt/FIntDebugLocalRbxTransportOffset` had no effect.
- A UDP relay on a fixed loopback port (passed to the client as `-rbxTransportPort`, forwarding
  to the port parsed from the server log line `Started RbxTransport network server 0.0.0.0|<port>`)
  works and lets server and client boot **in parallel**.
- `-task PlaySolo` is not a CLI task (InvalidTask → RBXCRASH).

Timings (cold, Wine prefix warm, Vulkan):

| path | to playable |
|---|---|
| open editor (~19 s) + Test Server & Clients (~25 s) | ~44 s |
| standalone server, server spawns client (serial) | ~10 s |
| server + client in parallel through relay (client launched 1 s later) | **~7.1 s** |

Breakdown of the parallel run: Vinegar/flatpak overhead ~0.5 s per process; server listening at
4.3 s (place load + auth ~2 s); client connected at 5.6 s, playable at 7.1 s.
Each process still boots the full Studio IDE (built-in plugins, Qt UI) — the biggest remaining cost.
Idle resource use with one client: server ~1.8 GB RSS / ~90 % CPU, client ~2.0 GB / ~155 % CPU (uncapped).
Both processes also log `ws://localhost:13469/studio` connect failures: that is the StudioMCP WS host
they look for (possible control channel).
