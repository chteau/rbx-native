# Open Cloud for a sandbox place (steps 1 and 5)

**TL;DR**
- Publishing a new version to an EXISTING place is a stable, API-key-only call (`universe-places:write`, `Saved` or `Published`, 10 MiB body cap). `rbx_cloud::publish_place` targets the right endpoint.
- A universe or place cannot be created through Open Cloud (confirmed by a March 2026 DevForum thread). The one-time setup must be done by hand in Studio or on the website.
- Open Cloud v2 can PATCH universe `visibility` (and the root place's name/description) and restart servers. It cannot set the icon, and the docs do not say whether the place file's `HttpEnabled`/`LoadStringEnabled` survive an API publish. That needs a live test.
- Joining a private experience is limited to the owner, people with Edit access, and invited early testers (max 10). The "friends" gate is a separate setting. Older servers are not killed by a publish, so session start should call `:restartServers`.

## Possible

### Publish to an existing place (Place Publishing API v1)
- [Verified] `POST https://apis.roblox.com/universes/v1/{universeId}/places/{placeId}/versions?versionType=Saved|Published`. The body is a `.rbxl` (`application/octet-stream`) or `.rbxlx` (`application/xml`). The response is `{"versionNumber": N}`. Source: https://create.roblox.com/docs/cloud/guides/usage-place-publishing and the local OpenAPI `reference/cloud/universes-api/v1.json`.
- [Verified] `Saved` stores a version without making it live. `Published` also makes it live. For a private sandbox place, `Saved` should not be enough for new servers (see "Server version" below). `Published` is the mode to use. Source: the OpenAPI `versionType` description.
- [Verified] Scope is `universe-places:write` on the target universe. The key is sent in the `x-api-key` header. OAuth is not supported for this endpoint. Source: the OpenAPI above and https://devforum.roblox.com/t/allow-editing-creating-experiences-with-open-cloud-oauth/4541620 (2026-03-27: "you can only publish to places using Open Cloud with an API key (not OAuth)").
- [Verified] Size limit: the spec carries `x-roblox-size-limit: 10485760` (10 MiB). A probe-injected place must stay under it. The `.rbxl` binary form is smaller than `.rbxlx`.
- [Verified] Error codes: 400 invalid file, 401 key invalid for the operation, 403 publish not allowed on place, 404 place or universe missing, 409 place not in universe, 500.
- [Verified] The API is marked BETA in the spec. HTTPS only.
- [Verified] Limitation: the API does not update places that contain `EditableImage`, `EditableMesh`, `PartOperation`, `SurfaceAppearance` or `BaseWrap` instances. The docs say to publish those from Studio. Source: the usage guide above.
- [Inferred] That limitation can break "Play" on a place with unions (`PartOperation`) or `SurfaceAppearance`. The sandbox would silently lack them. rbxstudio should detect those classes before publishing and warn.
- [Verified] The place-publishing page in the local docs does not state a rate limit for this endpoint. See "Unknown".

### Universe and place settings via Open Cloud v2
- [Verified] `PATCH /cloud/v2/universes/{universe_id}?updateMask=…` ("Update Universe"). Scope `universe:write`. Source: the local `reference/cloud/cloud.docs.json` (`Cloud_UpdateUniverse`).
- [Verified] Universe fields: `visibility` (`PUBLIC` | `PRIVATE`; `VISIBILITY_UNSPECIFIED` errors), `voiceChatEnabled`, `desktopEnabled`, `mobileEnabled`, `tabletEnabled`, `consoleEnabled`, `vrEnabled`, `privateServerPriceRobux`, and the social links. `displayName` and `description` are read-only on the universe. The schema says they are "updated by updating the root place's" name or description.
- [Verified] Setting `visibility` to `PRIVATE` removes all active players from running servers immediately. Source: the `Universe.visibility` description.
- [Verified] `PATCH /cloud/v2/universes/{universe_id}/places/{place_id}?updateMask=…`. Scope `universe.place:write`. The mutable fields in the schema are `displayName`, `description` and `serverSize`. Anonymizing the sandbox (step 5: random name, empty description) is therefore doable through the API after the manual creation.
- [Verified] There is no icon or thumbnail field on `Universe` or `Place` in this schema, so the icon cannot be cleared or set through these resources. [Inferred] A brand-new place has none by default, which already matches "no icon".
- [Verified] `POST /cloud/v2/universes/{universe_id}:restartServers` (scope `universe:write`). Body: `placeIds[]` (default all), `closeAllVersions` (default restarts only servers on older versions), `bleedOffServers` and `bleedOffDurationMinutes` (1-60). Source: the `RestartUniverseServersRequest` schema.
- [Verified] The Creator Hub UI equivalent is Configurations > Server management > Restart Servers. Source: https://create.roblox.com/docs/projects/update-games.

### One-time manual setup
- [Verified] The Place Publishing API only updates an existing place. The user needs the Universe ID and Place ID, taken from the Creator Dashboard ("Copy Universe ID" and the URL `…/experiences/<universe>/places/<place>/configure`). Source: the usage guide.
- [Inferred] A first-run wizard could cover these steps:
  1. In Studio or the website, create a new experience with a blank baseplate and publish it once (the Settings > Game Settings dialog already exists in rbxstudio).
  2. Keep it Private (the default for a new experience until released).
  3. Paste the Universe and Place IDs into rbxstudio settings.
  4. Create an API key with `universe-places:write` plus, optionally, `universe:write` and `universe.place:write`, restricted to that one experience.
- [Inferred] The key wizard shipped in the Home work already asks for three scopes. A fourth and fifth scope, or a separate sandbox key, would be needed.

### Who can join a private experience
- [Verified] Before release, a game is playable by users with Edit access plus the **early testers** the owner invites (up to 10, similar-age friends or trusted friends). The Playtest tab has "Early testers" (always on) and "Playtesters". The Public tab has a "Friends" toggle. Source: https://create.roblox.com/docs/production/publishing/publish-games-and-places (target audience and early-tester sections).
- [Verified] For group-owned games, access depends on group role permissions and can be adjusted per game. Source: https://create.roblox.com/docs/projects/configure-games.
- [Inferred] "Early testing with friends" works without making the place public. Add them as early testers by hand (dashboard or Studio's Manage Collaborators). Open Cloud v2 has no endpoint for this in the schema I read.
- [Verified] Age-check and age-verification prompts apply when inviting early testers. A friend outside the age range needs trusted-friend status.

### Server version and time to joinable
- [Verified] Publishing does not remove players from old servers. They keep running the old version until they empty and shut down, unless servers are restarted. Source: https://create.roblox.com/docs/projects/update-games.
- [Verified] After a restart, Roblox stops matchmaking into the old servers, waits out any bleed-off delay, then teleports players to a new server running the new version. Players can still join the old servers by invite, teleport or profile join.
- [Inferred] For a single-developer sandbox with no players, publish (`Published`), then call `:restartServers` with `closeAllVersions=true` if a stale server is possibly running. Launching the client afterwards lands on a fresh server with the new version. Since the sandbox is private and one developer owns it, bleed-off is unnecessary.
- [Inferred] The probe's hot-reload (step 6) removes the need to restart for later edits. Only the first publish per session needs the restart.

## Not possible
- [Verified] Creating a universe or place through Open Cloud. The DevForum request (2026-03-27) says "there is no way to create new universes with Open Cloud". https://devforum.roblox.com/t/allow-editing-creating-experiences-with-open-cloud-oauth/4541620. [Reported] Third-party tools use undocumented endpoints; those are not a supported path.
- [Verified] OAuth for place publishing. Only API keys.
- [Verified] Setting universe `displayName`/`description` directly. They are changed via the root place resource.
- [Verified] Clearing or setting the icon through Open Cloud v2 Universe/Place (no field in the schema I read).
- [Verified] Publishing places that contain `EditableImage`, `EditableMesh`, `PartOperation`, `SurfaceAppearance` or `BaseWrap` content. The API does not update them.
- [Verified] Calling `:restartServers` or `PATCH` from the in-game `HttpService`: both operations carry `apiKeyWithHttpService: false`. [Inferred] The probe cannot self-restart or self-publish.
- [Inferred] Editing the early-tester list through Open Cloud. No such endpoint was found in the schema. Manual only.

## Unknown / needs a live test
- Whether `HttpService.HttpEnabled` set in the published file is honoured. The docs describe it as an experience setting ("File > Experience Settings > Security > Allow HTTP Requests", https://create.roblox.com/docs/cloud-services/http-service). The engine class exposes it as a property of `HttpService` ([Verified] https://create.roblox.com/docs/reference/engine/classes/HttpService). [Inferred] In Studio it is stored in the place file, but whether the Place Publishing API preserves it or the server overrides it from stored experience settings is not documented. Test: publish a probe that prints `HttpService.HttpEnabled` and try a `GetAsync`.
- Same question for `ServerScriptService.LoadStringEnabled`. The class doc marks it `NotReplicated`/`NotScriptable`, defaults false, and "recommended to keep this disabled" ([Verified] https://create.roblox.com/docs/reference/engine/classes/ServerScriptService). Test the same way: publish and call `loadstring("return 1")` on the server.
- Fallback if either is ignored: the user toggles Allow HTTP Requests once in the sandbox experience's settings. [Inferred] That setting persists on the experience, so it would be a one-time manual step.
- Publish rate limit and typical latency from `Published` response to a joinable fresh server. Not in the local docs. Measure it.
- Whether `Saved` versions can be joined at all by the owner (no documented way to pick a `Saved` version for the live servers). [Inferred] Use `Published`.
- Whether the PATCH of `visibility` is needed at all on a never-released experience, and whether the field can be written by a key (it is marked `readOnly` in the schema yet is documented as the way to change privacy). Test with a throwaway experience.
- Whether a Cloud API key with `universe-places:write` also needs the "private place" experience to have been published at least once from Studio. [Inferred] Likely yes (the place needs a saved version).
- Whether the place file's `ServerSize`, attributes, or `Workspace.StreamingEnabled` etc. are overwritten by the publish. Not documented.

## Impact on the roadmap design
- **Step 1 (private place, publish)**: Keep `publish_place` with `Published` mode. Add (a) a pre-flight scan for `EditableImage`/`EditableMesh`/`PartOperation`/`SurfaceAppearance`/`BaseWrap` with a warning that they will not be updated; (b) a 10 MiB check on the serialized body, preferring binary `.rbxl`; (c) a post-publish `:restartServers` call (scope `universe:write`) so the launched client never lands on an old server. Say plainly in the setup wizard that creating the experience is manual, with the IDs pasted in. Do not promise auto-creation.
- **Step 2 (launch)**: Unchanged by this note. The launch URL is the Place ID of the sandbox place. [Inferred] The user's account must be the owner, a collaborator, or an invited early tester to join.
- **Step 3 (injection)**: The injected `HttpEnabled`/`LoadStringEnabled` values may not take effect through an API publish. Add a probe self-check: the server-side probe reports both flags on its first beat, and rbxstudio shows "Enable Allow HTTP Requests in the sandbox experience settings" if HTTP is blocked. Needs a live test before trusting the injection.
- **Step 5 (anonymization)**: Feasible after the manual creation. PATCH the place (`displayName` randomized, `description` empty) with `universe.place:write`, and PATCH the universe `visibility` to `PRIVATE` with `universe:write`. The icon cannot be managed by the API; rely on the new-experience default of none. Setting `PRIVATE` kicks active players, so it should run before launch, not during.
- **Friends**: The friend-testing story is "owner adds early testers by hand, up to 10". There is no API to automate it.
- **Key scopes**: The sandbox key needs `universe-places:write`, `universe.place:write`, `universe:write`, all scoped to the sandbox experience only. Recommend a separate key from the user's production one.

## Sources
- https://create.roblox.com/docs/cloud/guides/usage-place-publishing
- https://create.roblox.com/docs/production/publishing/publish-games-and-places
- https://create.roblox.com/docs/projects/update-games
- https://create.roblox.com/docs/projects/configure-games
- https://create.roblox.com/docs/cloud-services/http-service
- https://create.roblox.com/docs/reference/engine/classes/HttpService
- https://create.roblox.com/docs/reference/engine/classes/ServerScriptService
- https://devforum.roblox.com/t/allow-editing-creating-experiences-with-open-cloud-oauth/4541620
- Local clone (Roblox/creator-docs): `content/en-us/reference/cloud/universes-api/v1.json` and `content/en-us/reference/cloud/cloud.docs.json` (Universe, Place, `:restartServers` schemas)
- https://create.roblox.com/docs/cloud/reference/rate-limits
