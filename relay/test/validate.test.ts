import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { MAX_BODY_BYTES, type Refusal } from "../src/contracts.js";
import { parseState } from "../src/validate.js";

const fixture = (name: string): string =>
  readFileSync(`../plugins/team-presence/fixtures/${name}`, "utf8");

const studio = fixture("state_studio.json");
const base = (): Record<string, unknown> => JSON.parse(studio);
const parse = (o: Record<string, unknown>) =>
  parseState(JSON.stringify(o), 1234567);

function refusal(r: unknown): Refusal {
  expect(r).toHaveProperty("status");
  return r as Refusal;
}

describe("parseState", () => {
  it("accepts the golden studio state with seen reset", () => {
    const s = parseState(studio, 1234567);
    expect(s).toMatchObject({ userId: 1234567, client: "studio", seen: 0 });
    expect((s as { camera: number[] }).camera).toHaveLength(12);
  });

  it("accepts the golden native state", () => {
    const s = parseState(fixture("state_native.json"), 42);
    expect(s).toMatchObject({ name: "Ben", client: "rbx-native", edits: [] });
  });

  it("refuses a bad version", () => {
    expect(refusal(parse({ ...base(), v: 2 })).status).toBe(400);
  });

  it("refuses a userId mismatch", () => {
    expect(refusal(parseState(studio, 7)).error).toBe("schema");
  });

  it("refuses 101 selections but takes 100", () => {
    const path = (n: number) => `A.${n}`;
    const make = (n: number) =>
      parse({ ...base(), selection: Array.from({ length: n }, (_, i) => path(i)) });
    expect(refusal(make(101)).status).toBe(400);
    expect(make(100)).toHaveProperty("selection");
  });

  it("refuses 21 edits", () => {
    const edit = { path: "A", property: "B", value: "C", at: 1 };
    expect(
      refusal(parse({ ...base(), edits: Array.from({ length: 21 }, () => edit) }))
        .status,
    ).toBe(400);
  });

  it("refuses a 513-byte path", () => {
    expect(refusal(parse({ ...base(), selection: ["a".repeat(513)] })).status).toBe(400);
    expect(parse({ ...base(), selection: ["a".repeat(512)] })).toHaveProperty("name");
  });

  it("counts bytes rather than characters", () => {
    expect(refusal(parse({ ...base(), script: "é".repeat(257) })).status).toBe(400);
  });

  it("refuses an 11-number camera", () => {
    expect(
      refusal(parse({ ...base(), camera: Array.from({ length: 11 }, () => 0) })).status,
    ).toBe(400);
  });

  it("refuses a bad colour", () => {
    expect(refusal(parse({ ...base(), color: "#E8590C" })).status).toBe(400);
    expect(refusal(parse({ ...base(), color: "red" })).status).toBe(400);
  });

  it("refuses a body over the limit with 413", () => {
    const r = refusal(parseState(" ".repeat(MAX_BODY_BYTES + 1), 1));
    expect(r).toEqual({ status: 413, error: "size" });
  });

  it("refuses invalid JSON and drops unknown keys", () => {
    expect(refusal(parseState("{", 1)).status).toBe(400);
    const s = parse({ ...base(), extra: 1 });
    expect(s).not.toHaveProperty("extra");
  });
});
