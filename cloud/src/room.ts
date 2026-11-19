// One Durable Object per board. It is the single source of truth for the
// board's shapes: every change gets the next revision number, is stored in
// the object's SQLite database and is sent to everyone else on the board.
// Shapes are opaque JSON here; the apps own their format.
//
// Wire protocol (JSON text frames):
//   app  -> room  {t:"hello", rev, epoch}       after connecting
//   room -> app   {t:"welcome", epoch, rev, full, you, peers, people}
//                 {t:"sync", items, del}         zero or more chunks
//                 {t:"synced", rev}
//   app  -> room  {t:"ops", id, put:[shape], del:[id]}
//   room -> app   {t:"ack", id, rev, at}         to the sender
//                 {t:"ops", rev, by, at, items, del} to everyone else
//   app  -> room  {t:"presence", cursor:[x,y]|null, sel:[id], tool, view:[l,t,r,b]}
//   room -> app   {t:"presence", conn, user, ...}, {t:"join", conn, user}, {t:"leave", conn}
//                 {t:"error", code, message}
// An item is {s: shape, c: createdBy, e: editedBy, at: editedAt}.

import { DurableObject } from "cloudflare:workers";
import type { Env, Person, Role } from "./env";

const CHUNK_BYTES = 512 << 10;
const MAX_ID = 128;
const MAX_SHAPE_BYTES = 4 << 20;
const MAX_SELECTION = 500;
const TOUCH_MS = 15_000;

interface Attachment {
  conn: string;
  user: Person;
  role: Role;
  presence: Presence | null;
  /** Presence frames allowed in the current second. */
  budget: [number, number];
}

interface Presence {
  cursor: [number, number] | null;
  sel: string[];
  tool: string;
  view: [number, number, number, number] | null;
}

type Row = {
  id: string;
  data: string | null;
  rev: number;
  deleted: number;
  created_by: string;
  edited_by: string;
  edited_at: number;
};

export class BoardRoom extends DurableObject<Env> {
  private sql: SqlStorage;
  private lastTouch = 0;

  constructor(ctx: DurableObjectState, env: Env) {
    super(ctx, env);
    this.sql = ctx.storage.sql;
    this.sql.exec(`
      CREATE TABLE IF NOT EXISTS shapes (
        id TEXT PRIMARY KEY, data TEXT, rev INTEGER NOT NULL, deleted INTEGER NOT NULL DEFAULT 0,
        created_by TEXT NOT NULL, edited_by TEXT NOT NULL, edited_at INTEGER NOT NULL);
      CREATE INDEX IF NOT EXISTS shapes_rev ON shapes(rev);
      CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
      CREATE TABLE IF NOT EXISTS people (id TEXT PRIMARY KEY, name TEXT NOT NULL, color TEXT NOT NULL);
    `);
    if (this.meta("epoch") === null) {
      this.setMeta("epoch", crypto.randomUUID());
      this.setMeta("rev", "0");
    }
    this.ctx.setWebSocketAutoResponse(new WebSocketRequestResponsePair('{"t":"ping"}', '{"t":"pong"}'));
  }

  private meta(k: string): string | null {
    const rows = this.sql.exec<{ v: string }>("SELECT v FROM meta WHERE k = ?", k).toArray();
    return rows.length ? rows[0].v : null;
  }

  private setMeta(k: string, v: string) {
    this.sql.exec("INSERT INTO meta (k, v) VALUES (?, ?) ON CONFLICT(k) DO UPDATE SET v = excluded.v", k, v);
  }

  private get rev(): number {
    return Number(this.meta("rev") ?? 0);
  }

  async fetch(req: Request): Promise<Response> {
    const url = new URL(req.url);
    // Identity and role were checked by the Worker, which strips these
    // headers from anything the app sends.
    const user = JSON.parse(req.headers.get("x-ink-user") ?? "null") as Person | null;
    const role = req.headers.get("x-ink-role") as Role | null;
    const board = req.headers.get("x-ink-board");
    if (board && this.meta("board") === null) this.setMeta("board", board);

    if (url.pathname.endsWith("/socket")) {
      if (req.headers.get("upgrade") !== "websocket") return new Response("Expected a WebSocket", { status: 426 });
      if (!user || !role) return new Response("Forbidden", { status: 403 });
      const pair = new WebSocketPair();
      const [client, server] = Object.values(pair);
      this.ctx.acceptWebSocket(server, [user.id, user.email]);
      const att: Attachment = { conn: crypto.randomUUID().slice(0, 8), user, role, presence: null, budget: [0, 0] };
      server.serializeAttachment(att);
      this.sql.exec(
        "INSERT INTO people (id, name, color) VALUES (?, ?, ?) ON CONFLICT(id) DO UPDATE SET name = excluded.name, color = excluded.color",
        user.id, user.name, user.color,
      );
      return new Response(null, { status: 101, webSocket: client });
    }
    if (url.pathname === "/internal/revalidate" && req.method === "POST") {
      const { emails, all, code } = (await req.json()) as { emails?: string[]; all?: boolean; code?: number };
      const wanted = new Set((emails ?? []).map((e) => e.toLowerCase()));
      for (const ws of this.ctx.getWebSockets()) {
        const att = ws.deserializeAttachment() as Attachment;
        if (all || wanted.has(att.user.email.toLowerCase())) ws.close(code ?? 4001, "Access changed");
      }
      return new Response(null, { status: 204 });
    }
    if (url.pathname === "/internal/snapshot") {
      return Response.json({ epoch: this.meta("epoch"), rev: this.rev, items: this.items(0).items, people: this.people() });
    }
    if (url.pathname === "/internal/ops" && req.method === "POST") {
      if (!user || role === "viewer" || !role) return new Response("Forbidden", { status: 403 });
      const body = (await req.json()) as { put?: unknown[]; del?: unknown[] };
      const result = this.apply(user, body.put ?? [], body.del ?? [], null);
      if (typeof result === "string") return Response.json({ error: result }, { status: 400 });
      return Response.json({ rev: result.rev });
    }
    if (url.pathname === "/internal/presence") {
      return Response.json(this.peers(null));
    }
    return new Response("Not found", { status: 404 });
  }

