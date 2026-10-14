# Play/Test via a private sandbox and a probe: research synthesis

This file combines seven notes in this directory:
[open-cloud-place.md](open-cloud-place.md), [luau-execution.md](luau-execution.md),
[httpservice-probe.md](httpservice-probe.md), [tunnel-options.md](tunnel-options.md),
[client-launch.md](client-launch.md) [multi-instance.md](multi-instance.md) and [policy-risk.md](policy-risk.md).
No new research was done for it. Each confidence tag comes from the note the claim came from.

**TL;DR**
- **Verdict: the design is feasible with caveats.** Every step either has an official API path or a cheap fallback. No step is blocked outright. Three items must pass a live test before any code depends on them:
  1. `HttpEnabled` and `LoadStringEnabled` survive an Open Cloud publish.
  2. Roblox egress accepts a tunnel host.
  3. Sober joins a private place from a `roblox://` URI.
- Recommended tunnel: **cloudflared quick tunnel** by default, **Tailscale Funnel** as the fallback (Tailscale is already installed), and a "custom command" option. The channel is HTTPS request/response only: batched POSTs plus a ~25 s long-poll, under 400 requests/min.
- **"Don't disconnect my real game" is not possible on the same account.** Error 273/264 is enforced server-side per account. The only clean route is an alt account in an isolated client profile. It is optional and not the default.
- Add a separate client-less **Run/Test** mode built on Open Cloud Luau Execution. It is good for smoke tests and unit tests, but it cannot replace Play: it is a batch job, has rate caps, and keeps no state between tasks.

## Feasibility table

| Item | Verdict | Why (one line) |
|---|---|---|
| 1. Private place + `publish_place` | **Possible with caveats** | Publishing to an existing place works with an API key alone [Verified]. Creating the place via API is impossible, so it is created by hand. The body cap is 10 MiB. `PartOperation`, `SurfaceAppearance`, `EditableImage`, `EditableMesh` and `BaseWrap` are not updated by the API [Verified]. |
| 2. Launch the client via `roblox://placeId=` | **Possible with caveats** | The documented (though deprecated) deep link uses the client's own login [Verified]. Sober's URI handling is untested [Inferred]. Embedding: X11 reparenting only, untested. Wayland would need a nested compositor. Windows `SetParent` carries Hyperion risk. |
| 3. Inject `HttpEnabled`/`LoadStringEnabled` + probe | **Unknown** | Both properties are serialised in the place [Verified], but nothing documents whether an API publish preserves them. A live test is needed. The fallback is a one-time manual "Allow HTTP Requests" toggle. |
| 4. Tunnel + HTTP server, batched diffs, long-poll | **Possible with caveats** | HttpService request/response works from published servers, capped at ~500 requests/min [Verified]. WebSocket/SSE is Studio-only [Verified]. Whether Roblox accepts tunnel domains is untested. |
| 5. Anonymized sandbox | **Possible with caveats** | Name, description and visibility can be changed with PATCH [Verified]. The API cannot set the icon, but a new experience has none [Verified/Inferred]. |
| 6. Hot reload via `loadstring` | **Possible with caveats** (server) / **Not possible** (client) | `loadstring` works on the server only, and only if `LoadStringEnabled` survives the publish. Clients cannot `loadstring` [Verified/Inferred]. |
| Keep the real game running (same account) | **Not possible** | A second launch on the same account kicks the first (273/264) [Reported], and the check is account-level [Inferred]. |
| Keep the real game running (alt account, isolated profile) | **Unknown** | An alt account in a separate Sober data directory, a second OS user or bwrap should work [Inferred]. Untested. |
| Friends testing | **Possible with caveats** | Up to 10 early testers, added by hand. There is no API for it [Verified/Inferred]. |
| Multiple clients on one machine | **Possible with caveats** (alt accounts only) | Windows needs singleton-mutex hacks, which staff call malicious [Reported], so they will not be integrated. Sober does not support it [Reported]. |
| On-device engine / headless local run | **Not possible** | No local Roblox engine exists. Luau Execution runs on Roblox servers as batch jobs only [Verified]. |

