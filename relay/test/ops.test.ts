import { describe, expect, it } from "vitest";
import { CLOCK_SKEW_MS, OPS_REPLY_BYTES, type LoggedOp, type Refusal } from "../src/contracts.js";
import { OpLog, type OpStore, type OpsMeta } from "../src/ops.js";

function memoryStore(): OpStore {
  let meta: OpsMeta = { log: "", head: 0, floor: 1, bytes: 0 };
  const rows: LoggedOp[] = [];
  return {
    loadMeta: () => ({ ...meta }),
    saveMeta: (m) => {
      meta = { ...m };
    },
    insert: (e) => void rows.push(e),
    oldest: () => rows[0] ?? null,
    deleteOldest: () => void rows.shift(),
    after: (n) => rows.filter((e) => e.seq > n),
    transaction: (fn) => fn(),
  };
}

const newLog = (cap?: number) => new OpLog(memoryStore(), cap);
const post = (log: OpLog, userId: number, cts: string[], now = 0) =>
  log.append(userId, "studio", cts, now);
const entry = (seq: number, ct: string): LoggedOp => ({ seq, userId: 9, client: "rbx-native", ct });
const seqs = (log: OpLog, since: number) =>
  log.since(since, OPS_REPLY_BYTES).entries.map((e) => e.seq);
const rate: Refusal = { status: 429, error: "rate" };
const wrongLog: Refusal = { status: 409, error: "log" };

describe("OpLog", () => {
  it("takes a signature once", () => {
    const log = newLog();
    expect(log.fresh("abc", 1_000)).toBeNull();
    expect(log.fresh("abc", 2_000)).toEqual({ status: 401, error: "replay" });
    expect(log.fresh("abc", 1_000 + 3 * CLOCK_SKEW_MS)).toBeNull();
  });

  it("numbers seqs from one in append order", () => {
    const log = newLog();
    expect(log.since(0, OPS_REPLY_BYTES).log).toBe("");
    expect(post(log, 1, ["AA"])).toBe(1);
    expect(post(log, 2, ["BB", "CC"])).toBe(3);
    const reply = log.since(0, OPS_REPLY_BYTES);
    expect([reply.head, reply.floor]).toEqual([3, 1]);
    expect(reply.log).toMatch(/^[0-9a-f]{32}$/);
    expect(seqs(log, 0)).toEqual([1, 2, 3]);
    expect(log.since(1, OPS_REPLY_BYTES).entries[0]!.ct).toBe("BB");
    expect(log.since(2, OPS_REPLY_BYTES).entries[0]!.userId).toBe(2);
  });

  it("claims an empty log at the first post", () => {
    const log = newLog();
    expect(post(log, 1, ["AA"], 0)).toBe(1);
    expect(log.log).toHaveLength(32);
  });

  it("moves the floor when trimming at the cap", () => {
    const log = newLog(8);
    for (let i = 0; i < 4; i++) post(log, 1, ["AAAA"]);
    const reply = log.since(0, OPS_REPLY_BYTES);
    expect([reply.head, reply.floor]).toEqual([4, 3]);
    expect(seqs(log, 0)).toEqual([3, 4]);
    expect(log.since(2, OPS_REPLY_BYTES).entries).toHaveLength(2);
  });

  it("empties the log for an entry over the cap", () => {
    const log = newLog(2);
    post(log, 1, ["AAAA"]);
    const reply = log.since(0, OPS_REPLY_BYTES);
    expect([reply.head, reply.floor]).toEqual([1, 2]);
    expect(reply.entries).toEqual([]);
  });

  it("answers nothing for since past head", () => {
    const log = newLog();
    post(log, 1, ["AA"]);
    const reply = log.since(5, OPS_REPLY_BYTES);
    expect(reply.entries).toEqual([]);
    expect(reply.head).toBe(1);
    expect(log.since(1, OPS_REPLY_BYTES).entries).toEqual([]);
  });

  it("stops a reply at its byte cap but never answers empty", () => {
    const log = newLog();
    post(log, 1, ["AAAA", "AAAA", "AAAA"]);
    expect(log.since(0, 8).entries).toHaveLength(2);
    expect(log.since(0, 1).entries).toHaveLength(1);
  });

  it("counts requests, not chunks, against the rate limit", () => {
    const log = newLog();
    const many = Array<string>(500).fill("AA");
    for (let i = 0; i < 64; i++) expect(typeof post(log, 1, many, 1000)).toBe("number");
    expect(post(log, 1, ["AA"], 1000)).toEqual(rate);
    expect(typeof post(log, 2, ["AA"], 1000)).toBe("number");
    expect(typeof post(log, 1, ["AA"], 1100)).toBe("number");
  });

  it("hands an empty relay the old log with its seqs", () => {
    const log = newLog();
    expect(log.fill(1, "abc", [entry(5, "E5"), entry(6, "E6"), entry(7, "E7")], 10)).toBe(7);
    const reply = log.since(0, OPS_REPLY_BYTES);
    expect([reply.head, reply.floor, reply.log]).toEqual([7, 5, "abc"]);
    expect(reply.entries[0]).toEqual(entry(5, "E5"));
    expect(seqs(log, 6)).toEqual([7]);
    expect(post(log, 2, ["NEW"], 20)).toBe(8);
  });

  it("extends the same log with a tail fill and skips what is held", () => {
    const log = newLog();
    log.fill(1, "abc", [entry(1, "E1"), entry(2, "E2")], 0);
    const tail = [entry(2, "E2"), entry(3, "E3"), entry(4, "E4")];
    expect(log.fill(2, "abc", tail, 0)).toBe(4);
    expect(log.fill(3, "abc", tail, 0)).toBe(4);
    expect(seqs(log, 0)).toEqual([1, 2, 3, 4]);
  });

  it("refuses a fill from another log or past a gap", () => {
    const log = newLog();
    log.fill(1, "abc", [entry(1, "E1")], 0);
    expect(log.fill(1, "xyz", [entry(2, "E2")], 0)).toEqual(wrongLog);
    expect(log.fill(1, "abc", [entry(3, "E3")], 0)).toEqual(wrongLog);
    const claimed = newLog();
    post(claimed, 1, ["AA"]);
    expect(claimed.fill(1, "abc", [entry(1, "E1")], 0)).toEqual(wrongLog);
  });

  it("resumes from its store", () => {
    const store = memoryStore();
    const first = new OpLog(store);
    post(first, 1, ["AA", "BB"]);
    const again = new OpLog(store);
    expect([again.head, again.log]).toEqual([2, first.log]);
    expect(again.since(1, OPS_REPLY_BYTES).entries).toHaveLength(1);
  });
});
