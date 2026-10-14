# Headless engine alternatives: Open Cloud Luau Execution, Studio MCP, Lune / LuneTest

**TL;DR**
- Open Cloud **Luau Execution** runs your Luau on a real Roblox engine server (headless, a pinned place version). It's stable, API-key based, and `rbx_cloud` can drive it. It is a *batch job*, not a live session.
- Each task is a fresh server: no persistent state between tasks, startup latency, 5 min max, 5 creates/min documented, and a reported hidden ~30 creates/hour cap (Sept 2026). So it is **not** usable for ~200 ms probe diffs or per-keystroke hot reload.
- It **is** a good fit for a "Run (server-only)" smoke test and for unit/Jest-style tests of a place, run in batches. No client, no player, no tunnel needed.
- Studio's built-in MCP server (`start_stop_play`, `execute_luau`) gives a real Play session, but only inside a logged-in Roblox Studio. rbxstudio can't host it.
- Lune is a local Luau runtime with no Roblox engine; useful for pure-logic tests, not for engine behaviour.

## What Luau Execution is

- [Verified] "Run Luau scripts headlessly in a Roblox place via Open Cloud ... with full DataModel and engine Luau API access." Tasks run up to 5 minutes, up to 10 concurrent tasks per place; can save changes via `AssetService:SavePlaceAsync`, take large binary inputs and use `SerializationService` to deserialize RBXM content. https://create.roblox.com/docs/en-us/cloud/reference/features/luau-execution.md
- [Verified] Flow: create task (universe, place, optional version, script) → a server launches, loads the place, runs the code → poll the task → read return values and logs. Same page. Beta announcement: https://devforum.roblox.com/t/beta-open-cloud-engine-api-for-executing-luau/3172185
- [Verified] Endpoints (all marked STABLE in 2026): `POST /cloud/v2/universes/{u}/places/{p}/luau-execution-session-tasks` (head version), the same under `/versions/{v}/` (version pinned), `GET .../luau-execution-sessions/{session}/tasks/{task}`, `GET .../tasks/{task}/logs`, and `POST /cloud/v2/universes/{u}/luau-execution-session-task-binary-inputs`.
- [Verified] The beta text said changes did not persist; the 2026 reference now documents `SavePlaceAsync` for saving. The engine docs flag the Open Cloud context specifically (`AssetService` and `SerializationService` pages, `SecurityCapability.PluginOrOpenCloud`): https://create.roblox.com/docs/reference/engine/classes/SerializationService

## Limits

| Item | Value | Tag / source |
|---|---|---|
| Max task runtime | `timeout` field, default 5 min, max 5 min | [Verified] reference page above |
| Concurrent tasks | 10 incomplete per place (11th gets HTTP 429) | [Verified] reference. The 2024 beta said 2 per place [Reported] in the beta thread, so it was raised |
| Task create rate | 5/min per API key owner (head and version-pinned create, and binary-input create) | [Verified] reference ("Rate Limits: perApiKeyOwner: 5/minute"); the head limit was cut from 40 to 5 on Sep 16 2026 [Reported] https://devforum.roblox.com/t/4889837 |
| Hidden long-window cap | ~30 creates per 60 min per key owner, then 429 `RESOURCE_EXHAUSTED` with `retry-after` ~1800-3250 s; not in headers or schema | [Reported] (two independent CI-tool authors, 2026-09-23) https://devforum.roblox.com/t/4890044 |
| Binary input | up to 100 MiB per input, presigned URL valid 15 min | [Verified] reference |
| Return value + logs | return values via the task `output`; `print` output via the logs endpoint after completion; return size has a total cap and exceeding it fails the task | [Verified] qualitatively; the exact byte cap was not captured, see Unknown |
| Logs paging | default and max page size 10000 entries; `FLAT` or `STRUCTURED` view | [Verified] reference |
| Request bodies | 200 KB limit on the Instance API (a different API) | [Verified] https://create.roblox.com/docs/en-us/cloud/guides/instance.md |
| First task on a newly saved version | sometimes sits in `PROCESSING` ~11 min then `INTERNAL_ERROR`; a second task on that version runs in ~2 s | [Reported] https://devforum.roblox.com/t/4889837 |

## Auth, scope, price

- [Verified] Scope: `universe.place.luau-execution-session:write` on an API key bound to the experience (the beta thread called it `luau-execution-sessions:write`, the older name [Reported]). Auth is API key, OAuth 2.0, or cookie (docs say not to use cookies).
- [Verified] The reference lists no price. [Inferred] It is free like other Open Cloud endpoints, but only the rate limits above bind. Treat "no published pricing" as unconfirmed; do not tell users it is free.
- [Verified] Scope needs a universe the key's owner controls. The existing test universe from the roadmap works; no live-game access is needed.

## What runs inside the task server