## Possible
- [Verified] `POST /universes/v1/{u}/places/{p}/versions?versionType=Published` with scope `universe-places:write`, API key only, body ≤ 10 MiB. https://create.roblox.com/docs/cloud/guides/usage-place-publishing
- [Verified] `PATCH` on the place (`displayName`, `description`) and on the universe (`visibility`). `:restartServers` with `closeAllVersions`. Local OpenAPI `cloud.docs.json`; https://create.roblox.com/docs/projects/update-games
- [Verified] A publish does not stop old servers. Without a restart, the client could land on a stale version. https://create.roblox.com/docs/projects/update-games
- [Verified] The documented launch link is `roblox://placeId=<id>&launchData=<≤200 bytes>`, read in game via `Player:GetJoinData()`. https://create.roblox.com/docs/production/promotion/deeplinks
- [Verified] HttpService `GetAsync`, `PostAsync` and `RequestAsync` work from experience servers. The limit is 500 requests/min for non-Open-Cloud requests. HTTP/2 is used when the server offers it. https://create.roblox.com/docs/cloud-services/http-service
- [Verified] `LogService.MessageOut`, `GetLogHistory` and `ScriptContext.Error` cover server logging. RemoteEvents allow about 500 requests/s per client. https://create.roblox.com/docs/reference/engine/classes/LogService , https://create.roblox.com/docs/reference/engine/classes/RemoteEvent
- [Verified] Luau Execution runs version-pinned headless tasks. Each task is limited to 5 minutes and 10 concurrent tasks per place. Scope: `universe.place.luau-execution-session:write`. https://create.roblox.com/docs/en-us/cloud/reference/features/luau-execution.md
- [Verified] (local CLI) Tailscale 1.102.4 offers `tailscale funnel <port>`, `funnel status` and `funnel reset`. cloudflared is Apache-2.0. https://github.com/cloudflare/cloudflared , https://tailscale.com/kb/1223/funnel
- [Inferred] X11 `XReparentWindow` embedding of Sober on the maintainer's X11 session.
- [Inferred] An alt account in an isolated Sober profile keeps the user's live game running.

## Not possible
- [Verified] Creating a universe or place through Open Cloud. https://devforum.roblox.com/t/allow-editing-creating-experiences-with-open-cloud-oauth/4541620
- [Verified] Publishing places with OAuth. Setting the icon through the v2 API. Calling `:restartServers`/PATCH from in-game HttpService (`apiKeyWithHttpService: false`).
- [Verified] WebSocket/SSE (`CreateWebStreamClient`) from a published server: it is Studio-only. https://create.roblox.com/docs/reference/engine/classes/WebStreamClient
- [Inferred] Inbound connections to the game server. The probe has to poll.
- [Inferred] Reaching localhost from a published server (servers use public endpoints).
- [Inferred] Raw TCP tunnels such as bore, which are not HTTP(S).
- [Verified/Inferred] `loadstring` on the client.
- [Reported] Same account in two experiences at the same time (error 273/264). https://devforum.roblox.com/t/teleporting-players-causes-error-273-same-account-launched-experience-from-different-device/4013867
- [Inferred] A VM or sandbox cannot avoid the same-account kick, because the check is server-side.
- [Verified/Reported] Luau Execution as a live session: no persistent sessions, ~30 creates/hour in practice. https://devforum.roblox.com/t/4890044
- [Inferred] Minting auth tickets or reading cookies ourselves. The project forbids it, and nothing requires it.
- [Inferred] Cross-process embedding on Wayland without a nested compositor.

## Unknown / needs a live test
All of these are covered by the prioritized list below. Two have no test there:
- [Unknown] Hyperion's reaction to `SetParent` on Windows.
- [Unknown] Whether HttpService and DataStores work inside Luau Execution tasks in 2026. The 2024 beta said they were disabled [Reported].

## Revised design (replaces roadmap steps 1-6)

1. **One-time setup wizard (manual creation).**
   - The user creates a blank experience in Studio or on the website and publishes it once. It stays Private.
   - The user pastes the Universe ID and Place ID into the wizard.
   - The user creates a **separate sandbox API key** limited to that experience, with these scopes: `universe-places:write`, `universe.place:write`, `universe:write`, and optionally `universe.place.luau-execution-session:write`.
   - The wizard tells the user that the playing account must be the owner, a collaborator or an early tester.
   - The wizard does not promise automatic creation [Verified].
