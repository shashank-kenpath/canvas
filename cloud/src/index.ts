// HTTP API for the Inkboard apps. Everything except sign-in needs
// `Authorization: Bearer <session token>`.
//
//   POST   /api/auth/device              start a Google sign-in, returns a code to show
//   POST   /api/auth/device/poll         {handle} -> 202 pending | 200 {token, user}
//   GET    /api/me                       the signed-in person
//   POST   /api/auth/logout
//   GET    /api/boards                   boards this person can open
//   POST   /api/boards                   {title} -> new board (owner)
//   PATCH  /api/boards/:id               {title?, orgAccess?}
//   DELETE /api/boards/:id               owner only; archives the board
//   GET    /api/boards/:id/members
//   PUT    /api/boards/:id/members       {email, role: editor|viewer}
//   DELETE /api/boards/:id/members/:email
//   GET    /api/boards/:id/socket        WebSocket into the board's room
//   PUT    /api/assets/:sha256           image bytes, content addressed
//   GET    /api/assets/:sha256

import { authenticate, emailAllowed, pollDeviceLogin, revokeSession, startDeviceLogin } from "./auth";
import type { Env, Person, Role, User } from "./env";
import { HttpError, colorFor, error, json, now, readJson, sha256Hex } from "./util";

export { BoardRoom } from "./room";

const MAX_ASSET = 32 << 20;
const ASSET_TYPES = new Set(["image/png", "image/jpeg", "image/webp", "image/gif", "image/bmp"]);
const ORG_ACCESS = new Set(["none", "view", "edit"]);

interface BoardRow {
  id: string;
  title: string;
  owner_id: string;
  org_access: string;
  created_at: number;
  updated_at: number;
  archived: number;
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    try {
      return await route(req, env);
    } catch (e) {
      if (e instanceof HttpError) return error(e.status, e.message, e.code);
      console.error("unhandled", e instanceof Error ? e.message : String(e));
      return error(500, "Something went wrong on the server");
    }
  },
} satisfies ExportedHandler<Env>;

