import { env } from "cloudflare:test";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { pollDeviceLogin, resetKeyCache, startDeviceLogin, verifyIdToken } from "../src/auth";
import { base64url } from "../src/util";

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

async function idToken(claims: Record<string, unknown>) {
  const enc = (v: unknown) => base64url(new TextEncoder().encode(JSON.stringify(v)));
  const head = enc({ alg: "RS256", kid: "k1" });
  const body = enc({
    iss: "https://accounts.google.com",
    aud: "test-client",
    exp: Math.floor(Date.now() / 1000) + 600,
    sub: "g-123",
    email: "ana@example.com",
    email_verified: true,
    hd: "example.com",
    name: "Ana",
    ...claims,
  });
  const sig = await crypto.subtle.sign("RSASSA-PKCS1-v1_5", keys.privateKey, new TextEncoder().encode(`${head}.${body}`));
  return `${head}.${body}.${base64url(sig)}`;
}

function fakeGoogle(token: () => Promise<Record<string, unknown>>) {
  vi.stubGlobal("fetch", async (input: RequestInfo | URL) => {
    const url = String(input instanceof Request ? input.url : input);
    if (url.endsWith("/certs")) return Response.json({ keys: [jwk] });
    if (url.endsWith("/device/code")) {
      return Response.json({ device_code: "dev-1", user_code: "ABCD-EFGH", verification_url: "https://www.google.com/device", expires_in: 1800, interval: 5 });
    }
    if (url.endsWith("/token")) {
      const body = await token();
      return Response.json(body, { status: body.error ? 428 : 200 });
    }
    return new Response("unexpected", { status: 500 });
  });
}

describe("Google Workspace sign-in", () => {
  it("accepts an organisation account", async () => {
    fakeGoogle(async () => ({}));
    const c = await verifyIdToken(env, await idToken({}));
    expect(c.email).toBe("ana@example.com");
  });

  it("refuses personal Gmail and other organisations", async () => {
    fakeGoogle(async () => ({}));
    await expect(verifyIdToken(env, await idToken({ hd: undefined, email: "ana@gmail.com" }))).rejects.toMatchObject({ status: 403 });
    await expect(verifyIdToken(env, await idToken({ hd: "other.com", email: "ana@other.com" }))).rejects.toMatchObject({ status: 403 });
  });

  it("refuses tokens for another app or with a bad signature", async () => {
    fakeGoogle(async () => ({}));
    await expect(verifyIdToken(env, await idToken({ aud: "someone-else" }))).rejects.toMatchObject({ status: 401 });
    const good = await idToken({});
    const forged = good.slice(0, good.lastIndexOf(".") + 1) + base64url(new Uint8Array(256));
    await expect(verifyIdToken(env, forged)).rejects.toMatchObject({ status: 401 });
  });

  it("completes the device flow and creates a session", async () => {
    let approved = false;
    fakeGoogle(async () => (approved ? { id_token: await idToken({ sub: "g-flow", email: "flow@example.com" }) } : { error: "authorization_pending" }));
    const start = await startDeviceLogin(env, "Pixel Tablet");
    expect(start.userCode).toBe("ABCD-EFGH");
    expect((await pollDeviceLogin(env, start.handle)).status).toBe("pending");
    // Polling faster than Google allows is answered locally.
    expect((await pollDeviceLogin(env, start.handle)).status).toBe("pending");
    await env.DB.prepare("UPDATE device_logins SET next_poll_at = 0").run();
    approved = true;
    const done = await pollDeviceLogin(env, start.handle);
    expect(done.status).toBe("done");
    if (done.status === "done") {
      expect(done.user.email).toBe("flow@example.com");
      expect(done.token.length).toBeGreaterThan(30);
    }
    // The handle is single use.
    await expect(pollDeviceLogin(env, start.handle)).rejects.toMatchObject({ status: 410 });
  });
});
