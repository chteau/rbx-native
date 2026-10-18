import { describe, expect, it } from "vitest";
import { MAX_BODY_BYTES, type Refusal } from "../src/contracts.js";
import { parseSealed } from "../src/validate.js";

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
