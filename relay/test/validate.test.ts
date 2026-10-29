import { describe, expect, it } from "vitest";
import { MAX_BODY_BYTES, OPS_BODY_BYTES, type Refusal } from "../src/contracts.js";
import { parseFill, parseSealed, parseSealedOps } from "../src/validate.js";

const base = (): Record<string, unknown> => ({
  v: 2,
  userId: 1234567,
  client: "studio",
  ct: "AAECAwQFBgcICQoL+/8=",
  j: [{ at: 5, tag: "0123456789abcdef", ct: "AAEC" }],
  seen: 99,
});
const parse = (o: Record<string, unknown>) =>
  parseSealed(JSON.stringify(o), 1234567);

function refusal(r: unknown): Refusal {
  expect(r).toHaveProperty("status");
  return r as Refusal;
}

describe("parseSealed", () => {
  it("accepts a sealed body with seen reset", () => {
    expect(parse(base())).toEqual({
      v: 2,
      userId: 1234567,
      client: "studio",
      ct: "AAECAwQFBgcICQoL+/8=",
      j: [{ at: 5, tag: "0123456789abcdef", ct: "AAEC" }],
      seen: 0,
    });
  });

  it("accepts a missing j", () => {
    const o = base();
    delete o.j;
    expect(parse(o)).toMatchObject({ j: [] });
  });

  it("refuses another version with 'version'", () => {
    expect(refusal(parse({ ...base(), v: 1 }))).toEqual({ status: 400, error: "version" });
  });

  it("refuses a userId mismatch with 403", () => {
    expect(refusal(parseSealed(JSON.stringify(base()), 7)).status).toBe(403);
  });

  it("refuses a bad client or ciphertext", () => {
    expect(refusal(parse({ ...base(), client: "x" })).error).toBe("schema");
    expect(refusal(parse({ ...base(), ct: "" })).error).toBe("schema");
    expect(refusal(parse({ ...base(), ct: "a b" })).error).toBe("schema");
    expect(refusal(parse({ ...base(), ct: "A".repeat(16 * 1024 + 1) })).error).toBe("size");
  });

  it("limits the journal to 20 well-formed entries", () => {
    const e = { at: 1, tag: "0123456789abcdef", ct: "AAEC" };
    expect(parse({ ...base(), j: Array.from({ length: 20 }, () => e) })).toHaveProperty("j");
    expect(refusal(parse({ ...base(), j: Array.from({ length: 21 }, () => e) })).status).toBe(400);
    expect(refusal(parse({ ...base(), j: [{ ...e, tag: "ABCDEF0123456789" }] })).status).toBe(400);
    expect(refusal(parse({ ...base(), j: [{ ...e, at: -1 }] })).status).toBe(400);
    expect(refusal(parse({ ...base(), j: [{ ...e, ct: "!" }] })).status).toBe(400);
  });

  it("refuses a body over the limit with 413", () => {
    const r = refusal(parseSealed(" ".repeat(MAX_BODY_BYTES + 1), 1));
    expect(r).toEqual({ status: 413, error: "size" });
  });

  it("refuses invalid JSON and drops unknown keys", () => {
    expect(refusal(parseSealed("{", 1)).status).toBe(400);
    expect(parse({ ...base(), extra: 1 })).not.toHaveProperty("extra");
  });
});

const OP = '{"v":2,"userId":5,"client":"rbx-native","ct":"AAAA","x":1}';
const OPS = '{"v":2,"userId":5,"client":"studio","cts":["AAAA","BBBB"]}';
const schema: Refusal = { status: 400, error: "schema" };

describe("parseSealedOps", () => {
  it("parses one chunk or many", () => {
    expect(parseSealedOps(OP, 5)).toEqual({ v: 2, userId: 5, client: "rbx-native", cts: ["AAAA"] });
    expect(parseSealedOps(OPS, 5)).toMatchObject({ cts: ["AAAA", "BBBB"] });
  });

  it("refuses bad bodies", () => {
    const bad = (body: string) => parseSealedOps(body, 5);
    expect(bad(OP.replace("5", "6"))).toEqual({ status: 403, error: "user" });
    expect(parseSealedOps(OP, 6)).toEqual({ status: 403, error: "user" });
    expect(bad(OP.replace('"v":2', '"v":1'))).toEqual({ status: 400, error: "version" });
    expect(bad(OP.replace("rbx-native", "x"))).toEqual(schema);
    expect(bad(OP.replace("AAAA", ""))).toEqual(schema);
    expect(bad(OP.replace("AAAA", "A!"))).toEqual(schema);
    expect(bad(OPS.replace("BBBB", "B!"))).toEqual(schema);
    expect(bad(OPS.replace('["AAAA","BBBB"]', "[]"))).toEqual(schema);
    expect(bad("[]")).toEqual(schema);
    expect(bad("nope")).toEqual(schema);
    expect(bad(OP + " ".repeat(OPS_BODY_BYTES))).toEqual({ status: 413, error: "size" });
  });
});

const FILL =
  '{"v":2,"log":"ab12","entries":[{"seq":4,"userId":5,"client":"studio","ct":"AAAA"},{"seq":5,"userId":6,"client":"rbx-native","ct":"BBBB"}]}';

describe("parseFill", () => {
  it("parses and keeps each sender", () => {
    const fill = parseFill(FILL);
    expect(fill).toMatchObject({ log: "ab12" });
    expect((fill as { entries: unknown[] }).entries[1]).toEqual({
      seq: 5,
      userId: 6,
      client: "rbx-native",
      ct: "BBBB",
    });
  });

  it("refuses bad fills", () => {
    expect(parseFill(FILL.replace('"seq":5', '"seq":6'))).toEqual(schema);
    expect(parseFill(FILL.replace("ab12", "AB12"))).toEqual(schema);
    expect(parseFill(FILL.replace('"seq":4', '"seq":0'))).toEqual(schema);
    expect(parseFill(FILL.replace("BBBB", "B B"))).toEqual(schema);
    expect(parseFill(FILL.replace('"v":2', '"v":1'))).toEqual({ status: 400, error: "version" });
    expect(parseFill('{"v":2,"log":"ab","entries":[]}')).toEqual(schema);
  });
});
