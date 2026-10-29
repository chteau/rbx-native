// Strict parsing of a posted Sealed body: anything outside the schema or its
// limits is refused, unknown keys are dropped. The ciphertext is opaque.

import {
  MAX_BODY_BYTES,
  MAX_EDITS,
  OPS_BODY_BYTES,
  SCHEMA_VERSION,
  type Client,
  type LoggedOp,
  type OpsFill,
  type Refusal,
  type Sealed,
  type SealedEdit,
  type SealedOps,
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

const LOG_ID = /^[0-9a-f]{1,64}$/;

function client(value: unknown): Client | null {
  return value === "studio" || value === "rbx-native" ? value : null;
}

/** A non-empty base64 chunk, or null. */
function ct(value: unknown): string | null {
  return b64(value) ? value : null;
}

/** `body` as a JSON object, within the op log's size limit. */
function opsObject(body: string): { raw: Record<string, unknown> } | { fault: Refusal } {
  if (encoder.encode(body).length > OPS_BODY_BYTES) {
    return { fault: { status: 413, error: "size" } };
  }
  try {
    const raw: unknown = JSON.parse(body);
    return isObject(raw) ? { raw } : { fault: bad };
  } catch {
    return { fault: bad };
  }
}

/** `body` as `userId`'s chunks for the op log: `cts`, or a lone `ct`. */
export function parseSealedOps(body: string, userId: number): SealedOps | Refusal {
  const parsed = opsObject(body);
  if ("fault" in parsed) return parsed.fault;
  const raw = parsed.raw;
  if (raw.v !== SCHEMA_VERSION) return { status: 400, error: "version" };
  if (!Number.isSafeInteger(userId) || raw.userId !== userId) {
    return { status: 403, error: "user" };
  }
  const who = client(raw.client);
  if (who === null) return bad;
  let cts: string[];
  if (Array.isArray(raw.cts) && raw.ct === undefined && raw.cts.length > 0) {
    cts = [];
    for (const c of raw.cts) {
      const one = ct(c);
      if (one === null) return bad;
      cts.push(one);
    }
  } else if (raw.cts === undefined && raw.ct !== undefined) {
    const one = ct(raw.ct);
    if (one === null) return bad;
    cts = [one];
  } else {
    return bad;
  }
  return { v: SCHEMA_VERSION, userId, client: who, cts };
}

/** `body` as a fill: a named log's entries at consecutive seqs. */
export function parseFill(body: string): OpsFill | Refusal {
  const parsed = opsObject(body);
  if ("fault" in parsed) return parsed.fault;
  const raw = parsed.raw;
  if (raw.v !== SCHEMA_VERSION) return { status: 400, error: "version" };
  const { log, entries: list } = raw;
  if (typeof log !== "string" || !LOG_ID.test(log)) return bad;
  if (!Array.isArray(list) || list.length === 0) return bad;
  const entries: LoggedOp[] = [];
  for (const e of list) {
    if (!isObject(e)) return bad;
    const who = client(e.client);
    const chunk = ct(e.ct);
    if (!Number.isSafeInteger(e.seq) || (e.seq as number) < 1) return bad;
    if (!Number.isSafeInteger(e.userId) || (e.userId as number) < 0) return bad;
    if (who === null || chunk === null) return bad;
    const seq = e.seq as number;
    if (entries.length > 0 && seq !== entries[entries.length - 1]!.seq + 1) return bad;
    entries.push({ seq, userId: e.userId as number, client: who, ct: chunk });
  }
  return { v: SCHEMA_VERSION, log, entries };
}