2. **Anonymize once at setup.** PATCH the place with a random `displayName` and an empty `description`, and set the universe `visibility` to `PRIVATE`. This happens at setup or before launch, never during a session, because PRIVATE kicks active players [Verified]. The icon is left as the default (none).
3. **Start the channel before publishing.**
   - The HTTP server binds to `127.0.0.1` on a random port.
   - A pluggable `TunnelProvider` starts the tunnel: **cloudflared quick tunnel by default**, with the URL scraped from stderr within ~20 s; **Tailscale Funnel as fallback**, held in the foreground, not `--bg`; or a custom command plus a URL regex.
   - The app then runs `GET /healthz` through the public URL with the token. If that fails, Play aborts here, before the slow publish.
4. **Inject into the in-memory DOM only, then pre-flight.**
   - Set `HttpService.HttpEnabled = true` and `ServerScriptService.LoadStringEnabled = true`.
   - Add a probe `Script`. Its attributes, on a server-only container, carry `BASE` (the tunnel URL), `TOKEN` (128+ random bits, new for each Play) and `SESSION`.
   - Add a fixed mirror `LocalScript` and a `RemoteEvent`.
   - Pre-flight checks: warn on the classes the API cannot publish, check that the serialized `.rbxl` is ≤ 10 MiB, and optionally compile-check the probe with a version-pinned Luau Execution task.
5. **Publish and restart.** Call `publish_place(..., Published)`, then `:restartServers` with `closeAllVersions=true`.
6. **Launch.**
   - First show a confirmation: "This will disconnect you from any Roblox game you're in with this account."
   - Then launch `roblox://placeId=<sandbox>&launchData=<SESSION>`. On Windows this uses the shell protocol handler. On Linux it runs `flatpak run org.vinegarhq.Sober <uri>`, with `xdg-open` as the fallback.
   - The client runs in its own window. X11 embedding is a later, optional prototype.
   - An optional **sandbox profile** (an alt account in an isolated Sober data directory, bwrap or a second OS user) keeps the user's live game. There are no singleton-mutex tricks, and the app never stores cookies.
7. **Channel protocol (HTTPS on 443, JSON, `X-Probe-Token`).**
   - The editor answers 404 on a token mismatch and compares tokens in constant time.
   - **Uplink** `POST /v1/ingest`: diffs coalesced every 200 ms only when something changed, capped at ~150/min, with the window growing to 1 s under load. Batches over ~200 KB are split.
   - Message types: `hello`, `diff` (add/set/del/move with a per-direction `seq`), a `snapshot` every ~30 s, `log`, `err`, and `stats` every ~2 s.
   - **Downlink** `GET /v1/poll?since=<seq>`: the editor holds the request ~25 s and replies with 204 or a command array. Commands: `eval`, `set`, `invoke`, `reload`, `shutdown`. Commands carry monotone ids and are acked and deduped.
   - The total budget is ≤ 400 requests/min. On 429 the probe backs off 2/4/8 s and then sends a snapshot.
   - On its first beat, the probe reports `HttpEnabled`, `LoadStringEnabled` and `IsStudio`. If HTTP is blocked, the UI tells the user to enable Allow HTTP Requests once in the experience settings.
8. **Hot reload.**
   - The server receives `eval` and `reload` commands through `loadstring`, one publish per session.
   - The client gets no pushed code. The mirror LocalScript is command-driven and relays client logs and state through `FireServer`, batched every ~200 ms.
   - Changes to client-side scripts need a republish. An optional pure-Luau interpreter could remove that later.
9. **Teardown.** A `Drop` guard kills the tunnel and the server when Play stops, the window closes or the app panics. Funnel `reset` runs only on configuration the app created.
10. **Run/Test mode (new, no client).** Publish `Saved` and record the version, then create a version-pinned Luau Execution task and show its logs and return value in Output. Runs are queued, capped at ~25/hour, and a 429 shows its `retry-after`.

## Contradictions between notes, and which wins
- **Does `HttpEnabled` survive a publish?**
  - httpservice-probe: a published place "carries the flag from the saved DOM".
  - open-cloud-place: not documented, live test needed.
  - **open-cloud-place wins**: the probe note's reading is an inference from Studio-oriented docs. Keep the first-beat self-check and the manual fallback.
- **Where the session token goes.**
  - client-launch suggests carrying it in `launchData`.
  - tunnel-options and httpservice-probe inject it into the DOM.
  - **DOM injection wins** for the secret token, because `launchData` is visible to the player [Inferred, client-launch]. `launchData` carries only the non-secret `SESSION` id.