- [Verified] The `RunService` docs table has a "Luau Execution" row (false/false/true/-/false/false) distinct from the Play and Team Test rows: https://create.roblox.com/docs/reference/engine/classes/RunService. I did not map the columns to method names. [Inferred] The only `true` is probably the edit-style flag, so the task is an edit-like DataModel, not a running game; scripts under `ServerScriptService` likely do not auto-run, and the task script must `require` modules itself.
- [Reported] In the 2024 beta announcement thread, Roblox staff said access to "some cloud service APIs (like DataStores, HttpService, etc.) is disabled to prevent accidental overwriting of production data." Users asked for opt-in. https://devforum.roblox.com/t/beta-open-cloud-engine-api-for-executing-luau/3172185. [Unknown] whether that is still true in Oct 2026; the current reference says nothing either way.
- [Reported] Plugin-level security identity: users observed engine APIs normally plugin-only are usable (the capability `PluginOrOpenCloud` exists in the engine enum [Verified]). `StepPhysics`-style calls were asked for in the same thread; no confirmed answer found.
- [Inferred] Players: there are no players and no client. `Players`, `Player.Character`, replication and RemoteEvents have nothing to talk to; physics is not auto-stepping as in a running game (edit-mode semantics).

## Verdict per feature for rbxstudio

