// Google Workspace sign-in using the OAuth device flow, so TVs, phones and
// desktops all sign in the same way: the app shows a short code and a QR
// link, the person approves on any browser, and the app gets a session.
// The Google client secret and device code stay on the server.

import type { Env, User } from "./env";
import { HttpError, fromBase64url, now, randomToken, sha256Hex } from "./util";

const SESSION_MS = 60 * 24 * 3600 * 1000;
const ISSUERS = new Set(["accounts.google.com", "https://accounts.google.com"]);

function googleUrl(env: Env, path: string): string {
  if (env.GOOGLE_BASE) return env.GOOGLE_BASE.replace(/\/$/, "") + path;
  return path === "/certs" ? "https://www.googleapis.com/oauth2/v3/certs" : `https://oauth2.googleapis.com${path}`;
}

export function allowedDomains(env: Env): string[] {
  return env.ALLOWED_DOMAINS.split(",").map((d) => d.trim().toLowerCase()).filter(Boolean);
}

export function emailAllowed(env: Env, email: string): boolean {
  const domain = email.toLowerCase().split("@")[1] ?? "";
  return allowedDomains(env).includes(domain);
}

function configured(env: Env) {
  if (!env.GOOGLE_CLIENT_ID || !env.GOOGLE_CLIENT_SECRET || allowedDomains(env).length === 0) {
    throw new HttpError(503, "Sign-in is not set up on this server yet", "not_configured");
  }
}

export interface DeviceStart {
  handle: string;
  userCode: string;
  verificationUrl: string;
  interval: number;
  expiresIn: number;
}

export async function startDeviceLogin(env: Env, device: string): Promise<DeviceStart> {
  configured(env);
  const res = await fetch(googleUrl(env, "/device/code"), {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({ client_id: env.GOOGLE_CLIENT_ID, scope: "openid email profile" }),
  });
  if (!res.ok) throw new HttpError(502, "Google sign-in is unavailable right now", "google");
  const g = (await res.json()) as { device_code: string; user_code: string; verification_url: string; expires_in: number; interval?: number };
  const handle = randomToken(24);
  const interval = Math.max(5, g.interval ?? 5);
  await env.DB.prepare("INSERT INTO device_logins (handle, device_code, device, interval_s, next_poll_at, expires_at) VALUES (?, ?, ?, ?, ?, ?)")
    .bind(handle, g.device_code, device.slice(0, 80), interval, now(), now() + g.expires_in * 1000)
    .run();
  // Old unfinished sign-ins are cleared opportunistically.
  await env.DB.prepare("DELETE FROM device_logins WHERE expires_at < ?").bind(now()).run();
  return { handle, userCode: g.user_code, verificationUrl: g.verification_url, interval, expiresIn: g.expires_in };
}

export type PollResult =
  | { status: "pending"; interval: number }
  | { status: "done"; token: string; user: User };

export async function pollDeviceLogin(env: Env, handle: string): Promise<PollResult> {
  configured(env);
  const row = await env.DB.prepare("SELECT * FROM device_logins WHERE handle = ?").bind(handle).first<{
    device_code: string; device: string; interval_s: number; next_poll_at: number; expires_at: number;
  }>();
  if (!row || row.expires_at < now()) throw new HttpError(410, "This sign-in code has expired. Start again.", "expired");
  // Google rate-limits polling; answer early polls without asking it.
  if (row.next_poll_at > now()) return { status: "pending", interval: row.interval_s };
  const res = await fetch(googleUrl(env, "/token"), {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      client_id: env.GOOGLE_CLIENT_ID,
      client_secret: env.GOOGLE_CLIENT_SECRET!,
      device_code: row.device_code,
      grant_type: "urn:ietf:params:oauth:grant-type:device_code",
    }),
  });
  const g = (await res.json().catch(() => ({}))) as { error?: string; id_token?: string };
  if (g.error === "authorization_pending" || g.error === "slow_down") {
    const interval = g.error === "slow_down" ? row.interval_s + 5 : row.interval_s;
    await env.DB.prepare("UPDATE device_logins SET interval_s = ?, next_poll_at = ? WHERE handle = ?")
      .bind(interval, now() + interval * 1000 - 500, handle).run();
    return { status: "pending", interval };
  }
  await env.DB.prepare("DELETE FROM device_logins WHERE handle = ?").bind(handle).run();
  if (g.error === "access_denied") throw new HttpError(403, "Sign-in was cancelled", "denied");
  if (g.error === "expired_token") throw new HttpError(410, "This sign-in code has expired. Start again.", "expired");
  if (!res.ok || !g.id_token) throw new HttpError(502, "Google sign-in failed", "google");
  const claims = await verifyIdToken(env, g.id_token);
  const user = await upsertUser(env, claims);
  const token = await createSession(env, user.id, row.device);
  return { status: "done", token, user };
}

export interface GoogleClaims {
  sub: string;
  email: string;
  email_verified?: boolean;
  hd?: string;
  name?: string;
  picture?: string;
  aud: string;
  iss: string;
  exp: number;
}

let jwksCache: { at: number; keys: JsonWebKey[] } | null = null;