  async webSocketMessage(ws: WebSocket, message: string | ArrayBuffer) {
    const att = ws.deserializeAttachment() as Attachment;
    let msg: Record<string, unknown>;
    try {
      msg = JSON.parse(typeof message === "string" ? message : new TextDecoder().decode(message));
    } catch {
      return this.send(ws, { t: "error", code: "bad_message", message: "Messages must be JSON" });
    }
    switch (msg.t) {
      case "hello":
        return this.hello(ws, att, msg);
      case "ops": {
        if (att.role === "viewer") {
          return this.send(ws, { t: "error", code: "read_only", message: "You can view this board but not edit it", id: msg.id });
        }
        const result = this.apply(att.user, msg.put, msg.del, ws);
        if (typeof result === "string") return this.send(ws, { t: "error", code: "bad_ops", message: result, id: msg.id });
        this.send(ws, { t: "ack", id: msg.id, rev: result.rev, at: result.at });
        await this.touch();
        return;
      }
      case "presence":
        return this.presence(ws, att, msg);
      case "ping":
        return this.send(ws, { t: "pong" });
      default:
        return this.send(ws, { t: "error", code: "unknown", message: `Unknown message ${String(msg.t)}` });
    }
  }

  async webSocketClose(ws: WebSocket, code: number) {
    this.left(ws);
    try {
      ws.close(code === 1005 ? 1000 : code, "Closed");
    } catch {
      // Already closed.
    }
  }

  async webSocketError(ws: WebSocket) {
    this.left(ws);
  }

  private left(ws: WebSocket) {
    const att = ws.deserializeAttachment() as Attachment;
    this.broadcast({ t: "leave", conn: att.conn }, ws);
  }

  private send(ws: WebSocket, msg: unknown) {
    try {
      ws.send(JSON.stringify(msg));
    } catch {
      // The socket is closing; its close handler cleans up.
    }
  }

  private broadcast(msg: unknown, except: WebSocket | null) {
    const text = JSON.stringify(msg);
    for (const ws of this.ctx.getWebSockets()) {
      if (ws === except) continue;
      try {
        ws.send(text);
      } catch {
        // Ignore sockets that are going away.
      }
    }
  }

  private people(): Record<string, { name: string; color: string }> {
    const out: Record<string, { name: string; color: string }> = {};
    for (const p of this.sql.exec<{ id: string; name: string; color: string }>("SELECT id, name, color FROM people")) {
      out[p.id] = { name: p.name, color: p.color };
    }
    return out;
  }

  private peers(except: WebSocket | null) {
    const out = [];
    for (const ws of this.ctx.getWebSockets()) {
      if (ws === except) continue;
      const a = ws.deserializeAttachment() as Attachment;
      out.push({ conn: a.conn, user: a.user, role: a.role, ...(a.presence ?? {}) });
    }
    return out;
  }

  /** Shapes changed after `since`, as items and deleted ids. */
  private items(since: number) {
    const items: unknown[] = [];
    const del: string[] = [];
    for (const r of this.sql.exec<Row>("SELECT * FROM shapes WHERE rev > ? ORDER BY rev", since)) {
      if (r.deleted) del.push(r.id);
      else items.push({ s: JSON.parse(r.data!), c: r.created_by, e: r.edited_by, at: r.edited_at });
    }
    return { items, del };
  }

