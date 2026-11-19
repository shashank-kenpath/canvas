import { SELF, env } from "cloudflare:test";
import { createSession } from "../src/auth";

let n = 0;

export async function signIn(email: string, name = email.split("@")[0]) {
  const id = `user-${++n}-${email}`;
  await env.DB.prepare("INSERT INTO users (id, email, name, created_at, last_seen_at) VALUES (?, ?, ?, 0, 0)").bind(id, email, name).run();
  const token = await createSession(env, id, "test");
  return { id, email, name, token };
}

export type Who = Awaited<ReturnType<typeof signIn>>;

export function api(who: Who | null, method: string, path: string, body?: unknown) {
  const headers: Record<string, string> = {};
  if (who) headers.authorization = `Bearer ${who.token}`;
  if (body !== undefined) headers["content-type"] = "application/json";
  return SELF.fetch(`https://inkboard.test${path}`, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
}

/** A connected board socket that records every message it receives. */
export class Client {
  messages: Record<string, any>[] = [];
  closed: number | null = null;
  private waiters: (() => void)[] = [];

  private constructor(public ws: WebSocket) {
    ws.accept();
    ws.addEventListener("message", (e) => {
      this.messages.push(JSON.parse(e.data as string));
      this.waiters.splice(0).forEach((w) => w());
    });
    ws.addEventListener("close", (e) => {
      this.closed = e.code;
      this.waiters.splice(0).forEach((w) => w());
    });
  }

  static async connect(who: Who, boardId: string, hello: Record<string, unknown> = { rev: 0 }) {
    const res = await SELF.fetch(`https://inkboard.test/api/boards/${boardId}/socket`, {
      headers: { upgrade: "websocket", authorization: `Bearer ${who.token}` },
    });
    if (res.status !== 101 || !res.webSocket) throw new Error(`socket refused: ${res.status} ${await res.text()}`);
    const c = new Client(res.webSocket);
    c.send({ t: "hello", ...hello });
    await c.next((m) => m.t === "synced");
    return c;
  }

  send(msg: unknown) {
    this.ws.send(JSON.stringify(msg));
  }

  /** Resolves with the first message (received so far or later) that matches. */
  async next(match: (m: Record<string, any>) => boolean, timeout = 3000): Promise<Record<string, any>> {
    const deadline = Date.now() + timeout;
    for (;;) {
      const found = this.messages.find(match);
      if (found) {
        this.messages.splice(this.messages.indexOf(found), 1);
        return found;
      }
      if (this.closed !== null) throw new Error(`socket closed (${this.closed}) while waiting`);
      const left = deadline - Date.now();
      if (left <= 0) throw new Error(`timed out; have ${JSON.stringify(this.messages)}`);
      await new Promise<void>((resolve) => {
        const t = setTimeout(resolve, left);
        this.waiters.push(() => {
          clearTimeout(t);
          resolve();
        });
      });
    }
  }

  async waitClosed(timeout = 3000) {
    const deadline = Date.now() + timeout;
    while (this.closed === null && Date.now() < deadline) await new Promise((r) => setTimeout(r, 10));
    return this.closed;
  }

  close() {
    try {
      this.ws.close(1000);
    } catch {
      // ignore
    }
  }
}

export const shape = (id: string, extra: Record<string, unknown> = {}) => ({ id, tool: "rectangle", points: [[0, 0], [10, 10]], ...extra });
