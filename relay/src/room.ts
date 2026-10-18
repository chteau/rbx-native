// Pure room state: users, change cursor, rate limits and replay guard. All
// time comes in as an argument so the logic is testable without a clock.

import {
  CLOCK_SKEW_MS,
  EXPIRY_MS,
  JOURNAL_CAP,
  JOURNAL_TTL_MS,
  MAX_USERS,
  RATE_BURST,
  RATE_PER_SECOND,
  type Client,
  type JournalEntry,
  type Refusal,
  type Sealed,
  type SealedSnapshot,
} from "./contracts.js";

interface Held {
  tag: string;
  entry: JournalEntry;
}

interface Bucket {
  tokens: number;
  at: number;
}

export class Room {
  key: string | null = null;
  cursor = 0;
  /** Keyed by {@link senderKey}: one user can be in Studio and rbx-native at once. */
  private users = new Map<string, Sealed>();
  private buckets = new Map<number, Bucket>();
  private lastTs = new Map<number, number>();
  private journal: Held[] = [];

  put(state: Sealed, now: number): Refusal | null {
    if (!this.take(state.userId, now)) return { status: 429, error: "rate" };
    const key = senderKey(state.userId, state.client);
    if (!this.users.has(key) && this.users.size >= MAX_USERS) {
      return { status: 403, error: "full" };
    }
    this.users.set(key, { ...state, seen: now });
    this.cursor++;
    // Clients re-send their recent edits on every heartbeat, so the journal
    // dedupes on (userId, tag); otherwise re-sends would flood the cap and
    // evict real history in seconds.
    for (const edit of state.j) {
      const held = this.journal.some(
        (h) => h.entry.userId === state.userId && h.tag === edit.tag,
      );
      if (held) continue;
      this.journal.push({
        tag: edit.tag,
        entry: { userId: state.userId, at: edit.at, ct: edit.ct },
      });
    }
    this.journal = this.journal.filter((h) => h.entry.at >= now - JOURNAL_TTL_MS);
    if (this.journal.length > JOURNAL_CAP) {
      this.journal = this.journal.slice(-JOURNAL_CAP);
    }
    return null;
  }

  journalSince(since: number, now: number): JournalEntry[] {
    return this.journal
      .map((h) => h.entry)
      .filter((e) => e.at > since && e.at >= now - JOURNAL_TTL_MS)
      .sort((a, b) => a.at - b.at);
  }

  /** Drops one editor's state; the user's rate and replay state go with their last one. */
  remove(userId: number, client: Client): boolean {
    const had = this.users.delete(senderKey(userId, client));
    if (!this.hasUser(userId)) {
      this.buckets.delete(userId);
      this.lastTs.delete(userId);
    }
    if (had) this.cursor++;
    return had;
  }

  expire(now: number): boolean {
    let dropped = false;
    for (const [key, user] of this.users) {
      if (user.seen < now - EXPIRY_MS) {
        this.users.delete(key);
        dropped = true;
        if (!this.hasUser(user.userId)) {
          this.buckets.delete(user.userId);
          this.lastTs.delete(user.userId);
        }
      }
    }
    if (dropped) this.cursor++;
    return dropped;
  }

  snapshot(): SealedSnapshot {
    const users = [...this.users.values()].sort(
      (a, b) =>
        a.userId - b.userId ||
        Number(a.client === "rbx-native") - Number(b.client === "rbx-native"),
    );
    return { cursor: this.cursor, users };
  }

  checkTime(
    userId: number,
    ts: number,
    now: number,
    monotonic: boolean,
  ): Refusal | null {
    if (!Number.isFinite(ts) || Math.abs(now - ts) > CLOCK_SKEW_MS) {
      return { status: 401, error: "time" };
    }
    if (monotonic) {
      const last = this.lastTs.get(userId);
      if (last !== undefined && ts <= last) {
        return { status: 401, error: "replay" };
      }
      this.lastTs.set(userId, ts);
    }
    return null;
  }

  private hasUser(userId: number): boolean {
    for (const user of this.users.values()) {
      if (user.userId === userId) return true;
    }
    return false;
  }

  isEmpty(): boolean {
    return this.users.size === 0;
  }

  private take(userId: number, now: number): boolean {
    const b = this.buckets.get(userId) ?? { tokens: RATE_BURST, at: now };
    const elapsed = Math.max(0, now - b.at) / 1000;
    b.tokens = Math.min(RATE_BURST, b.tokens + elapsed * RATE_PER_SECOND);
    b.at = now;
    const ok = b.tokens >= 1;
    if (ok) b.tokens -= 1;
    this.buckets.set(userId, b);
    return ok;
  }
}

/** One sender: a user in one editor. */
export function senderKey(userId: number, client: Client): string {
  return `${userId}/${client}`;
}
