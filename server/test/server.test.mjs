import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { once } from 'node:events';
import { Client, StreamableHTTPClientTransport } from '@modelcontextprotocol/client';
import { createApp } from '../index.mjs';

const shape = (id, x = 0) => ({ id, tool: 'rectangle', points: [[x, 0], [x + 100, 60]], color: 0xff6750d8, width: 3, filled: false, text: '' });
async function launch(t, options = {}) {
  const root = await mkdtemp(join(tmpdir(), 'canvas-server-'));
  const args = { dbPath: join(root, 'rooms.sqlite'), downloads: root, ...options };
  const app = createApp(args); app.server.listen(0, '127.0.0.1'); await once(app.server, 'listening');
  const base = `http://127.0.0.1:${app.server.address().port}`;
  const close = () => new Promise(resolve => app.server.close(resolve));
  t.after(async () => { if (app.server.listening) await close(); await rm(root, { recursive: true, force: true }); });
  return { ...app, root, base, args, close };
}
async function request(base, path, { method = 'GET', token, body } = {}) {
  const response = await fetch(base + path, { method, headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...(body ? { 'content-type': 'application/json' } : {}) }, body: body ? JSON.stringify(body) : undefined });
  return { status: response.status, data: await response.json() };
}
test('Room auth, shape-level merge, deletion, invalid updates and persistence', async t => {
  const app = await launch(t);
  const created = await request(app.base, '/api/rooms', { method: 'POST', body: { name: 'Team ideas', shapes: [shape('a')] } });
  assert.equal(created.status, 201); const room = created.data;
  assert.equal(room.token.length, 43); assert.equal(room.revision, 1);
  assert.equal((await request(app.base, `/api/rooms/${room.roomId}`)).status, 401);
  assert.equal((await request(app.base, `/api/rooms/${room.roomId}`, { token: 'x'.repeat(43) })).status, 401);
  const apply = operations => request(app.base, `/api/rooms/${room.roomId}/ops`, { method: 'POST', token: room.token, body: { clientId: 'test', operations } });
  await Promise.all([apply([{ type: 'put', shape: shape('b', 100) }]), apply([{ type: 'put', shape: shape('c', 200) }])]);
  let current = await request(app.base, `/api/rooms/${room.roomId}`, { token: room.token });
  assert.equal(current.data.shapes.length, 3); assert.equal(current.data.revision, 3);
  await apply([{ type: 'delete', id: 'a' }]);
  assert.equal((await apply([{ type: 'put', shape: { ...shape('bad'), points: [[null, 0]] } }])).status, 400);
  await app.close();
  const restarted = createApp(app.args); restarted.server.listen(0, '127.0.0.1'); await once(restarted.server, 'listening');
  const base = `http://127.0.0.1:${restarted.server.address().port}`;
  current = await request(base, `/api/rooms/${room.roomId}`, { token: room.token });
  assert.deepEqual(current.data.shapes.map(s => s.id).sort(), ['b', 'c']); assert.equal(current.data.revision, 4);
  await new Promise(resolve => restarted.server.close(resolve));
});

test('Downloads are allowlisted and update feed is not cached', async t => {
  const app = await launch(t);
  await writeFile(join(app.root, 'update.json'), '{"versionCode":4}');
  await writeFile(join(app.root, 'secret.txt'), 'private');
  assert.equal((await fetch(app.base + '/secret.txt')).status, 404);
  assert.equal((await fetch(app.base + '/')).status, 404);
  const update = await fetch(app.base + '/update.json'); assert.equal(update.status, 200); assert.equal(update.headers.get('cache-control'), 'no-store');
  const crossOrigin = await fetch(app.base + '/health', { headers: { origin: 'https://untrusted.invalid' } }); assert.equal(crossOrigin.status, 403);
});