- **`Saved` vs `Published`.**
  - luau-execution uses `Saved` for test versions.
  - open-cloud-place requires `Published` for Play.
  - **Both are right in their own context**: `Published` + restart for Play, `Saved` + version pin for Run/Test.
- **Sober's display backend.**
  - client-launch: Sober prefers Wayland when a Wayland socket exists [Verified, metadata].
  - A community guide says it works on both [Reported].
  - **No conflict on the maintainer's X11 session.** X11 embedding on Wayland stays Unknown.
- **The ~500 requests/min budget.**
  - tunnel-options leaves open whether polls and posts share the budget.
  - httpservice-probe budgets them together.
  - **Assume they share it** (the conservative choice) until measured.

## Prioritized live tests (run in this order)

| # | Test | Pass criterion |
|---|---|---|
| 1 | Publish (API, `Published`) a place with injected `HttpEnabled`/`LoadStringEnabled` and a probe that prints both, calls `loadstring("return 1")()`, and POSTs once to a public echo URL. Join with the owner account. | The server log or the echo shows `HttpEnabled=true`, `LoadStringEnabled=true`, loadstring returned 1, and the POST returned 200. |
| 2 | Same probe POSTs to `https://<x>.trycloudflare.com/healthz`, then to a Funnel `*.ts.net` URL. | Both hosts receive the request with the token header, and the server sees 200 with no HttpService error. |
| 3 | `flatpak run org.vinegarhq.Sober "roblox://placeId=<sandbox>&launchData=abc"` (then `xdg-open` if that fails). | The client joins the private place as the owner, and `GetJoinData().LaunchData == "abc"`. |
| 4 | Long-poll hold: the editor holds `/v1/poll` for 25 s, then 60 s, through each tunnel. | The 25 s poll completes with 204 and no HttpService timeout on both tunnels. Record the maximum hold that succeeds. |
| 5 | Budget: run ingest at 200 ms plus a continuous long-poll for 5 min. | No 429 or stall, and observed requests/min ≤ 400. |
| 6 | Publish→joinable latency, with `:restartServers` `closeAllVersions=true` before launch. | The client lands on the new `placeVersion` reported in `hello`. Record the time (target < 60 s). |
| 7 | Same-account kick confirmation: be in a live game on account A, then launch the sandbox with A. | Expected: the live game shows 273/264. This confirms the warning dialog is needed. |
| 8 | Alt profile: the live game runs on account A in the default Sober, and the sandbox runs on alt B in an isolated profile (bwrap or a second user). | Both sessions stay connected for 10 min, and the two profiles share no login state. |
| 9 | Luau Execution task on the sandbox version: `pcall` HttpService and DataStore calls, `RunService:IsRunning()`. | The task completes and returns JSON. Record which services work. |
| 10 | X11 reparent of Sober's window into an rbxstudio dock. | It renders, takes keyboard focus, and pointer lock works after reparenting. |

## Risks
- Roblox may silently ignore the injected flags or block tunnel domains. That would break steps 3 and 4. Mitigations: tests 1-2, the manual toggle, and the custom tunnel option.
- The probe is remote code execution by design (`loadstring` + HTTP). The token is per session, the place is private, the server is loopback-only, and responses are 404 on token mismatch. Script source and diffs pass through the tunnel vendor, and the UI must say so [Inferred].
- Places with unions or SurfaceAppearance publish incomplete through the API [Verified]. The pre-flight warning is the only mitigation.
- The publish API and Sober are both beta or unofficial. The API is marked BETA [Verified]. Sober is closed source and unsupported by Roblox [Verified].
- Rate caps: 500 HTTP requests/min [Verified]; Luau Execution 5/min plus a hidden ~30/hour [Reported].
- Policy: integrating multi-instance tooling has a [Reported] ban risk, so it is excluded. Alt accounts are allowed only as a user-managed option. The ToS text itself was not fetched.
- Windows `SetParent` under Hyperion is unverified, so the client stays in its own window there.

## Impact on the roadmap design
- **Step 1:**
  - Manual creation plus a wizard. A separate scoped key.
  - `Published` + `:restartServers`.
  - Pre-flight checks: classes the API cannot publish, and the 10 MiB cap.
  - The playing account must have access to the place.
