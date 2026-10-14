# HttpService, loadstring and observability for the in-game probe (steps 3, 4, 6)

**TL;DR**
- Outbound HTTP from a *published* server is request/response only: `GetAsync`/`PostAsync`/`RequestAsync`. The roadmap's "HTTP-only, no WebSocket" holds for the running game. [Verified]
- `HttpService:CreateWebStreamClient` (SSE, chunked, **WebSocket**) exists but is **Studio-only**. It cannot run in the published sandbox server, so it does not overturn the design. [Verified]
- Budget the probe for ~500 generic HTTP requests/min per server. A long-poll costs one request per poll, so use polls of ~25 s and share the budget with uplink posts. [Verified limit / Inferred sizing]
- `loadstring` still exists and is server-only, behind `ServerScriptService.LoadStringEnabled` (default false, serialised in the place). Injecting it into the in-memory DOM before publish is the supported route. [Verified]
- Server observability is good (`LogService`, `ScriptContext.Error`). The client must mirror via a `RemoteEvent` (~500 calls/s per client limit). [Verified]

## Possible
- **HttpService in published places.** Docs describe requests from "experience servers", gated by `HttpService.HttpEnabled` (serialised: can_load/can_save true). Only *unpublished* experiences need the command-bar toggle, so a published place carries the flag from the saved DOM, which step 3 relies on. [Verified] https://create.roblox.com/docs/reference/engine/classes/HttpService
- **Generic limit.** The in-game HTTP page contrasts Open Cloud calls (2500/min per server) with "the overall limit of 500 HTTP requests per minute enforced on all other requests"; exceeding stalls ~30 s or errors. [Verified] https://create.roblox.com/docs/cloud-services/http-service
- **HTTP/2.** HttpService uses HTTP/2 automatically when offered (lowercase header names), so a tunnel speaking h2 helps latency. [Verified, same page]
- **Studio-only streaming.** `CreateWebStreamClient(WebStreamClientType, requestOptions)` handles SSE, raw chunked and WebSocket (`Send` only for WebSocket); max 4 clients. Useful only for a Studio-run fallback. [Verified] https://create.roblox.com/docs/reference/engine/classes/WebStreamClient
- **loadstring hot reload.** Setting `ServerScriptService.LoadStringEnabled = true` in the pre-publish DOM enables server `loadstring`. [Verified] https://create.roblox.com/docs/reference/engine/classes/ServerScriptService
- **Server logs.** `LogService.MessageOut(message, messageType, context)` fires for output in that VM; `GetLogHistory()` returns the backlog; `ScriptContext.Error` gives uncaught errors with stack. [Verified] https://create.roblox.com/docs/reference/engine/classes/LogService
- **Client relay.** A LocalScript can connect client-side `LogService.MessageOut` and `FireServer` batches. `RemoteEvent`/`UnreliableRemoteEvent`: ~500 requests/s per client, shared among remotes of the same type. [Verified] https://create.roblox.com/docs/reference/engine/classes/RemoteEvent
- **Alternate channels.** `MessagingService` and `MemoryStore` are server-to-server inside Roblox; the editor can only reach them through Open Cloud (e.g. `PublishUniverseMessage`, which is listed as callable from HttpService). A fallback, not a primary channel. [Inferred]

## Not possible
- **WebSocket/SSE from a published server.** `CreateWebStreamClient` is "available in Studio only"; remove references before publishing. [Verified] https://create.roblox.com/docs/reference/engine/classes/HttpService
- **localhost from a published server.** Docs say plugins (Studio) may reach localhost/127.0.0.1; servers use public endpoints. [Verified for plugins, Inferred for servers]
- **Open Cloud-section restrictions** (HTTPS only; only `x-api-key` + `content-type` headers; ports below 1024 except 80/443 and 1194 blocked with 403/ERR_ACCESS_DENIED). The page lists them under Open Cloud limitations; whether the generic path enforces the same is not stated, so assume HTTPS on 443. [Verified quote, Inferred scope]
- **Inbound push** from the editor to the game: no inbound connections, so the game must poll. [Inferred]
- **Client `loadstring`.** The property lives in ServerScriptService (NotReplicated), so only the server can enable it. [Verified/Inferred]