  private hello(ws: WebSocket, att: Attachment, msg: Record<string, unknown>) {
    const epoch = this.meta("epoch")!;
    const rev = this.rev;
    const known = typeof msg.rev === "number" && msg.epoch === epoch && msg.rev <= rev ? msg.rev : 0;
    const full = known === 0;
    this.send(ws, {
      t: "welcome",
      epoch,
      rev,
      full,
      you: { conn: att.conn, user: att.user, role: att.role },
      peers: this.peers(ws),
      people: this.people(),
    });
    const { items, del } = this.items(known);
    // Large boards go out in several frames.
    let chunk: unknown[] = [];
    let bytes = 0;
    for (const item of items) {
      const size = JSON.stringify(item).length;
      if (chunk.length && bytes + size > CHUNK_BYTES) {
        this.send(ws, { t: "sync", items: chunk, del: [] });
        chunk = [];
        bytes = 0;
      }
      chunk.push(item);
      bytes += size;
    }
    this.send(ws, { t: "sync", items: chunk, del: full ? [] : del });
    this.send(ws, { t: "synced", rev });
    this.broadcast({ t: "join", conn: att.conn, user: att.user, role: att.role }, ws);
  }

  /** Stores a batch under one new revision and tells everyone except `from`. */
  private apply(user: Person, put: unknown, del: unknown, from: WebSocket | null): { rev: number; at: number } | string {
    const puts = Array.isArray(put) ? put : [];
    const dels = Array.isArray(del) ? del : [];
    for (const s of puts) {
      if (typeof s !== "object" || s === null || Array.isArray(s)) return "Each shape must be an object";
      const id = (s as { id?: unknown }).id;
      if (typeof id !== "string" || !id || id.length > MAX_ID) return "Each shape needs a string id";
    }
    for (const id of dels) if (typeof id !== "string" || !id || id.length > MAX_ID) return "Deleted ids must be strings";
    if (!puts.length && !dels.length) return { rev: this.rev, at: Date.now() };

    const rev = this.rev + 1;
    const at = Date.now();
    const items: unknown[] = [];
    const removed: string[] = [];
    this.ctx.storage.transactionSync(() => {
      for (const s of puts as { id: string }[]) {
        const data = JSON.stringify(s);
        if (data.length > MAX_SHAPE_BYTES) continue;
        const prev = this.sql.exec<{ created_by: string; deleted: number }>("SELECT created_by, deleted FROM shapes WHERE id = ?", s.id).toArray()[0];
        const createdBy = prev && !prev.deleted ? prev.created_by : user.id;
        this.sql.exec(
          `INSERT INTO shapes (id, data, rev, deleted, created_by, edited_by, edited_at) VALUES (?, ?, ?, 0, ?, ?, ?)
           ON CONFLICT(id) DO UPDATE SET data = excluded.data, rev = excluded.rev, deleted = 0,
             created_by = excluded.created_by, edited_by = excluded.edited_by, edited_at = excluded.edited_at`,
          s.id, data, rev, createdBy, user.id, at,
        );
        items.push({ s, c: createdBy, e: user.id, at });
      }
      for (const id of dels as string[]) {
        const cur = this.sql.exec("UPDATE shapes SET data = NULL, deleted = 1, rev = ?, edited_by = ?, edited_at = ? WHERE id = ? AND deleted = 0", rev, user.id, at, id);
        if (cur.rowsWritten > 0) removed.push(id);
      }
      this.setMeta("rev", String(rev));
    });
    this.broadcast({ t: "ops", rev, by: user.id, at, items, del: removed }, from);
    return { rev, at };
  }

  private presence(ws: WebSocket, att: Attachment, msg: Record<string, unknown>) {
    const second = Math.floor(Date.now() / 1000);
    if (att.budget[0] !== second) att.budget = [second, 0];
    if (++att.budget[1] > 40) return;
    const finite = (v: unknown): v is number => typeof v === "number" && Number.isFinite(v);
    const cursor = Array.isArray(msg.cursor) && msg.cursor.length === 2 && msg.cursor.every(finite) ? (msg.cursor as [number, number]) : null;
    const view = Array.isArray(msg.view) && msg.view.length === 4 && msg.view.every(finite) ? (msg.view as Presence["view"]) : null;
    const sel = Array.isArray(msg.sel) ? msg.sel.filter((id): id is string => typeof id === "string" && id.length <= MAX_ID).slice(0, MAX_SELECTION) : [];
    const tool = typeof msg.tool === "string" ? msg.tool.slice(0, 24) : "";
    att.presence = { cursor, sel, tool, view };
    ws.serializeAttachment(att);
    this.broadcast({ t: "presence", conn: att.conn, user: att.user.id, cursor, sel, tool, view }, ws);
  }

  /** Keeps the board list's "last edited" time roughly current. */
  private async touch() {
    const board = this.meta("board");
    if (!board || Date.now() - this.lastTouch < TOUCH_MS) return;
    this.lastTouch = Date.now();
    try {
      await this.env.DB.prepare("UPDATE boards SET updated_at = ? WHERE id = ?").bind(this.lastTouch, board).run();
    } catch {
      // The list time is cosmetic; a failed update is retried on the next edit.
    }
  }
}
