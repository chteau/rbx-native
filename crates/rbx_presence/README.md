# rbx_presence — Team Presence over a relay

Team Presence lets rbx-native users and Roblox Studio collaborators (running the
`plugins/team-presence` plugin) see where each other are — camera, selection,
the open script, recent property edits — without speaking Roblox's own Team
Create protocol. Everything here uses **official Roblox APIs only**: Studio's
side is a plugin talking to a relay over `HttpService`; this crate is the
editor's side of the same relay.

Two transports, both official:

- **Worker** — the signed-WebSocket relay in `relay/` (a Cloudflare Worker). It
  keeps the live room in memory and fans out snapshots.
- **ntfy** — a plain [ntfy](https://ntfy.sh) pub/sub server. It runs none of our
  code; each client publishes its signed presence to an unguessable topic and
  reconstructs the room from the message stream. Peers trust each other by HMAC,
  never the server. The community fallback is a public ntfy instance.

The message shape is one JSON schema shared by all three implementations
(`schema.rs`, `relay/src/contracts.ts`, `plugins/team-presence/src/Schema.luau`),
pinned by the golden fixtures in `plugins/team-presence/fixtures/`.

## Authentication

Everything is derived from a per-place **secret** (`secret.rs`), which lives
inside the place as a string attribute on `ServerStorage/RbxNativePresence`.
`ServerStorage` is never sent to game clients, and Team Create replicates it to
every collaborator, so the people who can edit the place are the ones who can
join the room.

- `key  = hex(HMAC-SHA256(secret, "rbx-presence/key/v1/" + placeId))`
- `room = first 32 hex of SHA-256("rbx-presence/room/v1/" + key)`
- every request is signed: `sig = hex(HMAC(key, METHOD\npath\nuserId\nts\nSHA256(body)))`

The relay (and every ntfy peer) verifies the signature against the key it
derived from the same secret. The room id is derived from the secret too, so
the ntfy **topic name is unguessable** without the secret.

### What is already enforced

- **Replay** — the Worker DO requires each user's signed timestamp to strictly
  increase for mutating requests (POST/DELETE), checked *after* the signature so
  an unsigned request cannot advance the guard (`room.ts::checkTime`).
- **Clock skew** — a signed timestamp more than 60 s from the relay's / peer's
  clock is rejected (`CLOCK_SKEW_MS`, both transports).
- **Rate + user caps (Worker)** — a per-user token bucket (burst 8, 4/s) and a
  32-user room cap (`room.ts`). ntfy relies on the ntfy server's own limits.
- **Message size** — the relay refuses bodies over 16 KiB (`MAX_BODY_BYTES`).
  **ntfy.sh additionally caps a message at ~4 KiB**, below our 16 KiB, so the
  ntfy sender shrinks each envelope to fit `NTFY_MAX_BODY` (4096) before signing
  (`ntfy::fit_encode` / `Ntfy.fitEncode`): it sheds the oldest edits, then
  selection paths, then the camera, then the open script, keeping identity. A
  large selection therefore degrades gracefully on the community transport
  rather than silently failing to publish.

## Threat model and residual risks

**Trust boundary:** the place secret *is* the capability. Anyone who holds it is
treated as a collaborator. The relay operator is **not** trusted for
confidentiality or integrity of identity — see below.

- **Confidentiality (closed by encryption, see below).** Every payload —
  presence states, journal entries, ntfy messages, co-editing ops — is
  sealed with a key derived from the place secret. The relay operator and
  anyone reading a public ntfy topic see only the signature envelope
  (user id, editor kind, timestamps, sizes, edit times and opaque journal
  tags) and ciphertext. What still shows: who is in the room and when, and
  how much they send.
- **Impersonation (residual, inherent to a shared secret).** The key is shared
  per place, so any collaborator who holds the secret can sign presence or edits
  as *any* `userId` — the relay checks that the header, signature and
  `state.userId` agree, but cannot verify the human behind them. Within official
  APIs there is no offline way to bind a message to a specific Roblox identity
  (that would need per-user OAuth tokens the relay could verify). **Residual
  risk:** a malicious collaborator who already has edit access can forge another
  collaborator's presence and misattribute edits. This cannot be eliminated
  without a trusted, per-user-authenticated relay; it is bounded by the fact
  that forging requires edit access in the first place.
- **Secret exposure via the place file.** The secret is stored in
  `ServerStorage`. That keeps it from game clients and shares it with
  collaborators, but **anyone with a copy of the place file can read it** — most
  importantly an **uncopylocked place gives the secret away**, letting a stranger
  join the room (and, once encryption lands, decrypt it). Treat presence on an
  uncopylocked place as public. Rotate the secret (below) after any place leak.

### Secret rotation

There is no automated handover; rotation is a clean break that fails stale
clients closed:

1. The owner mints a new secret ("Create presence key" again writes a fresh 64-
   hex secret over the old one) and **Saves or Publishes** so Studio
   collaborators receive it.
2. The new secret derives a different key and therefore a **different room id and
   ntfy topic**. Clients still holding the old secret connect to the old room and
   simply find themselves alone — they never see, and can never decrypt, the new
   room. That is the desired fail-closed behaviour.
3. Everyone reconnects (toggle Share my presence off/on, or it reconnects on the
   next place reload) and lands in the new room.

Because the derivation strings are versioned (`.../key/v1/...`,
`.../room/v1/...`), a future wire-level key-version bump can be added without
colliding with existing rooms.

### Encryption (wire v2)

- `encKey = HKDF-SHA256(ikm = utf8(secret), salt = "", info = "rbx-presence/enc/v1/" + placeId)`
  → 32 bytes (`crypt::enc_key`, `Crypto.encKey`). The relay key is offered
  to the relay in a header, so it is never the encryption key.
- ChaCha20-Poly1305 (RFC 8439) with a random 96-bit nonce; the wire carries
  base64(nonce ‖ ciphertext ‖ tag). The AAD binds each payload to its room,
  sender and editor (`crypt::state_aad`, `journal_aad`, `ops_aad`), so a
  relay cannot move a ciphertext to another user.
- A state crosses the relay as `schema::Sealed` (`v`, `userId`, `client`,
  `ct`, plus one sealed entry per edit for the journal, deduped on an HMAC
  tag the relay cannot invert); the ntfy envelope's `d` is that JSON. The
  per-request HMAC signature stays as room authentication, so the relay
  routes, rate-limits and journals without decrypting.
- Rust uses RustCrypto (`chacha20poly1305`, `hkdf`); the plugin carries its
  own pure-Luau ChaCha20-Poly1305 (`Crypto.luau`, `bit32` only). Both, and
  the Worker's validation, are pinned by
  `plugins/team-presence/fixtures/crypto_vectors.json` (RFC 8439 §2.8.2 and
  RFC 5869 case 3 among them). The Luau module was run against every vector
  inside Roblox Studio 0.743 itself, not only in the test runner; all
  passed, and a 16 KiB seal+open took about 17 ms there.
- The plugin's nonces come from `HttpService:GenerateGUID` (v4, random)
  mixed with a `Random` stream: plugins have no CSPRNG API.
- Versioning: bodies carry `v: 2`; a relay answers a v1 (cleartext) body
  with `400 {"error":"version"}` and both clients turn that into "update
  the plugin, rbx-native and the relay together", so an old client fails
  loudly rather than being ignored.

## Hosted rooms (the default): no account, no server

With no relay named in the place, the first rbx-native user to share
presence hosts the room: `host` runs the Worker's routes on a loopback
port, the editor's bundled `cloudflared` opens a Cloudflare quick tunnel
to it (no account), and `announce` posts the tunnel's address, signed with
the place key, to the room's announce topic on ntfy.sh. Everyone else, the
Studio plugin included, reads the newest genuine announcement and talks to
that relay; a host that leaves posts a goodbye. ntfy carries only these few
messages a day, far under its 250/day anonymous limit (live presence over
public ntfy.sh is not workable; see `relay.rs`).

- **What the tunnel exposes:** only the presence routes for this one room;
  every request is HMAC-signed with the place key, the same checks as the
  Worker (`tests/relay_e2e.rs` runs the same tests against both). The
  tunnel URL is public but useless without the key.
- **Announcements:** signed over room, URL and time; stale (over 12 h),
  future, unsigned, and non-HTTPS addresses are ignored. Only key holders
  can redirect a room, and they are collaborators already.
- **DNS:** the host announces only after public DNS (asked over
  DNS-over-HTTPS) resolves the new hostname, so no collaborator's resolver
  caches an early "no such host".
- **Availability:** the room exists while some rbx-native user hosts it.
  Studio users see each other through Studio's own Team Create, so this is
  exactly when presence across the two editors matters. Two hosts at once
  settle on the newer announcement within a minute. Quick tunnels have no
  uptime promise.

## Offline catch-up — what each transport can reconstruct

On reconnect (or opening a place), presence can summarise "while you were away".
What is reconstructable depends entirely on the transport, and in every case is
limited to **property edits that collaborators' running plugins reported** — a
period when nobody ran the plugin reconstructs nothing, and nothing here is a
substitute for the place's own save history.

- **ntfy** retains published messages for ~12 h. On reconnect the client polls
  the retention window and rebuilds a **value-less journal** (who, which instance
  paths, how many distinct properties, how many edits) from the `edits` carried
  in those presence messages. Values are dropped on the way in.
- **Worker** keeps only the live room in memory — no history — **except** for the
  bounded, value-less journal the Durable Object now keeps: up to
  `JOURNAL_CAP` (256) entries for `JOURNAL_TTL_MS` (12 h), served on a signed
  `GET /v1/rooms/<room>/journal?since=N`. Each entry is `{userId, name, path,
  property, at}` — **never the edited value**.
- **Neither transport reconstructs instance add/remove** — the presence schema
  carries only property edits, not structural changes — and neither carries edit
  *values*. For a true structural+value diff, presence persists the last-seen
  place snapshot per place id and opens the existing Argon **Diff** window
  between that snapshot and the current place ("View changes"). That DOM diff is
  transport-independent and reconstructs real changes; see the module docs for
  its status.

`catchup::summarize` turns a value-less journal (from either source) into a
per-author summary (distinct instances, distinct properties, edit count), sorted
by edit count. It is pinned by `plugins/team-presence/fixtures/catchup.json`.
