# Presence relay

A small Cloudflare Worker that lets Roblox Studio (through the Team Presence
plugin) and rbx-native show each other's cameras, selections and open scripts.

**Optional.** Rooms work with no relay at all: by default (Auto) the first
rbx-native user in the place runs this same relay (`rbx_presence::host`, a
Rust port held to the same tests, `crates/rbx_presence/tests/relay_e2e.rs`)
behind a Cloudflare quick tunnel, no account needed. Deploy this Worker only
if a team wants an always-on relay of its own.

## What it keeps

- Presence lives in the memory of one Durable Object per room. It is never
  written to storage.
- The co-editing op log is the one thing stored (see "Op log" below), as
  ciphertext only.
- A user disappears 15 seconds after their last update (clients send a
  heartbeat every 5 s) or when they leave.
- Nothing is logged: no payloads, no room ids. Cloudflare observability is
  switched off in `wrangler.toml`.

## Endpoints

All under `<relay>/v1/rooms/<room>`; `GET /health` answers `ok`.

| Request | Purpose |
| --- | --- |
| `POST /state` | Publish your `UserState` JSON; answers `{"cursor":n}` |
| `DELETE /state` | Leave the room |
| `GET /poll?since=<cursor>` | Long-poll (up to 20 s); answers a snapshot `{cursor, users}` |
| `POST /ops` | Append sealed chunks to the op log; answers `{"seq":n,"log":"<id>"}` |
| `GET /ops?since=<seq>` | Op log entries after `seq`; answers at once unless the log's head is `seq`, then long-polls (up to 20 s) |
| `POST /ops/fill` | Hand the log a peer read earlier to a relay that lacks it |
| `GET /ws` | WebSocket: send `UserState` texts, receive snapshot texts |

## Op log

Co-editing in Studio-only teams needs no rbx-native host: the Worker keeps
the same numbered op log as `rbx_presence::host`. Each room's log lives in its
Durable Object's SQLite storage and holds ciphertext only, so the relay cannot
read it. It is capped at 64 MiB, trimming the oldest entries first, and it
survives everyone leaving. The first post on an empty log names it with a
random id; `fill` lets a peer restore a log the relay lost.

Each logged chunk is one row written. On Cloudflare's free plan, SQLite-backed
Durable Objects allow 100,000 rows written per day and 5 GB stored per account
([pricing](https://developers.cloudflare.com/durable-objects/platform/pricing/)),
so a busy team can reach the daily write limit; past it, posts fail until the
limit resets at 00:00 UTC.

## Auth

The key is `hex(HMAC-SHA256(secret, "rbx-presence/key/v1/" + placeId))` and the
room id is the first 32 hex digits of `SHA256("rbx-presence/room/v1/" + key)`.
Each request carries `X-Presence-User`, `X-Presence-Time` (ms) and
`X-Presence-Sig`, the HMAC with the key over
`METHOD \n path+query \n userId \n time \n hex(SHA256(body))`.
The first request of a room also carries `X-Presence-Key`, which the relay
checks against the room id and remembers; an unknown room answers
`401 {"error":"key"}`, meaning "resend with the key".

## Limits

16 KiB body (1 MiB on the ops routes), 100 selected paths, 20 edits, 512-byte strings, 32 users per
room, clock skew of 60 s, POST/DELETE times must increase, and 8 updates
burst with 4 per second sustained per user. Ops routes skip the
increasing-time rule (a signature can be used once instead) and allow a burst
of 64 requests with 32 per second sustained per user.

## Run locally

```
npm install
npx wrangler dev
npm test
```

## Deploy

```
npx wrangler login
npx wrangler deploy
```

Wrangler prints the Worker URL (`https://rbx-presence-relay.<account>.workers.dev`).
Paste it as the relay URL into the plugin's widget and into rbx-native.

## Self-hosting

Deploying to your own Cloudflare account is all it takes: the relay holds no
secrets, since each room's key comes from clients. Keep the Durable Object
binding `ROOMS` and class `RoomObject` from `wrangler.toml`.
