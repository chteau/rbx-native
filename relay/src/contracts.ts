// Shared types and module signatures for the presence relay. The message
// shape matches crates/rbx_presence/src/schema.rs and
// plugins/team-presence/src/Schema.luau; the golden files in
// plugins/team-presence/fixtures/ pin all three.

export const SCHEMA_VERSION = 2;
export const MAX_BODY_BYTES = 16 * 1024;
/** Op log routes accept bodies up to this size. */
export const OPS_BODY_BYTES = 1024 * 1024;
/** The op log keeps at most this many bytes of `ct`, trimming the oldest. */
export const OPS_LOG_BYTES = 64 * 1024 * 1024;
/** One op log reply carries at most this many bytes of `ct`. */
export const OPS_REPLY_BYTES = 512 * 1024;
/** Op log token bucket per user, one token per request. */
export const OPS_RATE_BURST = 64;
export const OPS_RATE_PER_SECOND = 32;
export const MAX_SELECTION = 100;
export const MAX_EDITS = 20;
/** Longest path/property/value/name/script string, in UTF-8 bytes. */
export const MAX_TEXT = 512;
export const MAX_USERS = 32;
export const EXPIRY_MS = 15_000;
/** A long-poll answers after this long even when nothing changed. */
export const POLL_TIMEOUT_MS = 20_000;
/** A signed request's time may differ from the relay's clock by this much. */
export const CLOCK_SKEW_MS = 60_000;
/** Token bucket per user: burst size and refill per second. */
export const RATE_BURST = 8;
export const RATE_PER_SECOND = 4;
/** Most journal entries kept per room. */
export const JOURNAL_CAP = 256;
/** Journal entries older than this are dropped. */
export const JOURNAL_TTL_MS = 12 * 60 * 60 * 1000;

/** One sealed journal entry as the relay replays it; it cannot read `ct`. */
export interface JournalEntry {
  userId: number;
  at: number;
  ct: string;
}

export const USER_HEADER = "X-Presence-User";
export const TIME_HEADER = "X-Presence-Time";
export const SIG_HEADER = "X-Presence-Sig";
export const KEY_HEADER = "X-Presence-Key";

export type Client = "studio" | "rbx-native";

/** One edit sealed for the journal; `tag` is what the relay dedupes on. */
export interface SealedEdit {
  at: number;
  tag: string;
  ct: string;
}

/** A user's state, encrypted; the relay never reads `ct`. */
export interface Sealed {
  v: number;
  userId: number;
  client: Client;
  ct: string;
  j: SealedEdit[];
  seen: number;
}

/** `POST /ops`: one or more chunks from one sender for the op log. */
export interface SealedOps {
  v: number;
  userId: number;
  client: Client;
  cts: string[];
}

/** One entry of the op log, as `GET /ops` returns it. */
export interface LoggedOp {
  seq: number;
  userId: number;
  client: Client;
  ct: string;
}

/** `GET /ops?since=N`: entries after `since`, oldest first. */
export interface OpsReply {
  head: number;
  floor: number;
  log: string;
  entries: LoggedOp[];
}

/** `POST /ops/fill`: entries of log `log` a peer read earlier. */
export interface OpsFill {
  v: number;
  log: string;
  entries: LoggedOp[];
}

/** `POST /ops` answers the seq of the last chunk and the log it went to. */
export interface OpsPosted {
  seq: number;
  log: string;
}

export interface SealedSnapshot {
  cursor: number;
  users: Sealed[];
}

export interface SealedJournalEntry {
  userId: number;
  at: number;
  ct: string;
}

/** Why a request was refused; `status` is the HTTP status to answer with. */
export interface Refusal {
  status: 400 | 401 | 403 | 409 | 413 | 429;
  /** Short machine word: "key" (room unknown, resend with the key),
   * "sig", "time", "replay", "size", "rate", "full", "schema", "log". */
  error: string;
}

// ---- src/auth.ts (WebCrypto, async) ----
// deriveKey(secret: string, placeId: number | bigint): Promise<string>
//   hex(HMAC-SHA-256(utf8(secret), "rbx-presence/key/v1/" + placeId))
// roomId(key: string): Promise<string>
//   first 32 hex of hex(SHA-256("rbx-presence/room/v1/" + key))
// sha256Hex(data: string | Uint8Array): Promise<string>
// canonical(method, path, userId: number, ts: number, body: string | Uint8Array): Promise<string>
//   `${method}\n${path}\n${userId}\n${ts}\n${await sha256Hex(body)}`
// sign(key: string, canonical: string): Promise<string>  hex(HMAC(utf8(key), canonical))
// verify(key: string, canonical: string, sig: string): Promise<boolean>  constant-time compare

