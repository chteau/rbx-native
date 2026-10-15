import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { canonical, deriveKey, roomId, sha256Hex, sign, verify } from "../src/auth";

interface Vectors {
  sha256: { input: string; sha256: string }[];
  hmac: { key: string; message: string; hmac: string }[];
  requests: {
    secret: string;
    placeId: number;
    key: string;
    room: string;
    method: string;
    path: string;
    userId: number;
    ts: number;
    body: string;
    bodySha256: string;
    canonical: string;
    sig: string;
  }[];
}

const vectors: Vectors = JSON.parse(
  readFileSync(resolve(process.cwd(), "../plugins/team-presence/fixtures/auth_vectors.json"), "utf8"),
);

function flipLastDigit(hex: string): string {
  const last = hex[hex.length - 1] === "0" ? "1" : "0";
  return hex.slice(0, -1) + last;
}

describe("sha256Hex", () => {
  it.each(vectors.sha256.map((v) => [v.input.length, v] as const))("input of %i chars", async (_, v) => {
    expect(await sha256Hex(v.input)).toBe(v.sha256);
    expect(await sha256Hex(new TextEncoder().encode(v.input))).toBe(v.sha256);
  });
});

describe("hmac", () => {
  it.each(vectors.hmac.map((v) => [v.key.length, v] as const))("key of %i chars", async (_, v) => {
    expect(await sign(v.key, v.message)).toBe(v.hmac);
  });
});

describe("requests", () => {
  it.each(vectors.requests.map((r) => [`${r.method} ${r.path}`, r] as const))("%s", async (_, r) => {
    expect(await deriveKey(r.secret, r.placeId)).toBe(r.key);
    expect(await deriveKey(r.secret, BigInt(r.placeId))).toBe(r.key);
    expect(await roomId(r.key)).toBe(r.room);
    expect(await sha256Hex(r.body)).toBe(r.bodySha256);
    const text = await canonical(r.method, r.path, r.userId, r.ts, r.body);
    expect(text).toBe(r.canonical);
    expect(await sign(r.key, text)).toBe(r.sig);
    expect(await verify(r.key, text, r.sig)).toBe(true);
    expect(await verify(r.key, text, flipLastDigit(r.sig))).toBe(false);
    expect(await verify(r.key, text, r.sig.slice(1))).toBe(false);
  });
});
