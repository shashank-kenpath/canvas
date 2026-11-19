// "Sign in with Google" in a browser, for the web app and the desktop apps.
// The page (or the desktop app, through the system browser) opens
// /auth/google; Google sends the person back to /auth/google/callback; the
// Worker swaps the code for an ID token, checks it like any other sign-in and
// hands a session token back:
//   web:     redirect to /app/#signin=<token>&state=<state>
//   desktop: redirect to http://127.0.0.1:<port>/callback?token=<token>&state=<state>
// `state` comes from the caller and is checked by it, so a token can only land
// where the sign-in started. The Worker's own round-trip state is signed.

import { createSession, upsertUser, verifyIdToken } from "./auth";
import type { Env } from "./env";
import { HttpError, base64url, fromBase64url, now } from "./util";

const STATE_TTL_MS = 15 * 60_000;

interface Trip {
  /** "web", or "app" for a desktop app listening on 127.0.0.1:port. */
  r: "web" | "app";
  p?: number;
  /** The caller's state, handed back untouched. */
  s: string;
  t: number;
}

function authUrl(env: Env): string {
  return env.GOOGLE_BASE ? env.GOOGLE_BASE.replace(/\/$/, "") + "/auth" : "https://accounts.google.com/o/oauth2/v2/auth";
}

function tokenUrl(env: Env): string {
  return env.GOOGLE_BASE ? env.GOOGLE_BASE.replace(/\/$/, "") + "/token" : "https://oauth2.googleapis.com/token";
}

function webConfigured(env: Env) {
  if (!env.GOOGLE_WEB_CLIENT_ID || !env.GOOGLE_WEB_CLIENT_SECRET || !env.ALLOWED_DOMAINS) {
    throw new HttpError(503, "Browser sign-in is not set up on this server yet", "not_configured");
  }
}

async function hmac(env: Env, data: string): Promise<string> {
  const key = await crypto.subtle.importKey("raw", new TextEncoder().encode(env.GOOGLE_WEB_CLIENT_SECRET!), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return base64url(await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(data)));
}

async function seal(env: Env, trip: Trip): Promise<string> {
  const body = base64url(new TextEncoder().encode(JSON.stringify(trip)));
  return `${body}.${await hmac(env, body)}`;
}

async function open(env: Env, state: string): Promise<Trip | null> {
  const [body, sig] = state.split(".");
  if (!body || !sig || sig !== (await hmac(env, body))) return null;
  try {
    const trip = JSON.parse(new TextDecoder().decode(fromBase64url(body))) as Trip;
    return now() - trip.t < STATE_TTL_MS ? trip : null;
  } catch {
    return null;
  }
}

function callbackUrl(url: URL): string {
  return `${url.origin}/auth/google/callback`;
}

/** Sends where the caller asked: the web app, or a desktop app's loopback port. */
function back(url: URL, trip: Pick<Trip, "r" | "p" | "s">, result: { token: string } | { error: string }): Response {
  if (trip.r === "app" && trip.p) {
    const q = new URLSearchParams({ state: trip.s, ...("token" in result ? { token: result.token } : { error: result.error }) });
    return Response.redirect(`http://127.0.0.1:${trip.p}/callback?${q}`, 302);
  }
  const q = new URLSearchParams({ state: trip.s, ...("token" in result ? { signin: result.token } : { signin_error: result.error }) });
  // A fragment never reaches a server, so the token stays out of logs.
  return Response.redirect(`${url.origin}/app/#${q}`, 302);
}

/** GET /auth/google?return=web|app&port=&state= */
export async function startWebLogin(env: Env, url: URL): Promise<Response> {
  webConfigured(env);
  const r = url.searchParams.get("return") === "app" ? "app" : "web";
  const port = Number(url.searchParams.get("port"));
  if (r === "app" && !(Number.isInteger(port) && port >= 1024 && port <= 65535)) throw new HttpError(400, "Bad port");
  const s = (url.searchParams.get("state") ?? "").slice(0, 128);
  if (!/^[\w-]{16,128}$/.test(s)) throw new HttpError(400, "Bad state");
  const state = await seal(env, { r, p: r === "app" ? port : undefined, s, t: now() });
  const q = new URLSearchParams({
    client_id: env.GOOGLE_WEB_CLIENT_ID!,
    redirect_uri: callbackUrl(url),
    response_type: "code",
    scope: "openid email profile",
    prompt: "select_account",
    state,
  });
  return Response.redirect(`${authUrl(env)}?${q}`, 302);
}

/** GET /auth/google/callback?code=&state= (from Google) */
export async function finishWebLogin(env: Env, url: URL): Promise<Response> {
  webConfigured(env);
  const trip = await open(env, url.searchParams.get("state") ?? "");
  if (!trip) throw new HttpError(400, "This sign-in link has expired. Start again from Inkboard.", "expired");
  const code = url.searchParams.get("code");
  if (!code) return back(url, trip, { error: url.searchParams.get("error") === "access_denied" ? "Sign-in was cancelled" : "Google sign-in failed" });
  try {
    const res = await fetch(tokenUrl(env), {
      method: "POST",
      headers: { "content-type": "application/x-www-form-urlencoded" },
      body: new URLSearchParams({
        client_id: env.GOOGLE_WEB_CLIENT_ID!,
        client_secret: env.GOOGLE_WEB_CLIENT_SECRET!,
        code,
        redirect_uri: callbackUrl(url),
        grant_type: "authorization_code",
      }),
    });
    const g = (await res.json().catch(() => ({}))) as { id_token?: string };
    if (!res.ok || !g.id_token) throw new HttpError(502, "Google sign-in failed", "google");
    const claims = await verifyIdToken(env, g.id_token);
    const user = await upsertUser(env, claims);
    const token = await createSession(env, user.id, trip.r === "app" ? "Inkboard desktop" : "Inkboard on the web");
    return back(url, trip, { token });
  } catch (e) {
    return back(url, trip, { error: e instanceof HttpError ? e.message : "Google sign-in failed" });
  }
}