async function route(req: Request, env: Env): Promise<Response> {
  const url = new URL(req.url);
  const path = url.pathname.replace(/\/+$/, "") || "/";
  const method = req.method;

  if (path === "/health") return json({ app: "Inkboard", ok: true });

  if (path === "/api/auth/device" && method === "POST") {
    const body = await readJson<{ device?: string }>(req);
    return json(await startDeviceLogin(env, String(body.device ?? "")));
  }
  if (path === "/api/auth/device/poll" && method === "POST") {
    const body = await readJson<{ handle?: string }>(req);
    const r = await pollDeviceLogin(env, String(body.handle ?? ""));
    return r.status === "pending" ? json(r, 202) : json({ token: r.token, user: publicUser(r.user) });
  }

  const user = await authenticate(env, req);

  if (path === "/api/me" && method === "GET") return json(publicUser(user));
  if (path === "/api/auth/logout" && method === "POST") {
    await revokeSession(env, user.tokenHash);
    return json({ ok: true });
  }
  if (path === "/api/boards" && method === "GET") return json({ boards: await listBoards(env, user) });
  if (path === "/api/boards" && method === "POST") {
    const body = await readJson<{ title?: string }>(req);
    const title = cleanTitle(body.title);
    const id = crypto.randomUUID();
    const t = now();
    await env.DB.prepare("INSERT INTO boards (id, title, owner_id, org_access, created_at, updated_at) VALUES (?, ?, ?, 'none', ?, ?)")
      .bind(id, title, user.id, t, t).run();
    return json(boardJson({ id, title, owner_id: user.id, org_access: "none", created_at: t, updated_at: t, archived: 0 }, "owner", user), 201);
  }

  const asset = path.match(/^\/api\/assets\/([0-9a-f]{64})$/);
  if (asset) return assets(req, env, asset[1]);

  const m = path.match(/^\/api\/boards\/([0-9a-f-]{36})(\/.*)?$/);
  if (!m) return error(404, "Not found", "not_found");
  const [, boardId, rest = ""] = m;
  const { board, role } = await access(env, user, boardId);

  if (rest === "/socket") {
    if (req.headers.get("upgrade") !== "websocket") return error(426, "Expected a WebSocket");
    return room(env, boardId, user, role, new Request(new URL(`/socket`, url), req));
  }
  if (rest === "" && method === "GET") return json(boardJson(board, role, user));
  if (rest === "" && method === "PATCH") {
    const body = await readJson<{ title?: string; orgAccess?: string }>(req);
    if (body.title !== undefined) {
      if (role === "viewer") throw new HttpError(403, "Only editors can rename this board", "forbidden");
      board.title = cleanTitle(body.title);
    }
    if (body.orgAccess !== undefined) {
      if (role !== "owner") throw new HttpError(403, "Only the owner can change who has access", "forbidden");
      if (!ORG_ACCESS.has(body.orgAccess)) throw new HttpError(400, "orgAccess must be none, view or edit");
      board.org_access = body.orgAccess;
    }
    board.updated_at = now();
    await env.DB.prepare("UPDATE boards SET title = ?, org_access = ?, updated_at = ? WHERE id = ?")
      .bind(board.title, board.org_access, board.updated_at, boardId).run();
    if (body.orgAccess !== undefined) await revalidate(env, boardId, { all: true });
    return json(boardJson(board, role, user));
  }
  if (rest === "" && method === "DELETE") {
    if (role !== "owner") throw new HttpError(403, "Only the owner can delete this board", "forbidden");
    await env.DB.prepare("UPDATE boards SET archived = 1, updated_at = ? WHERE id = ?").bind(now(), boardId).run();
    await revalidate(env, boardId, { all: true, code: 4004 });
    return json({ ok: true });
  }
  if (rest === "/members" && method === "GET") return json(await members(env, board));
  if (rest === "/members" && method === "PUT") {
    if (role !== "owner") throw new HttpError(403, "Only the owner can share this board", "forbidden");
    const body = await readJson<{ email?: string; role?: string }>(req);
    const email = String(body.email ?? "").trim().toLowerCase();
    if (!/^[^@\s]+@[^@\s]+$/.test(email)) throw new HttpError(400, "Enter an email address");
    if (!emailAllowed(env, email)) throw new HttpError(400, "Boards can only be shared inside your organisation", "domain");
    if (body.role !== "editor" && body.role !== "viewer") throw new HttpError(400, "role must be editor or viewer");
    const owner = await env.DB.prepare("SELECT email FROM users WHERE id = ?").bind(board.owner_id).first<string>("email");
    if (owner === email) throw new HttpError(400, "That person already owns this board");
    await env.DB.prepare(
      `INSERT INTO members (board_id, email, role, added_by, added_at) VALUES (?, ?, ?, ?, ?)
       ON CONFLICT(board_id, email) DO UPDATE SET role = excluded.role`,
    ).bind(boardId, email, body.role, user.id, now()).run();
    await revalidate(env, boardId, { emails: [email] });
    return json(await members(env, board));
  }
  const member = rest.match(/^\/members\/(.+)$/);
  if (member && method === "DELETE") {
    const email = decodeURIComponent(member[1]).toLowerCase();
    // People may leave a board themselves.
    if (role !== "owner" && email !== user.email) throw new HttpError(403, "Only the owner can remove people", "forbidden");
    await env.DB.prepare("DELETE FROM members WHERE board_id = ? AND email = ?").bind(boardId, email).run();
    await revalidate(env, boardId, { emails: [email] });
    return json(await members(env, board));
  }
  return error(404, "Not found", "not_found");
}

function publicUser(u: User) {
  return { id: u.id, email: u.email, name: u.name, picture: u.picture, color: colorFor(u.id) };
}

function person(u: User): Person {
  return { id: u.id, name: u.name, email: u.email, color: colorFor(u.id) };
}

function cleanTitle(title: unknown): string {
  const t = String(title ?? "").replace(/[\u0000-\u001f]/g, " ").trim().slice(0, 120);
  return t || "Untitled board";
}

/** The person's role on a board; owners, members, then organisation access. */
async function access(env: Env, user: User, boardId: string): Promise<{ board: BoardRow; role: Role }> {
  const board = await env.DB.prepare("SELECT * FROM boards WHERE id = ? AND archived = 0").bind(boardId).first<BoardRow>();
  if (!board) throw new HttpError(404, "This board does not exist or was deleted", "not_found");
  if (board.owner_id === user.id) return { board, role: "owner" };
  const member = await env.DB.prepare("SELECT role FROM members WHERE board_id = ? AND email = ?").bind(boardId, user.email).first<Role>("role");
  const fromOrg: Role | null = board.org_access === "edit" ? "editor" : board.org_access === "view" ? "viewer" : null;
  // The stronger of an explicit invitation and organisation-wide access wins.
  const role = member === "editor" || fromOrg === "editor" ? "editor" : member ?? fromOrg;
  if (!role) throw new HttpError(403, "You don't have access to this board. Ask its owner to share it.", "forbidden");
  return { board, role };
}

