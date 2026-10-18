// One Durable Object per room: authenticates every request, keeps the room's
// presence in memory only, and fans changes out to sockets and long-polls.
// Payloads are never logged.

import { canonical, roomId, verify } from "./auth.js";
import {
  EXPIRY_MS,
  KEY_HEADER,
  POLL_TIMEOUT_MS,
  SIG_HEADER,
  TIME_HEADER,
  USER_HEADER,
  type Client,
  type Env,
  type Refusal,
} from "./contracts.js";
import { Room } from "./room.js";
import { parseSealed } from "./validate.js";

const SWEEP_MS = 5_000;
const ROUTE = /^\/v1\/rooms\/([0-9a-f]{32})\/(state|poll|ws|journal)$/;

interface Poll {
  since: number;
  finish: () => void;
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json" },
  });
}

function refuse(r: Refusal): Response {
  return json({ error: r.error }, r.status);
}

function isRefusal(value: unknown): value is Refusal {
  return typeof value === "object" && value !== null && "status" in value;
}

export class RoomObject implements DurableObject {
  private readonly room = new Room();
  /** Each socket's user, and its editor once its first state names it. */
  private readonly sockets = new Map<WebSocket, { userId: number; client?: Client }>();
  private polls = new Set<Poll>();

  constructor(
    private readonly state: DurableObjectState,
    _env: Env,
  ) {}

  async fetch(request: Request): Promise<Response> {
    const url = new URL(request.url);
    const match = ROUTE.exec(url.pathname);
    if (!match) return new Response("not found", { status: 404 });
    const [, room, route] = match;
    const method = request.method;

    const allowed =
      (route === "state" && (method === "POST" || method === "DELETE")) ||
      (route === "poll" && method === "GET") ||
      (route === "journal" && method === "GET") ||
      (route === "ws" && method === "GET");
    if (!allowed) return new Response("method not allowed", { status: 405 });

    const userId = Number(request.headers.get(USER_HEADER));
    const ts = Number(request.headers.get(TIME_HEADER));
    const sig = request.headers.get(SIG_HEADER);
    if (!Number.isSafeInteger(userId) || sig === null || !request.headers.has(TIME_HEADER)) {
      return refuse({ status: 401, error: "sig" });
    }

    const offeredKey = request.headers.get(KEY_HEADER);
    if (offeredKey !== null) {
      if ((await roomId(offeredKey)) !== room) {
        return refuse({ status: 401, error: "sig" });
      }
      this.room.key = offeredKey;
    }
    const key = this.room.key;
    if (key === null) return refuse({ status: 401, error: "key" });

    const now = Date.now();
    const timeFault = this.room.checkTime(userId, ts, now, false);
    if (timeFault) return refuse(timeFault);

    const body = method === "POST" ? await request.text() : "";
    const text = await canonical(method, url.pathname + url.search, userId, ts, body);
    if (!(await verify(key, text, sig))) return refuse({ status: 401, error: "sig" });

    if (method !== "GET") {
      // Checked after the signature so an unsigned request cannot advance it.
      const replay = this.room.checkTime(userId, ts, now, true);
      if (replay) return refuse(replay);
    }

    if (route === "state" && method === "POST") return this.handlePost(body, userId, now);
    if (route === "state") return this.handleDelete(userId, url);
    if (route === "poll") return this.handlePoll(url);
    if (route === "journal") return this.handleJournal(url, now);
    return this.handleSocket(request, userId);
  }

  async alarm(): Promise<void> {
    if (this.room.expire(Date.now())) this.notify();
    await this.scheduleSweep();
  }

  private handlePost(body: string, userId: number, now: number): Response {
    const parsed = parseSealed(body, userId);
    if (isRefusal(parsed)) return refuse(parsed);
    const fault = this.room.put(parsed, now);
    if (fault) return refuse(fault);
    this.notify();
    void this.scheduleSweep();
    return json({ cursor: this.room.cursor });
  }

  /**
   * `?client=` (signed with the path) names which editor is leaving; only the
   * plugin deletes over HTTP, so it defaults to Studio.
   */
  private handleDelete(userId: number, url: URL): Response {
    const client = url.searchParams.get("client") === "rbx-native" ? "rbx-native" : "studio";
    if (this.room.remove(userId, client)) this.notify();
    return json({ cursor: this.room.cursor });
  }

  private handleJournal(url: URL, now: number): Response {
    const since = Number(url.searchParams.get("since") ?? 0);
    if (!Number.isFinite(since)) return refuse({ status: 400, error: "schema" });
    return json({ entries: this.room.journalSince(since, now) });
  }

  private handlePoll(url: URL): Promise<Response> | Response {
    const since = Number(url.searchParams.get("since") ?? 0);
    if (!Number.isFinite(since)) return refuse({ status: 400, error: "schema" });
    if (this.room.cursor > since) return json(this.room.snapshot());
    return new Promise<Response>((resolve) => {
      const poll: Poll = {
        since,
        finish: () => {
          clearTimeout(timer);
          this.polls.delete(poll);
          resolve(json(this.room.snapshot()));
        },
      };
      const timer = setTimeout(poll.finish, POLL_TIMEOUT_MS);
      this.polls.add(poll);
    });
  }

  private handleSocket(request: Request, userId: number): Response {
    if (request.headers.get("Upgrade")?.toLowerCase() !== "websocket") {
      return new Response("expected websocket", { status: 426 });
    }
    const pair = new WebSocketPair();
    const client = pair[0];
    const server = pair[1];
    server.accept();
    const entry: { userId: number; client?: Client } = { userId };
    this.sockets.set(server, entry);

    server.addEventListener("message", (event) => {
      const data = event.data;
      const parsed = typeof data === "string" ? parseSealed(data, userId) : null;
      if (parsed === null || isRefusal(parsed)) {
        server.close(1008, "invalid state");
        return;
      }
      entry.client = parsed.client;
      if (this.room.put(parsed, Date.now())) {
        server.close(1008, "refused");
        return;
      }
      this.notify();
      void this.scheduleSweep();
    });
    const gone = () => {
      if (!this.sockets.delete(server)) return;
      const client = entry.client;
      if (client === undefined) return;
      const another = [...this.sockets.values()].some(
        (e) => e.userId === userId && e.client === client,
      );
      if (!another && this.room.remove(userId, client)) this.notify();
    };
    server.addEventListener("close", gone);
    server.addEventListener("error", gone);

    server.send(JSON.stringify(this.room.snapshot()));
    return new Response(null, { status: 101, webSocket: client });
  }

  private notify(): void {
    const snapshot = JSON.stringify(this.room.snapshot());
    for (const socket of this.sockets.keys()) {
      try {
        socket.send(snapshot);
      } catch {
        this.sockets.delete(socket);
      }
    }
    for (const poll of [...this.polls]) {
      if (poll.since < this.room.cursor) poll.finish();
    }
  }

  private async scheduleSweep(): Promise<void> {
    if (this.room.isEmpty()) return;
    if ((await this.state.storage.getAlarm()) === null) {
      await this.state.storage.setAlarm(Date.now() + Math.min(SWEEP_MS, EXPIRY_MS));
    }
  }
}
