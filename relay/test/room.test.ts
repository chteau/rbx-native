import { describe, expect, it } from "vitest";
import {
  CLOCK_SKEW_MS,
  EXPIRY_MS,
  JOURNAL_CAP,
  JOURNAL_TTL_MS,
  MAX_USERS,
  RATE_BURST,
  type UserState,
} from "../src/contracts.js";
import { Room } from "../src/room.js";

const user = (userId: number): UserState => ({
  v: 1,
  userId,
  name: `u${userId}`,
  color: "#e8590c",
  client: "studio",
  selection: [],
  edits: [],
  seen: 0,
});

// Edits carry the client's own edit time; entry `i` happened at `baseAt + i`.
const withEdits = (userId: number, n: number, baseAt: number): UserState => ({
  ...user(userId),
  edits: Array.from({ length: n }, (_, i) => ({
    path: `Workspace.P${i}`,
    property: "Size",
    value: "secret",
    at: baseAt + i,
  })),
});

describe("journal", () => {
  it("records value-less entries, keyed on the client's edit time", () => {
    const room = new Room();
    room.put(withEdits(7, 2, 5), 1000);
    const entries = room.journalSince(4, 1000);
    expect(entries).toEqual([
      { userId: 7, name: "u7", path: "Workspace.P0", property: "Size", at: 5 },
      { userId: 7, name: "u7", path: "Workspace.P1", property: "Size", at: 6 },
    ]);
    for (const e of entries) expect("value" in e).toBe(false);
    // `since` is exclusive on the edit time.
    expect(room.journalSince(6, 1000).map((e) => e.at)).toEqual([]);
  });

  it("does not duplicate an edit re-sent on later heartbeats", () => {
    const room = new Room();
    room.put(withEdits(7, 2, 5), 1000);
    room.put(withEdits(7, 2, 5), 6000);
    room.put(withEdits(7, 2, 5), 11000);
    expect(room.journalSince(0, 11000)).toHaveLength(2);
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
    // One put with more distinct edits than the cap; MAX_EDITS bounds a real
    // message, but the cap must hold regardless.
    room.put(withEdits(1, JOURNAL_CAP + 50, 1), JOURNAL_CAP + 100);
    const entries = room.journalSince(0, JOURNAL_CAP + 100);
    expect(entries).toHaveLength(JOURNAL_CAP);
    // The newest edits are the ones kept.
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
    expect(room.remove(1)).toBe(true);
    expect(room.cursor).toBe(c + 1);
    expect(room.remove(1)).toBe(false);
    expect(room.cursor).toBe(c + 1);
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
