// One Durable Object per room: authenticates every request, keeps the room's
// presence in memory only, and fans changes out to sockets and long-polls.
// Payloads are never logged.

import { canonical, roomId, verify } from "./auth.js";
import {
  EXPIRY_MS,
  KEY_HEADER,
  OPS_REPLY_BYTES,
  POLL_TIMEOUT_MS,
  SIG_HEADER,
  TIME_HEADER,
  USER_HEADER,
  type Client,
  type Env,
  type LoggedOp,
  type Refusal,
} from "./contracts.js";
import { OpLog, type OpStore, type OpsMeta } from "./ops.js";
import { Room } from "./room.js";
import { parseFill, parseSealed, parseSealedOps } from "./validate.js";

const SWEEP_MS = 5_000;
const ROUTE = /^\/v1\/rooms\/([0-9a-f]{32})\/(state|poll|ws|journal|ops|ops\/fill)$/;

interface Poll {
  since: number;
  finish: () => void;
}

/** The op log in the Durable Object's SQLite storage. */
function sqlStore(storage: DurableObjectStorage): OpStore {
  const sql = storage.sql;
  sql.exec(
    "CREATE TABLE IF NOT EXISTS ops (seq INTEGER PRIMARY KEY, user_id INTEGER NOT NULL, client TEXT NOT NULL, ct TEXT NOT NULL)",
  );
  sql.exec(
    "CREATE TABLE IF NOT EXISTS ops_meta (id INTEGER PRIMARY KEY CHECK (id = 1), log TEXT NOT NULL, head INTEGER NOT NULL, floor INTEGER NOT NULL, bytes INTEGER NOT NULL)",
  );
  const row = (r: Record<string, SqlStorageValue>): LoggedOp => ({
    seq: r.seq as number,
    userId: r.user_id as number,
    client: r.client as Client,
    ct: r.ct as string,
  });
  return {
    loadMeta(): OpsMeta {
      const [m] = sql.exec("SELECT log, head, floor, bytes FROM ops_meta WHERE id = 1").toArray();
      if (!m) return { log: "", head: 0, floor: 1, bytes: 0 };
      return { log: m.log as string, head: m.head as number, floor: m.floor as number, bytes: m.bytes as number };
    },
    saveMeta(m) {
      sql.exec(
        "INSERT OR REPLACE INTO ops_meta (id, log, head, floor, bytes) VALUES (1, ?, ?, ?, ?)",
        m.log,
        m.head,
        m.floor,
        m.bytes,
      );
    },
    insert(e) {
      sql.exec("INSERT INTO ops (seq, user_id, client, ct) VALUES (?, ?, ?, ?)", e.seq, e.userId, e.client, e.ct);
    },
    oldest() {
      const [r] = sql.exec("SELECT seq, user_id, client, ct FROM ops ORDER BY seq LIMIT 1").toArray();
      return r ? row(r) : null;
    },
    deleteOldest() {
      sql.exec("DELETE FROM ops WHERE seq = (SELECT MIN(seq) FROM ops)");
    },
    *after(n) {
      for (const r of sql.exec("SELECT seq, user_id, client, ct FROM ops WHERE seq > ? ORDER BY seq", n)) {
        yield row(r);
      }
    },
    transaction: (fn) => storage.transactionSync(fn),
  };
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
  private readonly ops: OpLog;
  /** Long-polls on the op log, woken by the next post or fill. */
  private opsPolls = new Set<Poll>();

  constructor(
    private readonly state: DurableObjectState,
    _env: Env,
  ) {
    this.ops = new OpLog(sqlStore(state.storage));
  }

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
      (route === "ops" && (method === "POST" || method === "GET")) ||
      (route === "ops/fill" && method === "POST") ||
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

    if (route === "ops/fill") return this.handleFill(body, userId, sig, now);
    if (route === "ops" && method === "POST") return this.handleOpsPost(body, userId, sig, now);
    if (route === "ops") return this.handleOpsPoll(url);

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

  private handleOpsPost(body: string, userId: number, sig: string, now: number): Response {
    const sealed = parseSealedOps(body, userId);
    if (isRefusal(sealed)) return refuse(sealed);
    const stale = this.ops.fresh(sig, now);
    if (stale) return refuse(stale);
    return this.opsPosted(this.ops.append(userId, sealed.client, sealed.cts, now));
  }

  private handleFill(body: string, userId: number, sig: string, now: number): Response {
    const fill = parseFill(body);
    if (isRefusal(fill)) return refuse(fill);
    const stale = this.ops.fresh(sig, now);
    if (stale) return refuse(stale);
    return this.opsPosted(this.ops.fill(userId, fill.log, fill.entries, now));
  }

  private opsPosted(result: number | Refusal): Response {
    if (isRefusal(result)) return refuse(result);
    for (const poll of [...this.opsPolls]) {
      if (poll.since !== this.ops.head) poll.finish();
    }
    return json({ seq: result, log: this.ops.log });
  }

  /** Answers at once unless the reader is level with the head; then waits. */
  private handleOpsPoll(url: URL): Promise<Response> | Response {
    const since = Number(url.searchParams.get("since"));
    if (!Number.isSafeInteger(since) || since < 0 || !url.searchParams.has("since")) {
      return refuse({ status: 400, error: "schema" });
    }
    const reply = () => json(this.ops.since(since, OPS_REPLY_BYTES));
    if (this.ops.head !== since) return reply();
    return new Promise<Response>((resolve) => {
      const poll: Poll = {
        since,
        finish: () => {
          clearTimeout(timer);
          this.opsPolls.delete(poll);
          resolve(reply());
        },
      };
      const timer = setTimeout(poll.finish, POLL_TIMEOUT_MS);
      this.opsPolls.add(poll);
    });
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