## Unknown / needs a live test
- Real max body size and per-request timeout of `RequestAsync` (not found in docs; community says ~1 MB and tens of seconds). [Reported, unconfirmed]
- Whether tunnel domains (`trycloudflare.com`, Tailscale Funnel `ts.net`, others) are accepted by Roblox egress. No official blocklist found; roblox.com is known to be refused. [Inferred, test first]
- Whether `http://` or a non-443 port works on the generic path.
- Whether the 500/min budget counts failed or held requests (assumed once per request). [Inferred]
- That `HttpEnabled`/`LoadStringEnabled` survive `publish_place` with an injected DOM.
- Latency from publish to a joinable updated server.
- Client `LogService.MessageOut` coverage of engine messages.

## Client-side code delivery
- Options: (a) fixed LocalScript that exchanges state and commands through the server over RemoteEvents, with no pushed code; (b) a pure-Luau interpreter module (vLua/Fione class, not individually verified) in ReplicatedStorage fed source over a RemoteEvent; (c) `require(assetId)` with an upload per change, too slow. [Inferred]
- Recommend (a); add (b) only if client hot code is wanted.

## Probe protocol (draft)
Server `Script` reads `BASE` (https tunnel URL), `TOKEN` (pre-shared secret) and `SESSION` (GUID) from attributes set in the injected DOM.

Budget (500/min; keep ≤ 400/min):
- Uplink `POST /v1/ingest`: coalesced diffs every 200 ms only when dirty, capped at ~150/min (window grows to 1 s under load). Split batches over ~200 KB. [Inferred values]
- Downlink `GET /v1/poll?since=<seq>`: editor holds up to ~25 s, returns 204 if empty or a JSON command array. One poll in flight, ≤ ~3/min idle, re-issued immediately after a reply.
- On 429/stall: exponential backoff 2, 4, 8 s (docs advice) and send a full snapshot instead of queued diffs.

Messages (JSON via `JSONEncode`):
- `hello` {session, placeVersion, jobId, serverTime} → {ack, lastSeq, config}
- `diff` {seq, t, ops:[{op: add|set|del|move, path, class?, props?}]}, driven by `DescendantAdded/Removing` and `GetPropertyChangedSignal` for displayed properties only; full `snapshot` every ~30 s as resync anchor.
- `log` {entries:[{t, level, ctx: server|client:<userId>, text}]} from `LogService.MessageOut`; `err` {msg, stack, script} from `ScriptContext.Error`.
- `stats` every ~2 s: `Stats.HeartbeatTimeMs`, memory, player count.
- Commands: `eval` {id, chunk, src}, `set` {path, prop, value}, `invoke`, `reload`, `shutdown`; monotone `id`, acked and deduped so retries are safe. Results return as `{id, ok, result|err}` in the next ingest.
- Ordering: `seq` per direction; editor drops `seq` ≤ last applied and requests a `snapshot` on a gap.

Client mirror: LocalScript batches state and output for ~200 ms into one `FireServer` table (far below 500/s), size-capped, drop-oldest; the server tags it by player and forwards in the next ingest.

Security: token checked by the editor server on every request; injected scripts exist only in the published copy. `loadstring` + HTTP is remote code execution by design. [Inferred]

## Impact on the roadmap design
- **Step 3:** same method; add BASE/TOKEN/SESSION attributes, ingest Script, LogService/ScriptContext hooks; note `LoadStringEnabled` is server-only so the client mirror cannot `loadstring`.
- **Step 4:** keep "HTTP-only", refined to "request/response in the published server; WebSocket/SSE are Studio-only". Require an HTTPS tunnel on 443, budget ≤ 400 requests/min, long-poll ~25 s. A Studio-run variant could use `CreateWebStreamClient` for a real WebSocket.
- **Step 6:** keep `loadstring` for the server; client uses command-driven mirroring, interpreter optional. One publish per session still applies.
- **New:** confirm Roblox egress accepts the chosen tunnel host before relying on it.

## Sources
- https://create.roblox.com/docs/cloud-services/http-service
- https://create.roblox.com/docs/reference/engine/classes/HttpService
- https://create.roblox.com/docs/reference/engine/classes/WebStreamClient
- https://create.roblox.com/docs/reference/engine/classes/ServerScriptService
- https://create.roblox.com/docs/reference/engine/classes/LogService
- https://create.roblox.com/docs/reference/engine/classes/RemoteEvent
- https://create.roblox.com/docs/reference/engine/globals/LuaGlobals
