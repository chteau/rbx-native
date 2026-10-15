// Request authentication for the presence relay: key derivation, room ids
// and HMAC signatures. HMAC keys are the UTF-8 bytes of the hex strings,
// never hex-decoded, matching the plugin and rbx-native.

const encoder = new TextEncoder();

function toBytes(data: string | Uint8Array): Uint8Array {
  return typeof data === "string" ? encoder.encode(data) : data;
}

function toHex(buffer: ArrayBuffer): string {
  let out = "";
  for (const byte of new Uint8Array(buffer)) {
    out += byte.toString(16).padStart(2, "0");
  }
  return out;
}

async function hmacHex(key: string, message: string): Promise<string> {
  // Built from raw digests because WebCrypto refuses zero-length HMAC keys.
  const block = 64;
  let keyBytes = encoder.encode(key);
  if (keyBytes.length > block) {
    keyBytes = new Uint8Array(await crypto.subtle.digest("SHA-256", keyBytes));
  }
  const inner = new Uint8Array(block);
  const outer = new Uint8Array(block);
  inner.fill(0x36);
  outer.fill(0x5c);
  keyBytes.forEach((byte, i) => {
    inner[i] ^= byte;
    outer[i] ^= byte;
  });
  const msg = encoder.encode(message);
  const innerInput = new Uint8Array(block + msg.length);
  innerInput.set(inner);
  innerInput.set(msg, block);
  const innerHash = new Uint8Array(await crypto.subtle.digest("SHA-256", innerInput));
  const outerInput = new Uint8Array(block + innerHash.length);
  outerInput.set(outer);
  outerInput.set(innerHash, block);
  return toHex(await crypto.subtle.digest("SHA-256", outerInput));
}

export async function sha256Hex(data: string | Uint8Array): Promise<string> {
  return toHex(await crypto.subtle.digest("SHA-256", toBytes(data)));
}

export function deriveKey(secret: string, placeId: number | bigint): Promise<string> {
  return hmacHex(secret, `rbx-presence/key/v1/${placeId}`);
}

export async function roomId(key: string): Promise<string> {
  return (await sha256Hex(`rbx-presence/room/v1/${key}`)).slice(0, 32);
}

export async function canonical(
  method: string,
  path: string,
  userId: number,
  ts: number,
  body: string | Uint8Array,
): Promise<string> {
  return `${method}\n${path}\n${userId}\n${ts}\n${await sha256Hex(body)}`;
}

export function sign(key: string, canonicalText: string): Promise<string> {
  return hmacHex(key, canonicalText);
}

export async function verify(key: string, canonicalText: string, sig: string): Promise<boolean> {
  const expected = encoder.encode(await sign(key, canonicalText));
  const given = encoder.encode(sig);
  let diff = expected.length ^ given.length;
  for (let i = 0; i < expected.length; i++) {
    diff |= expected[i] ^ (given[i] ?? 0);
  }
  return diff === 0;
}
