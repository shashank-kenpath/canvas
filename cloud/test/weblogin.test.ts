import { env } from "cloudflare:test";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { resetKeyCache } from "../src/auth";
import { base64url } from "../src/util";
import { finishWebLogin, startWebLogin } from "../src/weblogin";

let keys: CryptoKeyPair;
let jwk: JsonWebKey;

beforeAll(async () => {
  keys = (await crypto.subtle.generateKey(
    { name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  )) as CryptoKeyPair;
  jwk = { ...((await crypto.subtle.exportKey("jwk", keys.publicKey)) as JsonWebKey), kid: "k1" } as JsonWebKey;
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetKeyCache();
});

async function idToken(claims: Record<string, unknown> = {}) {
  const enc = (v: unknown) => base64url(new TextEncoder().encode(JSON.stringify(v)));
  const head = enc({ alg: "RS256", kid: "k1" });
  const body = enc({
    iss: "https://accounts.google.com", aud: "web-client", exp: Math.floor(Date.now() / 1000) + 600,
    sub: "g-web-1", email: "wendy@example.com", email_verified: true, hd: "example.com", name: "Wendy", ...claims,
  });
  const sig = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", keys.privateKey, new TextEncoder().encode(`${head}.${body}`));
  return `${head}.${body}.${base64url(sig)}`;
}

function fakeGoogle(claims: Record<string, unknown> = {}) {
  const seen: Record<string, string>[] = [];
  vi.stubGlobal("fetch", async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.endsWith("/certs")) return Response.json({ keys: [jwk] });
    if (url.endsWith("/token")) {
      seen.push(Object.fromEntries(new URLSearchParams(String(init?.body))));
      return Response.json({ id_token: await idToken(claims) });
    }
    return new Response("unexpected", { status: 500 });
  });
  return seen;
}

const STATE = "client-state-0123456789";

async function googleState(query: string) {
  const res = await startWebLogin(env, new URL(`https://ink.test/auth/google?${query}`));
  expect(res.status).toBe(302);
  const to = new URL(res.headers.get("location")!);
  expect(to.origin + to.pathname).toBe("https://google.test/auth");
  expect(to.searchParams.get("client_id")).toBe("web-client");
  expect(to.searchParams.get("redirect_uri")).toBe("https://ink.test/auth/google/callback");
  return to.searchParams.get("state")!;
}

describe("browser sign-in", () => {
  it("brings the web app back signed in", async () => {
    const seen = fakeGoogle();
    const state = await googleState(`return=web&state=${STATE}`);
    const res = await finishWebLogin(env, new URL(`https://ink.test/auth/google/callback?code=c1&state=${encodeURIComponent(state)}`));
    const to = new URL(res.headers.get("location")!);
    expect(to.pathname).toBe("/app/");
    const hash = new URLSearchParams(to.hash.slice(1));
    expect(hash.get("state")).toBe(STATE);
    const token = hash.get("signin")!;
    expect(token.length).toBeGreaterThan(20);
    expect(seen[0].code).toBe("c1");
    const me = await (await import("cloudflare:test")).SELF.fetch("https://ink.test/api/me", { headers: { authorization: `Bearer ${token}` } });
    expect(((await me.json()) as { email: string }).email).toBe("wendy@example.com");
  });

  it("hands desktop apps the token on their loopback port", async () => {
    fakeGoogle();
    const state = await googleState(`return=app&port=53682&state=${STATE}`);
    const res = await finishWebLogin(env, new URL(`https://ink.test/auth/google/callback?code=c2&state=${encodeURIComponent(state)}`));
    const to = new URL(res.headers.get("location")!);
    expect(to.origin).toBe("http://127.0.0.1:53682");
    expect(to.searchParams.get("state")).toBe(STATE);
    expect(to.searchParams.get("token")).toBeTruthy();
  });

  it("refuses tampered state and accounts outside the organisation", async () => {
    fakeGoogle({ hd: "gmail.com", email: "x@gmail.com" });
    const state = await googleState(`return=web&state=${STATE}`);
    const forged = state.replace(/^./, (c) => (c === "a" ? "b" : "a"));
    await expect(finishWebLogin(env, new URL(`https://ink.test/auth/google/callback?code=c&state=${encodeURIComponent(forged)}`))).rejects.toThrow();
    const res = await finishWebLogin(env, new URL(`https://ink.test/auth/google/callback?code=c&state=${encodeURIComponent(state)}`));
    const hash = new URLSearchParams(new URL(res.headers.get("location")!).hash.slice(1));
    expect(hash.get("signin")).toBeNull();
    expect(hash.get("signin_error")).toMatch(/example\.com/);
    await expect(startWebLogin(env, new URL("https://ink.test/auth/google?return=app&port=80&state=" + STATE))).rejects.toThrow();
  });
});
