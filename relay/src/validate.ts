// Strict parsing of a posted Sealed body: anything outside the schema or its
// limits is refused, unknown keys are dropped. The ciphertext is opaque.

import {
  MAX_BODY_BYTES,
  MAX_EDITS,
  SCHEMA_VERSION,
  type Refusal,
  type Sealed,
  type SealedEdit,
} from "./contracts.js";

const encoder = new TextEncoder();
const BASE64 = /^[A-Za-z0-9+/=]+$/;
const TAG = /^[0-9a-f]{16}$/;

const bad: Refusal = { status: 400, error: "schema" };

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function b64(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= MAX_BODY_BYTES && BASE64.test(value);
}

function parseEdit(value: unknown): SealedEdit | null {
  if (!isObject(value)) return null;
  const { at, tag, ct } = value;
  if (typeof at !== "number" || !Number.isFinite(at) || at < 0) return null;
  if (typeof tag !== "string" || !TAG.test(tag)) return null;
  if (!b64(ct)) return null;
  return { at, tag, ct };
}

export function parseSealed(body: string, userId: number): Sealed | Refusal {
  if (encoder.encode(body).length > MAX_BODY_BYTES) {
    return { status: 413, error: "size" };
  }
  let raw: unknown;
  try {
    raw = JSON.parse(body);
  } catch {
    return bad;
  }
  if (!isObject(raw)) return bad;
  if (raw.v !== SCHEMA_VERSION) return { status: 400, error: "version" };
  if (!Number.isSafeInteger(userId) || raw.userId !== userId) {
    return { status: 403, error: "user" };
  }
  if (raw.client !== "studio" && raw.client !== "rbx-native") return bad;
  if (!b64(raw.ct)) return bad;

  const j: SealedEdit[] = [];
  if (raw.j !== undefined) {
    if (!Array.isArray(raw.j) || raw.j.length > MAX_EDITS) return bad;
    for (const e of raw.j) {
      const edit = parseEdit(e);
      if (edit === null) return bad;
      j.push(edit);
    }
  }

  return { v: SCHEMA_VERSION, userId, client: raw.client, ct: raw.ct, j, seen: 0 };
}