| Feature | Fit | Why |
|---|---|---|
| "Run" (server-only sim) | **Partial** | Fine for "load the place, run `X`, report errors/return value", e.g. a smoke test or a script that `require`s modules and runs a scenario. Not a live running game: no auto-start of Scripts, no clients, 5 min cap, startup latency of seconds ([Reported] ~2 s warm, minutes when cold). |
| Unit tests (Jest-Lua / TestEZ) | **Good** | This is the intended use. Upload place version (`rbx_cloud::publish_place` with `Saved` mode gives a version number), create a version-pinned task that runs the test runner, return results as JSON. Real-world tool: `jest-roblox-cli` with an Open Cloud backend https://github.com/christopher-buss/jest-roblox-cli. Budget: one task per test run, ≤ ~30 runs/hour per key owner. |
| Hot reload | **No** | A task can't reach into an earlier task's server (sessions can't be created, only tasks; feature request open: https://devforum.roblox.com/t/4221716). Each edit would cost a publish + task, over the 5/min and hidden ~30/h caps. |
| Probe / live state mirror | **No** | Task finishes and the server dies; nothing stays up for the probe loop. |
| Validating the injected probe | **Good, cheap** | Before the real Play publish, run a task against the version to `loadstring`/compile-check the probe scripts and confirm the DOM injection deserializes. [Inferred] |

## Studio built-in MCP server

- [Verified] Studio ships an MCP server (stdio) exposing `execute_luau` (datamodel_type Edit / Client / Server), `start_stop_play`, `get_studio_state`, `get_console_output`, `screen_capture`, `character_navigation`, `user_keyboard_input`, `user_mouse_input`, plus script/instance tools. https://create.roblox.com/docs/studio/mcp
- [Verified] Announced 2026-03-05: built-in server, playtest automation with virtual input. https://devforum.roblox.com/t/assistant-updates-studio-built-in-mcp-server-and-playtest-automation/4474643 The earlier open-source server: https://github.com/Roblox/studio-rust-mcp-server
- [Reported] Rough edges: with "Server & Client" local test the MCP only saw the Edit state and couldn't address individual windows (https://devforum.roblox.com/t/4849810).
- **Fit for rbxstudio: none as a dependency.** It requires the user to run real Roblox Studio (on Linux, Vinegar/Wine) and log in. It is a good *comparison* for the tool surface we should expose, and the user could point an AI client at both. It competes with, rather than implements, a native Play.

## Lune, LuneTest, run-in-roblox

- [Reported] **Lune** is a standalone Luau runtime (not the Roblox engine). It can load a Rojo-built place file and run scripts against a simulated DataModel. No physics, rendering, or real replication. Pure-logic tests only. Lune's own repo page wasn't reachable via the research tool (`luau-lang/lune` 404), so version and API details are [Unknown].
- [Reported] **LuneTest** (Oct 2026, 0.x): `lune run test` runs unit specs in Lune (6000 tests in ~0.2 s claimed), tries e2e specs in Lune first, falls back to real Studio, and uses Open Cloud Luau Execution for server specs in CI ("your place is never changed"). Mostly tested on macOS. https://devforum.roblox.com/t/4914875 and https://github.com/AustinBarikdar/LuneTest
- [Reported] **jest-roblox-cli**: Jest for roblox-ts / Luau with three backends: Open Cloud, attached Studio, and self-launched headless Studio. https://github.com/christopher-buss/jest-roblox-cli
- [Reported] Roblox's own reference CI project: https://github.com/Roblox/place-ci-cd-demo (linked from the docs and the beta post).
- **run-in-roblox**: the old Rojo-team tool that drove local Studio; the research tool couldn't load its repo page. [Unknown] its 2026 status. [Inferred] Superseded by Luau Execution and the MCP server; needs Studio on Windows/macOS anyway.
- **Local engine on Linux**: none. All "real engine" options run on Roblox servers (Luau Execution) or inside Studio.

## Possible

- [Verified] Run a Luau script server-side against a specific saved place version and read return value + logs, from `rbx_cloud`, with a single scoped API key.
- [Verified] Pass the edited place by publishing a version first, then pin the task to that `version_id` (isolates concurrent runs).
- [Verified] Use it as a test runner (Jest-Lua/TestEZ) with batch results as the task's return value.
- [Verified] Pull structured logs (`view=STRUCTURED`) to show Output-window style lines in rbxstudio after the task.
- [Reported] Combine with a pure-Lune fast path for logic tests, falling back to cloud.

## Not possible

- [Verified] A persistent session shared across tasks (only tasks exist; sessions can't be created).
- [Inferred] Streaming logs while the task runs (logs are read after completion per the docs: "after the task completes").
- [Inferred] Players, a client, character, UI, input, or rendering. A Luau Execution task is not a playtest.
- [Verified] Running longer than 5 minutes, or more than 10 concurrent tasks per place.
- [Reported] High-frequency use: ~30 creates/hour/key owner in practice.

## Unknown / needs a live test

- Whether `HttpService`, DataStores, `MessagingService` work in a task today (2024 said disabled; 2026 docs silent). Test: task that does `pcall(HttpService.GetAsync, ...)` and `pcall(DataStoreService.GetDataStore("x").GetAsync, ...)`.
- Whether `RunService:Run()`, `Workspace` physics, or Heartbeat advance inside a task (the RunService table suggests an Edit-like context). Test: print `RunService:IsRunning()` and measure `Heartbeat:Wait()` times.
- Whether server `Script`s under `ServerScriptService` run automatically (we assume not).
- Exact return-value size cap and log volume cap; cold-start latency distribution on a fresh publish.
- Whether the hidden ~30/h cap applies to a free test-universe key owner in our region, and whether it has been documented since 2026-09-23.
- Pricing/quota policy beyond rate limits.
- Current state of Lune's Roblox library and `run-in-roblox`.

## Impact on the roadmap design

- **Step 1 (private place + `publish_place`)**: still needed for any cloud-side run; the same sandbox place and version-pinned publish feed Luau Execution too. Publishing with `Saved` mode yields a version number, so record it.
- **New "Run" mode, no client**: add a second path beside Play: publish → create version-pinned task → poll → show logs/return in the Output panel. Needs only the existing API key (add the `universe.place.luau-execution-session:write` scope to the key wizard). No Sober, no tunnel, no probe, no injected `HttpEnabled`/`LoadStringEnabled`.
- **Step 2 (client launch)**: unchanged; the client is required only for true Play.
- **Steps 3-4 (injection, tunnel, probe)**: unchanged for Play. Not used for Run/tests.
- **Step 6 (hot reload via `loadstring`)**: Luau Execution can't replace it (no persistent server, rate limits). Keep `loadstring` over the probe channel for Play.
- **Tests**: offer "Run tests" using a version-pinned task with a Jest-Lua/TestEZ runner; serialize through one queue, cap at ~25 runs/hour, and surface `retry-after` on 429 instead of retrying blindly.
- **Fast pre-check**: before the real Play publish, a task can verify the injected probe compiles; cheap and avoids a wasted Sober launch.
- **Studio MCP**: do not depend on it; consider exposing rbxstudio's own MCP-style tool surface later.

## Sources

- https://create.roblox.com/docs/en-us/cloud/reference/features/luau-execution.md
- https://create.roblox.com/docs/en-us/cloud/guides/instance.md
- https://create.roblox.com/docs/studio/mcp
- https://create.roblox.com/docs/reference/engine/classes/RunService
- https://create.roblox.com/docs/reference/engine/classes/SerializationService
- https://devforum.roblox.com/t/beta-open-cloud-engine-api-for-executing-luau/3172185
- https://devforum.roblox.com/t/4890044
- https://devforum.roblox.com/t/4889837
- https://devforum.roblox.com/t/4221716
- https://devforum.roblox.com/t/assistant-updates-studio-built-in-mcp-server-and-playtest-automation/4474643
- https://devforum.roblox.com/t/4849810
- https://devforum.roblox.com/t/4914875
- https://github.com/AustinBarikdar/LuneTest
- https://github.com/christopher-buss/jest-roblox-cli
- https://github.com/Roblox/studio-rust-mcp-server
- https://github.com/Roblox/place-ci-cd-demo