test('Official MCP client initializes, lists tools and edits only its room', async t => {
  const app = await launch(t);
  const first = (await request(app.base, '/api/rooms', { method: 'POST', body: { name: 'First board', shapes: [] } })).data;
  const other = (await request(app.base, '/api/rooms', { method: 'POST', body: { name: 'Other board', shapes: [] } })).data;
  const client = new Client({ name: 'canvas-integration-test', version: '1.0.0' });
  await client.connect(new StreamableHTTPClientTransport(new URL(`${app.base}/mcp/${first.roomId}/${first.token}`)));
  t.after(() => client.close());
  const tools = await client.listTools();
  assert.deepEqual(tools.tools.map(tool => tool.name).sort(), ['add_shapes', 'delete_shapes', 'get_board', 'list_shapes', 'update_shape']);
  const result = await client.callTool({ name: 'add_shapes', arguments: { shapes: [shape('claude')] } });
  assert.notEqual(result.isError, true);
  const list = await client.callTool({ name: 'list_shapes', arguments: {} });
  assert.equal(list.structuredContent.shapes[0].id, 'claude');
  assert.equal((await request(app.base, `/api/rooms/${other.roomId}`, { token: other.token })).data.shapes.length, 0);
  const denied = await fetch(`${app.base}/mcp/${first.roomId}/${other.token}`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{}' }); assert.equal(denied.status, 401);
});

test('Incremental sync: unchanged polls, acknowledged edits and contended edits', async t => {
  const app = await launch(t);
  const room = (await request(app.base, '/api/rooms', { method: 'POST', body: { name: 'Sync', shapes: [shape('a')] } })).data;
  const get = since => request(app.base, `/api/rooms/${room.roomId}${since == null ? '' : `?since=${since}`}`, { token: room.token });
  const ops = (since, operations) => request(app.base, `/api/rooms/${room.roomId}/ops`, { method: 'POST', token: room.token, body: { clientId: 'test', since, operations } });
  assert.deepEqual((await get(1)).data, { roomId: room.roomId, revision: 1, unchanged: true });
  assert.equal((await get(0)).data.shapes.length, 1);
  const text = { id: 't', tool: 'text', points: [[5, 5]], color: 0xff1d1d1d, width: 3, filled: false, text: 'नमस्ते', fontSize: 48 };
  assert.deepEqual((await ops(1, [{ type: 'put', shape: text }])).data, { roomId: room.roomId, revision: 2, ack: true });
  const stale = await ops(1, [{ type: 'put', shape: shape('b', 300) }]);
  assert.equal(stale.data.ack, undefined); assert.equal(stale.data.revision, 3);
  assert.deepEqual(stale.data.shapes.map(s => s.id), ['a', 't', 'b']);
  assert.equal(stale.data.shapes[1].fontSize, 48); assert.equal(stale.data.shapes[0].fontSize, undefined);
  assert.equal((await get(2)).data.shapes.length, 3);
});

test('Board codes join a room; wrong codes are rejected and rate limited', async t => {
  const app = await launch(t);
  const room = (await request(app.base, '/api/rooms', { method: 'POST', body: { name: 'Wall display', shapes: [shape('a')] } })).data;
  assert.equal((await request(app.base, `/api/rooms/${room.roomId}/code`, { method: 'POST', body: {} })).status, 401);
  const issued = await request(app.base, `/api/rooms/${room.roomId}/code`, { method: 'POST', token: room.token, body: {} });
  assert.match(issued.data.code, /^\d{6}$/); assert.equal(issued.data.expiresInSeconds, 600);
  const again = await request(app.base, `/api/rooms/${room.roomId}/code`, { method: 'POST', token: room.token, body: {} });
  assert.equal(again.data.code, issued.data.code);
  const joined = await request(app.base, '/api/join-code', { method: 'POST', body: { code: issued.data.code } });
  assert.equal(joined.status, 200); assert.equal(joined.data.token, room.token); assert.equal(joined.data.roomId, room.roomId);
  assert.equal(joined.data.name, 'Wall display'); assert.equal(joined.data.shapes.length, 1);
  const wrong = issued.data.code === '000000' ? '000001' : '000000';
  let last;
  for (let i = 0; i < 10; i++) last = await request(app.base, '/api/join-code', { method: 'POST', body: { code: wrong } });
  assert.equal(last.status, 429);
  const page = await fetch(app.base + '/pair'); assert.equal(page.status, 200);
  assert.match(page.headers.get('content-type'), /text\/html/); assert.match(await page.text(), /Enter the board code/);
});

test('MCP shapes may omit style fields and carry a font size', async t => {
  const app = await launch(t);
  const room = (await request(app.base, '/api/rooms', { method: 'POST', body: { name: 'Claude', shapes: [] } })).data;
  const client = new Client({ name: 'canvas-integration-test', version: '1.0.0' });
  await client.connect(new StreamableHTTPClientTransport(new URL(`${app.base}/mcp/${room.roomId}/${room.token}`)));
  t.after(() => client.close());
  const added = await client.callTool({ name: 'add_shapes', arguments: { shapes: [{ id: 'title', tool: 'text', points: [[40, 40]], text: 'Plan', fontSize: 40 }, { id: 'box', tool: 'rectangle', points: [[0, 0], [200, 120]] }] } });
  assert.notEqual(added.isError, true);
  const shapes = (await request(app.base, `/api/rooms/${room.roomId}`, { token: room.token })).data.shapes;
  assert.deepEqual(shapes[0], { id: 'title', tool: 'text', points: [[40, 40]], color: 0xff1d1d1d, width: 3.5, filled: false, text: 'Plan', fontSize: 40 });
  assert.equal(shapes[1].filled, false);
});
