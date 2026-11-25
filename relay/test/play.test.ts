import { describe, expect, it } from "vitest";
import { PLAY_RATE_BURST, PlayHub, parsePid } from "../src/play.js";

const A = "0000000000000001";
const B = "0000000000000002";
const C = "0000000000000003";

const message = (to: string, body: number[]): Uint8Array =>
  Uint8Array.from([...Buffer.from(to, "hex"), ...body]);

describe("play route", () => {
  it("takes a 16 hex pid, never 0", () => {
    expect(parsePid("00000000000000AB")).toBe("00000000000000ab");
    expect(parsePid("0000000000000000")).toBeNull();
    expect(parsePid("12")).toBeNull();
    expect(parsePid(null)).toBeNull();
  });

  it("forwards to the end named, re-addressed from the sender", () => {
    const hub = new PlayHub<string>();
    hub.join(A, "a", 0);
    hub.join(B, "b", 0);
    hub.join(C, "c", 0);
    const out = hub.route(A, message(B, [9, 9]), 0);
    expect(out).toHaveLength(1);
    expect(out[0][0]).toBe("b");
    expect([...out[0][1]]).toEqual([...Buffer.from(A, "hex"), 9, 9]);
  });

  it("sends 0 to every other end", () => {
    const hub = new PlayHub<string>();
    for (const [pid, s] of [[A, "a"], [B, "b"], [C, "c"]]) hub.join(pid, s, 0);
    const to = hub.route(B, message("0000000000000000", [1]), 0).map(([s]) => s);
    expect(to.sort()).toEqual(["a", "c"]);
  });

  it("replaces a reconnecting end and forgets a closed one", () => {
    const hub = new PlayHub<string>();
    hub.join(A, "a1", 0);
    expect(hub.join(A, "a2", 0)).toBe("a1");
    hub.join(B, "b", 0);
    hub.leave(A, "a1");
    expect(hub.route(B, message(A, [1]), 0).map(([s]) => s)).toEqual(["a2"]);
    hub.leave(A, "a2");
    expect(hub.route(B, message(A, [1]), 0)).toEqual([]);
  });

  it("drops what's over the rate, and refills", () => {
    const hub = new PlayHub<string>();
    hub.join(A, "a", 0);
    hub.join(B, "b", 0);
    let sent = 0;
    for (let i = 0; i < PLAY_RATE_BURST + 10; i++) sent += hub.route(A, message(B, [i]), 0).length;
    expect(sent).toBe(PLAY_RATE_BURST);
    expect(hub.route(A, message(B, [1]), 1000)).toHaveLength(1);
  });
});
