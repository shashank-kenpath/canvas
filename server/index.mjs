import http from 'node:http';
import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { RoomStore, PairingCodes, ApiError, MAX_BODY_BYTES } from './store.mjs';
import { connectPage } from './page.mjs';
import { handleRoomMcp } from './mcp.mjs';

const defaultRoot = '/home/ubuntu/canvas-studio';
export function json(res, status, body) {
  const encoded = JSON.stringify(body);
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-store', 'X-Content-Type-Options': 'nosniff', 'Content-Length': Buffer.byteLength(encoded) });
  res.end(encoded);
}
async function readJson(req) {
  if (!(req.headers['content-type'] ?? '').toLowerCase().startsWith('application/json')) throw new ApiError(415, 'Use application/json.');
  if (Number(req.headers['content-length']) > MAX_BODY_BYTES) throw new ApiError(413, 'Request is too large.');
  let size = 0; const chunks = [];
  for await (const chunk of req) { size += chunk.length; if (size > MAX_BODY_BYTES) throw new ApiError(413, 'Request is too large.'); chunks.push(chunk); }
  try { return JSON.parse(Buffer.concat(chunks).toString('utf8')); }
  catch { throw new ApiError(400, 'Invalid JSON.'); }
}
function bearer(req) { return /^Bearer ([A-Za-z0-9_-]{43})$/.exec(req.headers.authorization ?? '')?.[1] ?? ''; }
const allowedDownload = name => /^(?:canvas-studio(?:-[A-Za-z0-9._-]+)?\.apk|update\.json|SHA256SUMS|TLDRAW-LICENSE\.md)$/.test(name);

export function createApp({ dbPath = process.env.DB_PATH ?? `${defaultRoot}/data/rooms.sqlite`, downloads = process.env.DOWNLOAD_DIR ?? `${defaultRoot}/downloads`, publicBase = process.env.PUBLIC_BASE_URL ?? 'https://annotated-remedies-property-binding.trycloudflare.com', store = new RoomStore(dbPath) } = {}) {
  const rate = new Map();
  const codes = new PairingCodes();
  const origins = new Set([new URL(publicBase).origin, 'http://localhost', 'http://127.0.0.1']);
  const rateLimit = (req, create, guess) => {
    const key = String(req.headers['cf-connecting-ip'] ?? req.socket.remoteAddress ?? 'unknown');
    const now = Date.now();
    let item = rate.get(key);
    if (!item || item.reset < now) { item = { count: 0, creates: 0, guesses: 0, reset: now + 60_000 }; rate.set(key, item); }
    if (++item.count > 600 || (create && ++item.creates > 10) || (guess && ++item.guesses > 10)) throw new ApiError(429, 'Too many requests. Please retry shortly.');
    if (rate.size > 10000) { for (const [ip, value] of rate) if (value.reset < now) rate.delete(ip); }
  };
  const server = http.createServer(async (req, res) => {
    try {
      const origin = req.headers.origin;
      if (origin && !origins.has(origin)) throw new ApiError(403, 'This browser origin is not allowed.');
      const url = new URL(req.url, 'http://localhost');
      if (url.pathname === '/health' && req.method === 'GET') return json(res, 200, { ok: true, service: 'canvas-studio' });
      if (req.method === 'GET' || req.method === 'HEAD') {
        const name = url.pathname.slice(1);
        if (allowedDownload(name)) {
          const path = resolve(downloads, name); let info;
          try { info = await stat(path); } catch { throw new ApiError(404, 'Download not found.'); }
          if (!info.isFile()) throw new ApiError(404, 'Download not found.');
          const type = name.endsWith('.apk') ? 'application/vnd.android.package-archive' : name.endsWith('.json') ? 'application/json' : 'text/plain; charset=utf-8';
          res.writeHead(200, { 'Content-Type': type, 'Content-Length': info.size, 'Cache-Control': name === 'update.json' ? 'no-store' : 'public, max-age=60', 'X-Content-Type-Options': 'nosniff' });
          if (req.method === 'HEAD') return res.end();
          const stream = createReadStream(path); stream.on('error', () => res.destroy()); res.on('close', () => stream.destroy()); return stream.pipe(res);
        }
      }
      if ((url.pathname === '/join' || url.pathname === '/pair') && req.method === 'GET') {
        res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8', 'Cache-Control': 'no-store', 'Referrer-Policy': 'no-referrer', 'X-Content-Type-Options': 'nosniff', 'X-Frame-Options': 'DENY', 'Content-Security-Policy': "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; connect-src 'self'; base-uri 'none'; form-action 'none'" });
        return res.end(connectPage);
      }
      const joining = url.pathname === '/api/join-code' && req.method === 'POST';
      rateLimit(req, url.pathname === '/api/rooms' && req.method === 'POST', joining);
      if (joining) {
        const { roomId, token } = codes.redeem((await readJson(req))?.code);
        store.authorize(roomId, token);
        return json(res, 200, { ...store.snapshot(roomId), token });
      }
      if (url.pathname === '/api/rooms' && req.method === 'POST') return json(res, 201, store.create(await readJson(req)));
      const room = /^\/api\/rooms\/([a-f0-9]{12})(\/ops|\/code)?$/.exec(url.pathname);
      if (room) {
        store.authorize(room[1], bearer(req));
        if (!room[2] && req.method === 'GET') {
          const since = Number(url.searchParams.get('since') ?? NaN);
          const revision = store.revision(room[1]);
          return json(res, 200, since === revision ? { roomId: room[1], revision, unchanged: true } : store.snapshot(room[1]));
        }
        if (room[2] === '/code' && req.method === 'POST') { req.resume(); return json(res, 200, codes.issue(room[1], bearer(req))); }
        if (room[2] === '/ops' && req.method === 'POST') return json(res, 200, store.apply(room[1], await readJson(req)));
        throw new ApiError(405, 'Method not allowed.');
      }
      const mcp = /^\/mcp\/([a-f0-9]{12})\/([A-Za-z0-9_-]{43})$/.exec(url.pathname);
      if (mcp) {
        store.authorize(mcp[1], mcp[2]);
        if (req.method !== 'POST') { res.setHeader('Allow', 'POST'); throw new ApiError(405, 'Use the Streamable HTTP POST transport.'); }
        return await handleRoomMcp(store, mcp[1], req, res, await readJson(req));
      }
      throw new ApiError(404, 'Not found.');
    } catch (error) {
      if (res.headersSent) { res.destroy(); return; }
      json(res, error.status ?? 500, { error: error instanceof ApiError ? error.message : 'Request failed.' });
    }
  });
  server.requestTimeout = 30000; server.headersTimeout = 15000;
  server.on('close', () => store.close());
  return { server, store };
}
if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  const { server } = createApp();
  const port = Number(process.env.PORT ?? 18763);
  server.listen(port, '127.0.0.1', () => console.log(`Canvas Studio collaboration listening on loopback port ${port}`));
  for (const signal of ['SIGTERM', 'SIGINT']) process.on(signal, () => server.close(() => process.exit(0)));
}
