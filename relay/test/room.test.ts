import { describe, expect, it } from "vitest";
import {
  CLOCK_SKEW_MS,
  EXPIRY_MS,
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