function boardJson(b: BoardRow, role: Role, _user: User, owner?: { name: string; email: string }) {
  return {
    id: b.id,
    title: b.title,
    role,
    orgAccess: b.org_access,
    ownerId: b.owner_id,
    owner: owner ?? null,
    createdAt: b.created_at,
    updatedAt: b.updated_at,
  };
}

async function listBoards(env: Env, user: User) {
  const { results } = await env.DB.prepare(
    `SELECT b.*, m.role AS member_role, u.name AS owner_name, u.email AS owner_email
     FROM boards b
     JOIN users u ON u.id = b.owner_id
     LEFT JOIN members m ON m.board_id = b.id AND m.email = ?
     WHERE b.archived = 0 AND (b.owner_id = ? OR m.email IS NOT NULL OR b.org_access != 'none')
     ORDER BY b.updated_at DESC LIMIT 500`,
  ).bind(user.email, user.id).all<BoardRow & { member_role: Role | null; owner_name: string; owner_email: string }>();
  return results.map((b) => {
    const fromOrg: Role | null = b.org_access === "edit" ? "editor" : b.org_access === "view" ? "viewer" : null;
    const role: Role = b.owner_id === user.id ? "owner" : b.member_role === "editor" || fromOrg === "editor" ? "editor" : (b.member_role ?? fromOrg ?? "viewer");
    return boardJson(b, role, user, { name: b.owner_name, email: b.owner_email });
  });
}

async function members(env: Env, board: BoardRow) {
  const owner = await env.DB.prepare("SELECT id, name, email FROM users WHERE id = ?").bind(board.owner_id).first<{ id: string; name: string; email: string }>();
  const { results } = await env.DB.prepare(
    `SELECT m.email, m.role, u.name FROM members m LEFT JOIN users u ON u.email = m.email WHERE m.board_id = ? ORDER BY m.added_at`,
  ).bind(board.id).all<{ email: string; role: Role; name: string | null }>();
  return {
    orgAccess: board.org_access,
    owner: owner ? { email: owner.email, name: owner.name, role: "owner" } : null,
    members: results.map((m) => ({ email: m.email, name: m.name, role: m.role })),
  };
}

function roomStub(env: Env, boardId: string) {
  return env.ROOMS.get(env.ROOMS.idFromName(boardId));
}

async function room(env: Env, boardId: string, user: User, role: Role, req: Request): Promise<Response> {
  const headers = new Headers();
  for (const [k, v] of req.headers) if (!k.toLowerCase().startsWith("x-ink-")) headers.set(k, v);
  headers.set("x-ink-user", JSON.stringify(person(user)));
  headers.set("x-ink-role", role);
  headers.set("x-ink-board", boardId);
  return roomStub(env, boardId).fetch(new Request(req.url, { method: req.method, headers }));
}

/** Disconnects affected people so they reconnect with their new access. */
async function revalidate(env: Env, boardId: string, body: { emails?: string[]; all?: boolean; code?: number }) {
  await roomStub(env, boardId).fetch("https://room/internal/revalidate", { method: "POST", body: JSON.stringify(body) });
}

async function assets(req: Request, env: Env, hash: string): Promise<Response> {
  if (req.method === "GET") {
    const obj = await env.ASSETS.get(hash);
    if (!obj) return error(404, "Image not found", "not_found");
    return new Response(obj.body, {
      headers: { "content-type": obj.httpMetadata?.contentType ?? "application/octet-stream", "cache-control": "private, max-age=31536000, immutable" },
    });
  }
  if (req.method === "PUT") {
    const type = (req.headers.get("content-type") ?? "").split(";")[0].trim();
    if (!ASSET_TYPES.has(type)) throw new HttpError(415, "Only PNG, JPEG, WebP, GIF and BMP images can be uploaded");
    const length = Number(req.headers.get("content-length") ?? 0);
    if (length > MAX_ASSET) throw new HttpError(413, "Images must be 32 MB or smaller");
    if (await env.ASSETS.head(hash)) return json({ hash, existed: true });
    const bytes = await req.arrayBuffer();
    if (bytes.byteLength > MAX_ASSET) throw new HttpError(413, "Images must be 32 MB or smaller");
    if ((await sha256Hex(bytes)) !== hash) throw new HttpError(400, "The image does not match its hash");
    await env.ASSETS.put(hash, bytes, { httpMetadata: { contentType: type } });
    return json({ hash, existed: false }, 201);
  }
  return error(405, "Method not allowed");
}
