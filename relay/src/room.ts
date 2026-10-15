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
  type JournalEntry,
  type Refusal,
  type Snapshot,
  type UserState,
} from "./contracts.js";

interface Bucket {
  tokens: number;
  at: number;
}

export class Room {
  key: string | null = null;
  cursor = 0;
  private users = new Map<number, UserState>();
  private buckets = new Map<number, Bucket>();
  private lastTs = new Map<number, number>();
  private journal: JournalEntry[] = [];

  put(state: UserState, now: number): Refusal | null {
    if (!this.take(state.userId, now)) return { status: 429, error: "rate" };
    if (!this.users.has(state.userId) && this.users.size >= MAX_USERS) {
      return { status: 403, error: "full" };
    }
    this.users.set(state.userId, { ...state, seen: now });
    this.cursor++;
    // Clients re-send their recent edits on every heartbeat, so the journal
    // keys on the client's own edit time and skips an edit it already holds;
    // otherwise re-sends would flood the cap and evict real history in
    // seconds. The value is never stored.
    for (const edit of state.edits) {
      const at = edit.at;
      const seen = this.journal.some(
        (e) =>
          e.userId === state.userId &&
          e.at === at &&
          e.path === edit.path &&
          e.property === edit.property,
      );
      if (seen) continue;
      this.journal.push({
        userId: state.userId,
        name: state.name,
        path: edit.path,
        property: edit.property,
        at,
      });
    }
    this.journal = this.journal.filter((e) => e.at >= now - JOURNAL_TTL_MS);
    if (this.journal.length > JOURNAL_CAP) {
      this.journal = this.journal.slice(-JOURNAL_CAP);
    }
    return null;
  }

  journalSince(since: number, now: number): JournalEntry[] {
    return this.journal
      .filter((e) => e.at > since && e.at >= now - JOURNAL_TTL_MS)
      .sort((a, b) => a.at - b.at);
  }

  remove(userId: number): boolean {
    const had = this.users.delete(userId);
    this.buckets.delete(userId);
    this.lastTs.delete(userId);
    if (had) this.cursor++;
    return had;
  }

  expire(now: number): boolean {
    let dropped = false;
    for (const [id, user] of this.users) {
      if (user.seen < now - EXPIRY_MS) {
        this.users.delete(id);
        this.buckets.delete(id);
        this.lastTs.delete(id);
        dropped = true;
      }
    }
    if (dropped) this.cursor++;
    return dropped;
  }

  snapshot(): Snapshot {
    return { cursor: this.cursor, users: [...this.users.values()] };
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
