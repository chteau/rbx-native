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

- **Confidentiality (OPEN — see "Encryption" below).** Signing proves
  authenticity, not secrecy. Presence payloads are currently sent in the clear.
  On the Worker relay the operator can read every payload; on a **public ntfy
  instance the topic is world-readable to anyone who learns or guesses the topic
  string**. The unguessable topic is obscurity, not encryption. Leaked today:
  display names, selection instance paths, the open script's path, the *names
  and short display values* of recent property edits, and camera pose. This is
  the one unresolved item in the security review and the encryption design below
  closes it.
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

### Encryption (DESIGNED — not yet on the wire)

The confidentiality gap above is closed by encrypting the payload with a key
only holders of the place secret can derive. The agreed construction, to be
implemented identically in Rust, TypeScript (WebCrypto) and Luau and pinned by a
new golden fixture, is:

- `encKey = HKDF-SHA256(ikm = utf8(secret), salt = "", info = "rbx-presence/enc/v1/" + placeId)` → 32 bytes.
  HKDF is `HMAC-SHA256`-based, so it reuses primitives all three already have.
- AEAD over the `UserState` JSON (and over each value-less journal entry):
  a 96-/192-bit random nonce prefixed to the ciphertext, authenticated tag
  appended. The existing per-request HMAC signature stays as the relay-layer
  room-auth (it is what lets the Worker route and rate-limit without decrypting),
  so this is encryption *in addition to* signing, not instead of it.
- The ntfy envelope's `d` becomes base64(nonce ‖ ciphertext ‖ tag) instead of
  cleartext JSON; the Worker body likewise. Peers/relay without the secret see
  only opaque bytes.

**Why it is not landed in this pass:** AEAD must be byte-for-byte identical in
hand-written Luau (the plugin has no crypto library beyond the SHA-256/HMAC it
ships) and verified against *real* Roblox Studio over a live relay before it can
replace the working plaintext wire. Landing it in Rust alone would break
interop. It is therefore specified here and left for a coordinated, live-tested
change. **Do not flip the wire to encrypted until all three implementations pass
a shared golden fixture and a live Studio round trip.**

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