- **Step 2:**
  - Launch with `roblox://placeId=…&launchData=<session>`.
  - Show a same-account disconnect warning.
  - Optional alt sandbox profile.
  - Embedding is X11-only and optional. On Windows, use positioning instead of `SetParent`.
- **Step 3:**
  - Add token, URL and session attributes and a first-beat flag self-check.
  - The tunnel must be up and health-checked **before** publish, so the order is reversed relative to the roadmap.
- **Step 4:**
  - A pluggable tunnel: cloudflared by default, Funnel as fallback, plus a custom command.
  - Request/response only: ~25 s long-poll, ≤ 400 requests/min, HTTPS on 443.
- **Step 5:** API PATCH at setup. The icon cannot be managed by the API, and the default is none.
- **Step 6:** Hot reload through `loadstring` on the server only. The client stays a command-driven mirror.
- **New:** a Run/Test mode on Luau Execution, and a Play confirmation dialog.

## Sources
- https://create.roblox.com/docs/cloud/guides/usage-place-publishing
- https://create.roblox.com/docs/production/publishing/publish-games-and-places
- https://create.roblox.com/docs/projects/update-games
- https://create.roblox.com/docs/cloud-services/http-service
- https://create.roblox.com/docs/reference/engine/classes/HttpService
- https://create.roblox.com/docs/reference/engine/classes/ServerScriptService
- https://create.roblox.com/docs/reference/engine/classes/WebStreamClient
- https://create.roblox.com/docs/reference/engine/classes/LogService
- https://create.roblox.com/docs/reference/engine/classes/RemoteEvent
- https://create.roblox.com/docs/production/promotion/deeplinks
- https://create.roblox.com/docs/en-us/cloud/reference/features/luau-execution.md
- https://create.roblox.com/docs/studio/mcp
- https://create.roblox.com/docs/studio/testing-modes
- https://devforum.roblox.com/t/allow-editing-creating-experiences-with-open-cloud-oauth/4541620
- https://devforum.roblox.com/t/4890044
- https://devforum.roblox.com/t/4889837
- https://devforum.roblox.com/t/teleporting-players-causes-error-273-same-account-launched-experience-from-different-device/4013867
- https://en.help.roblox.com/hc/en-us/articles/36665660855700-Error-Code-264-Same-account-launched-experience-from-different-devices
- https://github.com/vinegarhq/sober/issues/30
- https://sober.vinegarhq.org/
- https://github.com/cloudflare/cloudflared
- https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/do-more-with-tunnels/trycloudflare/
- https://tailscale.com/kb/1223/funnel
- https://roblox.fandom.com/wiki/Hyperion

## Live results (2026-10-14, `cargo run -p rbx_play --example live_probe`)

Sandbox: "Experimenting Place" (universe 1113565074, place 3086963177), Sober on Linux, cloudflared 2026.10.0 quick tunnel.

| # | Result |
|---|---|
| 1 | **Pass.** Published (API, `Published`, v340) with injected flags; hello reported `http_enabled: true, loadstring_enabled: true, is_studio: false`; `eval "return 1 + 1"` acked `"2"` 0.2 s later and its `print` came back as a server log. |
| 2 | **Pass** for trycloudflare: the game server reached the tunnel on first try. Funnel not tested. |
| 3 | **Pass.** `flatpak run org.vinegarhq.Sober "roblox://placeId=…&launchData=…"` joined directly; `launchData` arrived as the session id. |
| — | Leaving the game produced `Left { user_id }` at once, which ends the session. |

Findings that change the design:
- `ServerScriptService.LoadStringEnabled` is **not readable from scripts** ("not a valid member"); the probe tests `loadstring` by calling it.
- **Sober is single-instance**: a second `flatpak run` exits with "An instance of Sober is already running" (and a GUI error). Play must detect a running Sober and close it (`flatpak kill org.vinegarhq.Sober`) after confirmation.
- `TunnelProvider::start` must keep draining the child's pipes after the URL: cloudflared dies on its next log line otherwise (fixed, regression test in `tests/tunnel.rs`).
- Query the trycloudflare name only ~10 s after it is printed; an earlier lookup caches NXDOMAIN for about a minute (first healthz 200 at 15 s instead of 65 s).
- Timings: tunnel URL 5 s, healthz 15 s, published 17 s, probe hello 4.8 s after launch.
