# Presence relay

A small Cloudflare Worker that lets Roblox Studio (through the Team Presence
plugin) and rbx-native show each other's cameras, selections and open scripts.

**Optional.** Rooms work with no relay at all: by default (Auto) the first
rbx-native user in the place runs this same relay (`rbx_presence::host`, a
Rust port held to the same tests, `crates/rbx_presence/tests/relay_e2e.rs`)
behind a Cloudflare quick tunnel, no account needed. Deploy this Worker only
if a team wants an always-on relay of its own.

## What it keeps

- Presence lives in the memory of one Durable Object per room. Nothing is
  written to storage.
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
| `GET /ws` | WebSocket: send `UserState` texts, receive snapshot texts |

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

16 KiB body, 100 selected paths, 20 edits, 512-byte strings, 32 users per
room, clock skew of 60 s, POST/DELETE times must increase, and 8 updates
burst with 4 per second sustained per user.

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
