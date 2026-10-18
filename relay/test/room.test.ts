import { describe, expect, it } from "vitest";
import {
  CLOCK_SKEW_MS,
  EXPIRY_MS,
  JOURNAL_CAP,
  JOURNAL_TTL_MS,
  MAX_USERS,
  RATE_BURST,
  type Sealed,
} from "../src/contracts.js";
import { Room } from "../src/room.js";

const user = (userId: number): Sealed => ({
  v: 2,
  userId,
  client: "studio",
  ct: "AAEC",
  j: [],
  seen: 0,
});

const tag = (n: number): string => n.toString(16).padStart(16, "0");

// Entry `i` happened at `baseAt + i`.
const withEdits = (userId: number, n: number, baseAt: number): Sealed => ({
  ...user(userId),
  j: Array.from({ length: n }, (_, i) => ({
    at: baseAt + i,
    tag: tag(baseAt + i),
    ct: `ct${i}`,
  })),
});

describe("journal", () => {
  it("records opaque entries", () => {
    const room = new Room();
    room.put(withEdits(7, 2, 5), 1000);
    expect(room.journalSince(4, 1000)).toEqual([
      { userId: 7, at: 5, ct: "ct0" },
      { userId: 7, at: 6, ct: "ct1" },
    ]);
    expect(room.journalSince(6, 1000)).toEqual([]);
  });

  it("does not duplicate an entry re-sent on later heartbeats", () => {
    const room = new Room();
    room.put(withEdits(7, 2, 5), 1000);
    room.put(withEdits(7, 2, 5), 6000);
    room.put(withEdits(7, 2, 5), 11000);
    expect(room.journalSince(0, 11000)).toHaveLength(2);
  });

  it("dedupes per user, not across users", () => {
    const room = new Room();
    room.put(withEdits(1, 1, 5), 1000);
    room.put(withEdits(2, 1, 5), 1000);
    expect(room.journalSince(0, 1000)).toHaveLength(2);
  });

  it("drops entries older than the TTL", () => {
    const room = new Room();
    room.put(withEdits(1, 1, 0), 0);
    const later = JOURNAL_TTL_MS + 1;
    expect(room.journalSince(-1, later)).toEqual([]);
    room.put(withEdits(2, 1, later), later);
    expect(room.journalSince(-1, later).map((e) => e.userId)).toEqual([2]);
  });

  it("keeps only the last JOURNAL_CAP distinct entries", () => {
    const room = new Room();
    room.put(withEdits(1, JOURNAL_CAP + 50, 1), JOURNAL_CAP + 100);
    const entries = room.journalSince(0, JOURNAL_CAP + 100);
    expect(entries).toHaveLength(JOURNAL_CAP);
    expect(entries[entries.length - 1]?.at).toBe(JOURNAL_CAP + 50);
  });
});

describe("Room", () => {
  it("stores users, stamps seen and advances the cursor", () => {
    const room = new Room();
    expect(room.isEmpty()).toBe(true);
    expect(room.put(user(1), 1000)).toBeNull();
    const snap = room.snapshot();
    expect(snap.cursor).toBe(1);
    expect(snap.users).toHaveLength(1);
    expect(snap.users[0]?.seen).toBe(1000);
    expect(room.isEmpty()).toBe(false);
  });

  it("rate limits after the burst and refills over time", () => {
    const room = new Room();
    for (let i = 0; i < RATE_BURST; i++) {
      expect(room.put(user(1), 0)).toBeNull();
    }
    expect(room.put(user(1), 0)).toEqual({ status: 429, error: "rate" });
    expect(room.put(user(2), 0)).toBeNull();
    expect(room.put(user(1), 1000)).toBeNull();
  });

  it("refuses a new user when full but accepts existing ones", () => {
    const room = new Room();
    for (let i = 0; i < MAX_USERS; i++) room.put(user(i), 0);
    expect(room.put(user(999), 0)).toEqual({ status: 403, error: "full" });
    expect(room.put(user(0), 1000)).toBeNull();
  });

  it("expires users not seen within EXPIRY_MS", () => {
    const room = new Room();
    room.put(user(1), 0);
    room.put(user(2), EXPIRY_MS);
    const before = room.cursor;
    expect(room.expire(EXPIRY_MS)).toBe(false);
    expect(room.expire(EXPIRY_MS + 1)).toBe(true);
    expect(room.cursor).toBe(before + 1);
    expect(room.snapshot().users.map((u) => u.userId)).toEqual([2]);
  });

  it("removes users and bumps the cursor only when someone left", () => {
    const room = new Room();
    room.put(user(1), 0);
    const c = room.cursor;
    expect(room.remove(1, "studio")).toBe(true);
    expect(room.cursor).toBe(c + 1);
    expect(room.remove(1, "studio")).toBe(false);
    expect(room.cursor).toBe(c + 1);
  });

  it("keeps one user's states apart and sorts the snapshot", () => {
    const room = new Room();
    room.put({ ...user(2) }, 0);
    room.put({ ...user(1), client: "rbx-native" }, 100);
    room.put(user(1), 0);
    const clients = room.snapshot().users.map((u) => `${u.userId}/${u.client}`);
    expect(clients).toEqual(["1/studio", "1/rbx-native", "2/studio"]);
    expect(room.remove(1, "rbx-native")).toBe(true);
    expect(room.snapshot().users.map((u) => u.client)).toEqual(["studio", "studio"]);
  });

  it("checks clock skew", () => {
    const room = new Room();
    expect(room.checkTime(1, 10_000, 10_000 + CLOCK_SKEW_MS, false)).toBeNull();
    expect(room.checkTime(1, 10_000, 10_001 + CLOCK_SKEW_MS, false)).toEqual({
      status: 401,
      error: "time",
    });
    expect(room.checkTime(1, 10_000 + CLOCK_SKEW_MS + 1, 10_000, false)).toEqual({
      status: 401,
      error: "time",
    });
  });

  it("rejects replayed timestamps only when monotonic", () => {
    const room = new Room();
    expect(room.checkTime(1, 100, 100, true)).toBeNull();
    expect(room.checkTime(1, 100, 100, true)).toEqual({ status: 401, error: "replay" });
    expect(room.checkTime(1, 99, 100, true)).toEqual({ status: 401, error: "replay" });
    expect(room.checkTime(1, 99, 100, false)).toBeNull();
    expect(room.checkTime(1, 101, 101, true)).toBeNull();
    expect(room.checkTime(2, 100, 100, true)).toBeNull();
  });
});
