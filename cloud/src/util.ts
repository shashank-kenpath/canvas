export function json(body: unknown, status = 200, headers: Record<string, string> = {}): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json", "cache-control": "no-store", ...headers },
  });
}

export function error(status: number, message: string, code = "error"): Response {
  return json({ error: message, code }, status);
}

export class HttpError extends Error {
  constructor(public status: number, message: string, public code = "error") {
    super(message);
  }
}

export function randomToken(bytes = 32): string {
  const data = crypto.getRandomValues(new Uint8Array(bytes));
  return base64url(data);
}

export function base64url(data: ArrayBuffer | Uint8Array): string {
  const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
  let s = "";
  for (const b of bytes) s += String.fromCharCode(b);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

export function fromBase64url(s: string): Uint8Array {
  const b = atob(s.replace(/-/g, "+").replace(/_/g, "/") + "===".slice((s.length + 3) % 4));
  const out = new Uint8Array(b.length);
  for (let i = 0; i < b.length; i++) out[i] = b.charCodeAt(i);
  return out;
}

export async function sha256Hex(data: string | ArrayBuffer | Uint8Array): Promise<string> {
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  const digest = await crypto.subtle.digest("SHA-256", bytes);
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export async function readJson<T = Record<string, unknown>>(req: Request, maxBytes = 64 << 10): Promise<T> {
  const text = await req.text();
  if (text.length > maxBytes) throw new HttpError(413, "Request too large");
  try {
    const v = JSON.parse(text || "{}");
    if (typeof v !== "object" || v === null || Array.isArray(v)) throw new Error();
    return v as T;
  } catch {
    throw new HttpError(400, "Expected a JSON object");
  }
}

// Cursor colours, chosen to stay readable on a white board.
const COLORS = ["#e03131", "#e8590c", "#f08c00", "#2f9e44", "#0c8599", "#1971c2", "#4263eb", "#7048e8", "#ae3ec9", "#d6336c", "#5c940d", "#495057"];

export function colorFor(id: string): string {
  let h = 2166136261;
  for (let i = 0; i < id.length; i++) {
    h ^= id.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return COLORS[(h >>> 0) % COLORS.length];
}

export const now = () => Date.now();
