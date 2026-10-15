// Strict parsing of a posted UserState: anything outside the schema or its
// limits is refused, unknown keys are dropped.

import {
  MAX_BODY_BYTES,
  MAX_EDITS,
  MAX_SELECTION,
  MAX_TEXT,
  SCHEMA_VERSION,
  type Edit,
  type Refusal,
  type UserState,
} from "./contracts.js";

const encoder = new TextEncoder();
const COLOR = /^#[0-9a-f]{6}$/;

const bad: Refusal = { status: 400, error: "schema" };

function text(value: unknown): value is string {
  return typeof value === "string" && encoder.encode(value).length <= MAX_TEXT;
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function parseEdit(value: unknown): Edit | null {
  if (!isObject(value)) return null;
  const { path, property, value: v, at } = value;
  if (!text(path) || !text(property) || !text(v)) return null;
  if (typeof at !== "number" || !Number.isFinite(at)) return null;
  return { path, property, value: v, at };
}

export function parseState(body: string, userId: number): UserState | Refusal {
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
  if (raw.v !== SCHEMA_VERSION) return bad;
  if (raw.userId !== userId || !Number.isSafeInteger(userId)) return bad;
  if (!text(raw.name)) return bad;
  if (typeof raw.color !== "string" || !COLOR.test(raw.color)) return bad;
  if (raw.client !== "studio" && raw.client !== "rbx-native") return bad;

  const state: UserState = {
    v: SCHEMA_VERSION,
    userId,
    name: raw.name,
    color: raw.color,
    client: raw.client,
    selection: [],
    edits: [],
    seen: 0,
  };

  if (raw.camera !== undefined) {
    const cam = raw.camera;
    if (
      !Array.isArray(cam) ||
      cam.length !== 12 ||
      !cam.every((n) => typeof n === "number" && Number.isFinite(n))
    ) {
      return bad;
    }
    state.camera = cam as number[];
  }

  if (raw.selection !== undefined) {
    const sel = raw.selection;
    if (!Array.isArray(sel) || sel.length > MAX_SELECTION) return bad;
    if (!sel.every(text)) return bad;
    state.selection = sel as string[];
  }

  if (raw.script !== undefined) {
    if (!text(raw.script)) return bad;
    state.script = raw.script;
  }

  if (raw.edits !== undefined) {
    const edits = raw.edits;
    if (!Array.isArray(edits) || edits.length > MAX_EDITS) return bad;
    const parsed: Edit[] = [];
    for (const e of edits) {
      const edit = parseEdit(e);
      if (edit === null) return bad;
      parsed.push(edit);
    }
    state.edits = parsed;
  }

  return state;
}
