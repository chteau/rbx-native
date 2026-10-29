// The co-editing op log: an append-only, numbered list of sealed entries
// that readers fetch by `since`. The relay never opens `ct`. Pure logic over
// a synchronous store (SQLite in the Durable Object); no Workers APIs.
//
// A log is named by a random id minted when its first entry lands. Unlike
// the rbx-native host, the Worker's log is persistent, so the first post on
// an empty log claims it at once (no grace for a peer to fill it first).

import {
  CLOCK_SKEW_MS,
  OPS_LOG_BYTES,
  OPS_RATE_BURST,
  OPS_RATE_PER_SECOND,
  type Client,
  type LoggedOp,
  type OpsReply,
  type Refusal,
} from "./contracts.js";

export interface OpsMeta {
  /** Empty until the first entry lands. */
  log: string;
  head: number;
  floor: number;
  /** Total length of the held `ct`. */
  bytes: number;
}

/** Synchronous storage for the entries and the meta. */
export interface OpStore {
  loadMeta(): OpsMeta;
  saveMeta(meta: OpsMeta): void;
  insert(entry: LoggedOp): void;
  /** The held entry with the lowest seq. */
  oldest(): LoggedOp | null;
  deleteOldest(): void;
  /** Entries with seq > `n`, oldest first (lazily, so a reader can stop). */
  after(n: number): Iterable<LoggedOp>;
  /** Runs `fn` atomically. */
  transaction<T>(fn: () => T): T;
}

const REFUSED_LOG: Refusal = { status: 409, error: "log" };

interface Bucket {
  tokens: number;
  at: number;
}

function newLogId(): string {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}

export class OpLog {
  private meta: OpsMeta;
  private readonly buckets = new Map<number, Bucket>();
  /** Signatures of posts taken within the clock-skew window, and when. */
  private readonly seen = new Map<string, number>();

  constructor(
    private readonly store: OpStore,
    private readonly cap = OPS_LOG_BYTES,
  ) {
    this.meta = store.loadMeta();
  }

  get head(): number {
    return this.meta.head;
  }

  /** The log's id; empty while it holds nothing. */
  get log(): string {
    return this.meta.log;
  }

  private take(userId: number, now: number): Refusal | null {
    const bucket = this.buckets.get(userId) ?? { tokens: OPS_RATE_BURST, at: now };
    this.buckets.set(userId, bucket);
    const elapsed = Math.max(0, now - bucket.at) / 1000;
    bucket.tokens = Math.min(OPS_RATE_BURST, bucket.tokens + elapsed * OPS_RATE_PER_SECOND);
    bucket.at = now;
    if (bucket.tokens < 1) return { status: 429, error: "rate" };
    bucket.tokens -= 1;
    return null;
  }

  /**
   * Refuses a post whose signature was already taken: a captured request
   * sent again. Older ones are refused on their timestamp already, so only
   * the clock-skew window is remembered.
   */
  fresh(sig: string, now: number): Refusal | null {
    const window = 2 * CLOCK_SKEW_MS;
    for (const [s, at] of this.seen) {
      if (now - at > window) this.seen.delete(s);
    }
    if (this.seen.has(sig)) return { status: 401, error: "replay" };
    this.seen.set(sig, now);
    return null;
  }

  private push(entry: LoggedOp): void {
    this.store.insert(entry);
    this.meta.head = entry.seq;
    this.meta.bytes += entry.ct.length;
  }

  private trim(): void {
    while (this.meta.bytes > this.cap) {
      const old = this.store.oldest();
      if (old === null) break;
      this.store.deleteOldest();
      this.meta.bytes -= old.ct.length;
    }
    this.meta.floor = this.store.oldest()?.seq ?? this.meta.head + 1;
    this.store.saveMeta(this.meta);
  }

  /** Logs `cts` from one sender under consecutive seqs; answers the last. */
  append(userId: number, client: Client, cts: string[], now: number): number | Refusal {
    const fault = this.take(userId, now);
    if (fault) return fault;
    return this.commit(() => {
      if (this.meta.log === "") this.meta.log = newLogId();
      for (const ct of cts) this.push({ seq: this.meta.head + 1, userId, client, ct });
    });
  }

  /**
   * Takes entries of log `log` a peer read earlier (consecutive seqs,
   * checked by the caller): the whole log when this one holds nothing yet,
   * otherwise those past the head of the same log. Entries already held are
   * skipped. Answers the new head; 409 "log" for another log or a gap.
   */
  fill(userId: number, log: string, entries: LoggedOp[], now: number): number | Refusal {
    const first = entries[0]?.seq;
    if (first === undefined) return this.meta.head;
    const empty = this.meta.log === "" && this.meta.head === 0;
    if (!empty && (this.meta.log !== log || first > this.meta.head + 1)) return REFUSED_LOG;
    const fault = this.take(userId, now);
    if (fault) return fault;
    return this.commit(() => {
      if (empty) {
        this.meta.log = log;
        this.meta.head = first - 1;
        this.meta.floor = first;
      }
      for (const entry of entries) {
        if (entry.seq > this.meta.head) this.push(entry);
      }
    });
  }

  private commit(change: () => void): number {
    this.store.transaction(() => {
      change();
      this.trim();
    });
    return this.meta.head;
  }

  /**
   * Entries after `n`, oldest first, holding at most `maxBytes` of `ct`
   * (always at least one entry when any is due, so a reader cannot stall).
   */
  since(n: number, maxBytes: number): OpsReply {
    const entries: LoggedOp[] = [];
    let used = 0;
    for (const entry of this.store.after(n)) {
      if (entries.length > 0 && used + entry.ct.length > maxBytes) break;
      used += entry.ct.length;
      entries.push(entry);
    }
    return { head: this.meta.head, floor: this.meta.floor, log: this.meta.log, entries };
  }
}
