// Pure room state: users, change cursor, rate limits and replay guard. All
// time comes in as an argument so the logic is testable without a clock.

import {
  CLOCK_SKEW_MS,
  EXPIRY_MS,
  MAX_USERS,
  RATE_BURST,
  RATE_PER_SECOND,
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

  put(state: UserState, now: number): Refusal | null {
    if (!this.take(state.userId, now)) return { status: 429, error: "rate" };
    if (!this.users.has(state.userId) && this.users.size >= MAX_USERS) {
      return { status: 403, error: "full" };
    }
    this.users.set(state.userId, { ...state, seen: now });
    this.cursor++;
    return null;
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