async function googleKeys(env: Env): Promise<JsonWebKey[]> {
  if (jwksCache && now() - jwksCache.at < 3600_000) return jwksCache.keys;
  const res = await fetch(googleUrl(env, "/certs"));
  if (!res.ok) throw new HttpError(502, "Could not check the Google sign-in", "google");
  const { keys } = (await res.json()) as { keys: JsonWebKey[] };
  jwksCache = { at: now(), keys };
  return keys;
}

export function resetKeyCache() {
  jwksCache = null;
}

/** Checks the signature and claims, and that the account is in an allowed Workspace. */
export async function verifyIdToken(env: Env, idToken: string): Promise<GoogleClaims> {
  const parts = idToken.split(".");
  if (parts.length !== 3) throw new HttpError(401, "Malformed sign-in token");
  const header = JSON.parse(new TextDecoder().decode(fromBase64url(parts[0]))) as { kid?: string; alg?: string };
  if (header.alg !== "RS256") throw new HttpError(401, "Unexpected sign-in token");
  const jwk = (await googleKeys(env)).find((k) => (k as { kid?: string }).kid === header.kid);
  if (!jwk) throw new HttpError(401, "Unknown sign-in key");
  const key = await crypto.subtle.importKey("jwk", jwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, false, ["verify"]);
  const ok = await crypto.subtle.verify(
    "RSASSA-PKCS1-v1_5",
    key,
    fromBase64url(parts[2]),
    new TextEncoder().encode(`${parts[0]}.${parts[1]}`),
  );
  if (!ok) throw new HttpError(401, "Invalid sign-in token");
  const c = JSON.parse(new TextDecoder().decode(fromBase64url(parts[1]))) as GoogleClaims;
  if (!ISSUERS.has(c.iss) || ![env.GOOGLE_CLIENT_ID, env.GOOGLE_WEB_CLIENT_ID].includes(c.aud) || c.exp * 1000 < now() - 60_000) {
    throw new HttpError(401, "Sign-in token is not valid for this app");
  }
  // `hd` is only present for Workspace accounts, so personal Gmail is refused.
  const domains = allowedDomains(env);
  if (!c.email_verified || !c.hd || !domains.includes(c.hd.toLowerCase()) || !emailAllowed(env, c.email)) {
    throw new HttpError(403, `Sign in with your ${domains.join(" or ")} Google account`, "domain");
  }
  return c;
}

export async function upsertUser(env: Env, c: GoogleClaims): Promise<User> {
  const t = now();
  const email = c.email.toLowerCase();
  const name = (c.name || email.split("@")[0]).slice(0, 80);
  await env.DB.prepare(
    `INSERT INTO users (id, email, name, picture, created_at, last_seen_at) VALUES (?, ?, ?, ?, ?, ?)
     ON CONFLICT(id) DO UPDATE SET email = excluded.email, name = excluded.name, picture = excluded.picture, last_seen_at = excluded.last_seen_at`,
  ).bind(c.sub, email, name, c.picture ?? null, t, t).run();
  const disabled = await env.DB.prepare("SELECT disabled FROM users WHERE id = ?").bind(c.sub).first<number>("disabled");
  if (disabled) throw new HttpError(403, "This account has been turned off by an administrator", "disabled");
  return { id: c.sub, email, name, picture: c.picture ?? null };
}

export async function createSession(env: Env, userId: string, device: string): Promise<string> {
  const token = randomToken(32);
  await env.DB.prepare("INSERT INTO sessions (token_hash, user_id, device, created_at, expires_at) VALUES (?, ?, ?, ?, ?)")
    .bind(await sha256Hex(token), userId, device.slice(0, 80), now(), now() + SESSION_MS)
    .run();
  return token;
}

/** The signed-in user for a request, from `Authorization: Bearer <token>`. */
/**
 * Browsers cannot send headers with a WebSocket, so the web app offers the
 * subprotocols `inkboard` and `bearer.<token>`; the room answers `inkboard`.
 */
function socketToken(req: Request): string {
  if (req.headers.get("upgrade") !== "websocket") return "";
  const offered = (req.headers.get("sec-websocket-protocol") ?? "").split(",").map((p) => p.trim());
  return offered.find((p) => p.startsWith("bearer."))?.slice(7) ?? "";
}

export async function authenticate(env: Env, req: Request): Promise<User & { tokenHash: string }> {
  const header = req.headers.get("authorization") ?? "";
  const token = header.startsWith("Bearer ") ? header.slice(7).trim() : socketToken(req);
  if (!token) throw new HttpError(401, "Sign in first", "signed_out");
  const tokenHash = await sha256Hex(token);
  const row = await env.DB.prepare(
    `SELECT u.id, u.email, u.name, u.picture, u.disabled, s.expires_at, s.revoked
     FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = ?`,
  ).bind(tokenHash).first<User & { disabled: number; expires_at: number; revoked: number }>();
  if (!row || row.revoked || row.expires_at < now()) throw new HttpError(401, "Your sign-in has expired", "signed_out");
  if (row.disabled || !emailAllowed(env, row.email)) throw new HttpError(403, "This account can no longer sign in", "disabled");
  return { id: row.id, email: row.email, name: row.name, picture: row.picture, tokenHash };
}

export async function revokeSession(env: Env, tokenHash: string) {
  await env.DB.prepare("UPDATE sessions SET revoked = 1 WHERE token_hash = ?").bind(tokenHash).run();
}
