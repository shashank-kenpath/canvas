// Inkboard relay: lets a panel behind a home or school router be reached
// from the internet. The panel keeps an outbound long-poll open; requests
// to /d/<device>/<path> are queued for it and its answers are passed back.
// Used for the public Claude (MCP) address and for joining shared boards
// from another network. Holds nothing on disk.
//
//   node server/relay.mjs            (listens on 127.0.0.1:18780)
//   RELAY_PORT=18780 RELAY_HOST=127.0.0.1

import http from 'node:http';
import crypto from 'node:crypto';

const PORT = Number(process.env.RELAY_PORT || 18780);
const HOST = process.env.RELAY_HOST || '127.0.0.1';
const MAX_BODY = 8 << 20;
const POLL_MS = 25_000;
const ANSWER_MS = 20_000;
const OFFLINE_MS = 60_000;
const MAX_QUEUE = 64;
const MAX_DEVICES = 5000;

/** id -> { hash, codes:Set, seen, queue:[], waiter, pending:Map(rid->res) } */
const devices = new Map();
let nextRid = 1;

const hash = (s) => crypto.createHash('sha256').update(String(s)).digest('hex');
const validId = (s) => typeof s === 'string' && /^[a-z0-9]{8,32}$/.test(s);

function send(res, status, body, type = 'application/json') {
  const data = Buffer.isBuffer(body) ? body : Buffer.from(typeof body === 'string' ? body : JSON.stringify(body));
  res.writeHead(status, {
    'content-type': type,
    'content-length': data.length,
    'access-control-allow-origin': '*',
    'access-control-allow-headers': '*',
    'access-control-allow-methods': 'GET, POST, DELETE, OPTIONS',
    'access-control-expose-headers': 'mcp-session-id',
    'cache-control': 'no-store',
  });
  res.end(data);
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    const parts = [];
    let size = 0;
    req.on('data', (c) => {
      size += c.length;
      if (size > MAX_BODY) { reject(new Error('too large')); req.destroy(); return; }
      parts.push(c);
    });
    req.on('end', () => resolve(Buffer.concat(parts)));
    req.on('error', reject);
  });
}

/** Finds or claims a device slot; the first secret seen for an id owns it. */
function device(id, secret) {
  if (!validId(id) || typeof secret !== 'string' || secret.length < 16) return null;
  let d = devices.get(id);
  if (!d) {
    if (devices.size >= MAX_DEVICES) prune(true);
    d = { hash: hash(secret), codes: new Set(), seen: Date.now(), queue: [], waiter: null, pending: new Map() };
    devices.set(id, d);
  }
  if (d.hash !== hash(secret)) return null;
  d.seen = Date.now();
  return d;
}

function prune(force = false) {
  const now = Date.now();
  for (const [id, d] of devices) {
    if (now - d.seen > (force ? OFFLINE_MS : 24 * 3600_000) && !d.waiter && d.pending.size === 0) devices.delete(id);
  }
}
setInterval(prune, 3600_000).unref();

function flush(d) {
  if (!d.waiter || d.queue.length === 0) return;
  const { res, timer } = d.waiter;
  d.waiter = null;
  clearTimeout(timer);
  send(res, 200, d.queue.splice(0));
}

const online = (d) => d && (d.waiter || Date.now() - d.seen < OFFLINE_MS);

async function handle(req, res) {
  const url = new URL(req.url, 'http://relay');
  const path = url.pathname;
  if (req.method === 'OPTIONS') return send(res, 204, '');

  // ---- panel side
  if (path === '/_dev/poll' && req.method === 'POST') {
    const d = device(url.searchParams.get('id'), url.searchParams.get('secret'));
    if (!d) return send(res, 403, { error: 'Not allowed' });
    const codes = (url.searchParams.get('codes') || '').split(',').filter((c) => /^\d{6}$/.test(c));
    d.codes = new Set(codes.slice(0, 8));
    if (d.waiter) { clearTimeout(d.waiter.timer); send(d.waiter.res, 200, []); }
    const timer = setTimeout(() => { if (d.waiter?.res === res) { d.waiter = null; d.seen = Date.now(); send(res, 200, []); } }, POLL_MS);
    d.waiter = { res, timer };
    res.on('close', () => { if (d.waiter?.res === res) { clearTimeout(timer); d.waiter = null; } });
    return flush(d);
  }
  if (path === '/_dev/reply' && req.method === 'POST') {
    const d = device(url.searchParams.get('id'), url.searchParams.get('secret'));
    if (!d) return send(res, 403, { error: 'Not allowed' });
    let replies;
    try { replies = JSON.parse((await readBody(req)).toString('utf8')); } catch { return send(res, 400, { error: 'Bad reply' }); }
    for (const r of Array.isArray(replies) ? replies : []) {
      const waiting = d.pending.get(r.rid);
      if (!waiting) continue;
      d.pending.delete(r.rid);
      clearTimeout(waiting.timer);
      const body = Buffer.from(String(r.body || ''), 'base64');
      if (r.status === 202 || r.status === 204) { waiting.res.writeHead(r.status, { 'access-control-allow-origin': '*' }); waiting.res.end(); }
      else send(waiting.res, Number(r.status) || 502, body, String(r.ctype || 'application/json'));
    }
    return send(res, 200, { ok: true });
  }

  // ---- public side
  if (path === '/find' && req.method === 'GET') {
    const code = url.searchParams.get('code') || '';
    if (!/^\d{6}$/.test(code)) return send(res, 400, { error: 'Bad code' });
    for (const [id, d] of devices) if (d.codes.has(code) && online(d)) return send(res, 200, { id });
    return send(res, 404, { error: 'No board is shared with that code right now' });
  }
  const m = path.match(/^\/d\/([a-z0-9]{8,32})(\/.*)$/);
  if (m) {
    const d = devices.get(m[1]);
    if (!online(d)) return send(res, 503, { error: 'The Inkboard panel is offline. Open the app on the panel.' });
    if (d.queue.length + d.pending.size >= MAX_QUEUE) return send(res, 503, { error: 'Busy, try again' });
    let body;
    try { body = await readBody(req); } catch { return send(res, 413, { error: 'Too large' }); }
    const rid = nextRid++;
    const query = url.search.startsWith('?') ? url.search.slice(1) : '';
    const timer = setTimeout(() => { if (d.pending.delete(rid)) send(res, 504, { error: 'The panel did not answer in time' }); }, ANSWER_MS);
    d.pending.set(rid, { res, timer });
    res.on('close', () => { if (d.pending.get(rid)?.res === res) { clearTimeout(timer); d.pending.delete(rid); } });
    d.queue.push({ rid, method: req.method, path: m[2], query, body: body.toString('base64') });
    return flush(d);
  }
  if (path === '/' || path === '/health') return send(res, 200, 'Inkboard relay\n', 'text/plain');
  return send(res, 404, { error: 'Not found' });
}

http.createServer((req, res) => {
  handle(req, res).catch(() => { if (!res.headersSent) send(res, 500, { error: 'Relay error' }); });
}).listen(PORT, HOST, () => console.log(`inkboard relay on http://${HOST}:${PORT}`));