// ---- src/validate.ts ----
// parseState(body: string, userId: number): UserState | Refusal
//   JSON parse, v === SCHEMA_VERSION, userId matches the header, field types,
//   limits (MAX_SELECTION, MAX_EDITS, MAX_TEXT bytes, camera exactly 12
//   finite numbers, color /^#[0-9a-f]{6}$/), unknown keys dropped; returns a
//   fresh object with seen = 0.

// parseSealedOps(body: string, userId: number): SealedOps | Refusal
//   body over OPS_BODY_BYTES 413 size; v !== SCHEMA_VERSION 400 version;
//   userId mismatch 403 user; `cts` non-empty array or a lone `ct`, each
//   non-empty base64; anything else 400 schema.
// parseFill(body: string): OpsFill | Refusal
//   log 1..64 lowercase hex; entries non-empty, seq integer >= 1 and
//   consecutive, each with userId, client and ct.

// ---- src/ops.ts (pure, no Workers APIs; `now` always passed in) ----
// interface OpStore: synchronous storage (SQLite in the Durable Object, a
//   map in tests) for the entries and the log's meta (id, head, floor, bytes).
// class OpLog {
//   head, log                         // log is "" until the first entry lands
//   fresh(sig, now): Refusal | null   // replay guard, in memory
//   append(userId, client, cts, now): number | Refusal
//       // rate limit (one token per request), mints the log id on the first
//       // post (no claim grace: the log is persistent), trims the oldest
//       // entries past OPS_LOG_BYTES
//   fill(userId, log, entries, now): number | Refusal
//       // whole log onto an empty one, or entries past head of the same log
//       // (held ones skipped); else 409 log
//   since(n, maxBytes): OpsReply      // at least one entry when any is due
// }

// ---- src/room.ts (pure, no Workers APIs; `now` always passed in) ----
// class Room {
//   key: string | null              // learnt from the first KEY_HEADER that matches the room id
//   cursor: number
//   put(state: UserState, now: number): Refusal | null   // rate limit, MAX_USERS, sets seen = now, cursor++
//   remove(userId: number): boolean                        // cursor++ when someone left
//   expire(now: number): boolean                           // drops seen < now - EXPIRY_MS; cursor++ if any
//   snapshot(): Snapshot
//   checkTime(userId: number, ts: number, now: number, monotonic: boolean): Refusal | null
//       // |now - ts| <= CLOCK_SKEW_MS; when monotonic, ts must exceed the
//       // last ts accepted from that user (replay guard for POST/DELETE)
//   isEmpty(): boolean
// }

// ---- src/durable.ts ----
// export class RoomObject implements DurableObject, one per room id. Holds a
// Room, the open WebSockets (userId per socket) and the pending long-polls.
// fetch(request): routes by URL pathname suffix after /v1/rooms/<room>:
//   POST /state, DELETE /state, GET /poll?since=N, GET /ws (upgrade),
//   GET /journal?since=N → { entries: JournalEntry[] } (value-less edits
//   recorded since the unix-ms time N, oldest first),
//   POST /ops → OpsPosted, GET /ops?since=N → OpsReply (long-poll while
//   head === N), POST /ops/fill → OpsPosted. The op log lives in the
//   Durable Object's SQLite storage; ops routes skip the increasing-time
//   check and use OpLog.fresh(sig) instead.
// Authentication (key learning, signature, time) is done here, before
// any route, from the four headers. Every change resolves waiting polls and
// sends the snapshot to every socket. An alarm every 5 s while the room is
// non-empty runs expire(). Payloads are never logged.

// ---- src/index.ts ----
// default export { fetch(request, env) }: GET /health → "ok";
// /v1/rooms/<32 lowercase hex>/... → env.ROOMS.get(idFromName(room)).fetch;
// anything else 404. CORS is not needed (no browser client).

export interface Env {
  ROOMS: DurableObjectNamespace;
}
