// The play route (`GET /v1/rooms/<room>/play?pid=<16 hex>`): Local Run
// play between editors, as crates/rbx_presence/src/host/play.rs serves it.
// Each binary message is `[to: u64 BE][sealed]`; it goes to the end `to`
// names (0: every other end) as `[from: u64 BE][sealed]`. The relay keeps
// and reads nothing. Pure: time comes in as an argument.

/** Messages a second an end may send, and how many at once. */
export const PLAY_RATE_PER_SECOND = 600;
export const PLAY_RATE_BURST = 1200;
/** The largest message: a 192 KiB chunk, its header, the seal, the address. */
export const PLAY_MESSAGE_BYTES = 192 * 1024 + 64;

const PID = /^[0-9a-f]{16}$/;
const EVERYONE = "0000000000000000";

/** The end a `pid` query names, or null (absent, malformed, or 0). */
export function parsePid(query: string | null): string | null {
  const pid = query?.toLowerCase() ?? "";
  return PID.test(pid) && pid !== EVERYONE ? pid : null;
}

function hex(bytes: Uint8Array): string {
  return [...bytes].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function unhex(pid: string): Uint8Array {
  return Uint8Array.from(pid.match(/../g) ?? [], (b) => parseInt(b, 16));
}

interface End<S> {
  socket: S;
  tokens: number;
  at: number;
}

export class PlayHub<S> {
  private ends = new Map<string, End<S>>();

  /** Opens `pid`'s end; a reconnecting end replaces its old socket, returned to close. */
  join(pid: string, socket: S, now: number): S | undefined {
    const old = this.ends.get(pid)?.socket;
    this.ends.set(pid, { socket, tokens: PLAY_RATE_BURST, at: now });
    return old;
  }

  /** Closes `pid`'s end, if `socket` is still the one open. */
  leave(pid: string, socket: S): void {
    if (this.ends.get(pid)?.socket === socket) this.ends.delete(pid);
  }

  /** Where one message from `from` goes, re-addressed from it; nothing when over the rate. */
  route(from: string, data: Uint8Array, now: number): [S, Uint8Array][] {
    const end = this.ends.get(from);
    if (!end || data.length <= 8 || data.length > PLAY_MESSAGE_BYTES) return [];
    end.tokens = Math.min(
      PLAY_RATE_BURST,
      end.tokens + (Math.max(0, now - end.at) / 1000) * PLAY_RATE_PER_SECOND,
    );
    end.at = now;
    if (end.tokens < 1) return [];
    end.tokens -= 1;
    const to = hex(data.subarray(0, 8));
    const forwarded = new Uint8Array(data.length);
    forwarded.set(unhex(from), 0);
    forwarded.set(data.subarray(8), 8);
    const out: [S, Uint8Array][] = [];
    for (const [pid, other] of this.ends) {
      if (to === EVERYONE ? pid !== from : pid === to) out.push([other.socket, forwarded]);
    }
    return out;
  }
}
