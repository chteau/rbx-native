// Worker entry: health check and routing of room requests to their
// Durable Object.

import type { Env } from "./contracts.js";

export { RoomObject } from "./durable.js";

const ROOM_PATH = /^\/v1\/rooms\/([0-9a-f]{32})\//;

export default {
  async fetch(request: Request, env: Env): Promise<Response> {
    const url = new URL(request.url);
    if (request.method === "GET" && url.pathname === "/health") {
      return new Response("ok");
    }
    const match = ROOM_PATH.exec(url.pathname);
    if (!match) return new Response("not found", { status: 404 });
    const stub = env.ROOMS.get(env.ROOMS.idFromName(match[1]));
    return stub.fetch(request);
  },
};
