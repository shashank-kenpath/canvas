import { DatabaseSync } from 'node:sqlite';
import { randomBytes, randomInt, createHash, timingSafeEqual } from 'node:crypto';
import { mkdirSync } from 'node:fs';
import { dirname } from 'node:path';
import { z } from 'zod';

export const MAX_BODY_BYTES = 8 * 1024 * 1024;
export const MAX_SHAPES = 5000;
export class ApiError extends Error {
  constructor(status, message) { super(message); this.status = status; }
}
const point = z.tuple([z.number().finite().min(-1e8).max(1e8), z.number().finite().min(-1e8).max(1e8)]);
export const shapeSchema = z.object({
  id: z.string().min(1).max(128),
  tool: z.enum(['pen', 'rectangle', 'ellipse', 'arrow', 'text']),
  points: z.array(point).min(1).max(20000),
  color: z.number().int().min(0).max(0xffffffff).default(0xff1d1d1d),
  width: z.number().finite().positive().max(1000).default(3.5),
  filled: z.boolean().default(false),
  text: z.string().max(20000).default(''),
  fontSize: z.number().finite().min(1).max(4000).optional(),
});
export const operationSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('put'), shape: shapeSchema }),
  z.object({ type: z.literal('delete'), id: z.string().min(1).max(128) }),
]);
const tokenHash = token => createHash('sha256').update(token).digest();
function parse(schema, input) {
  const result = schema.safeParse(input);
  if (!result.success) throw new ApiError(400, 'Invalid board data. Check shape coordinates and limits.');
  return result.data;
}
function encodeShapes(shapes) {
  if (shapes.length > MAX_SHAPES) throw new ApiError(413, `A shared board supports at most ${MAX_SHAPES} shapes.`);
  const encoded = JSON.stringify(shapes);
  if (Buffer.byteLength(encoded) > MAX_BODY_BYTES) throw new ApiError(413, 'This shared board is too large.');
  return encoded;
}

export class RoomStore {
  constructor(path) {
    if (path !== ':memory:') mkdirSync(dirname(path), { recursive: true, mode: 0o700 });
    this.db = new DatabaseSync(path);
    this.db.exec('PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; CREATE TABLE IF NOT EXISTS rooms (id TEXT PRIMARY KEY, token_hash BLOB NOT NULL, name TEXT NOT NULL, revision INTEGER NOT NULL, shapes TEXT NOT NULL, updated INTEGER NOT NULL)');
  }
  create(input) {
    const data = parse(z.object({ name: z.string().trim().min(1).max(120), shapes: z.array(shapeSchema).max(MAX_SHAPES) }), input);
    const ids = new Set(data.shapes.map(s => s.id));
    if (ids.size !== data.shapes.length) throw new ApiError(400, 'Shape IDs must be unique.');
    const encoded = encodeShapes(data.shapes);
    if (this.db.prepare('SELECT count(*) AS count FROM rooms').get().count >= 1000) throw new ApiError(503, 'Shared-board capacity reached.');
    const roomId = randomBytes(6).toString('hex');
    const token = randomBytes(32).toString('base64url');
    this.db.prepare('INSERT INTO rooms VALUES (?, ?, ?, ?, ?, ?)').run(roomId, tokenHash(token), data.name, 1, encoded, Date.now());
    return { ...this.snapshot(roomId), token };
  }
  authorize(roomId, token) {
    if (typeof token !== 'string' || token.length > 256) throw new ApiError(401, 'A valid room invitation is required.');
    const row = this.db.prepare('SELECT token_hash FROM rooms WHERE id = ?').get(roomId);
    const expected = row ? Buffer.from(row.token_hash) : Buffer.alloc(32);
    const valid = timingSafeEqual(expected, tokenHash(token));
    if (!row || !valid) throw new ApiError(401, 'A valid room invitation is required.');
  }
  revision(roomId) {
    const row = this.db.prepare('SELECT revision FROM rooms WHERE id = ?').get(roomId);
    if (!row) throw new ApiError(404, 'Shared board not found.');
    return row.revision;
  }
  snapshot(roomId) {
    const row = this.db.prepare('SELECT id,name,revision,shapes FROM rooms WHERE id = ?').get(roomId);
    if (!row) throw new ApiError(404, 'Shared board not found.');
    return { roomId: row.id, name: row.name, revision: row.revision, shapes: JSON.parse(row.shapes) };
  }
  apply(roomId, input) {
    const data = parse(z.object({ clientId: z.string().min(1).max(128), since: z.number().int().nonnegative().optional(), operations: z.array(operationSchema).min(1).max(200) }), input);
    this.db.exec('BEGIN IMMEDIATE');
    try {
      const current = this.snapshot(roomId);
      const shapes = new Map(current.shapes.map(s => [s.id, s]));
      for (const operation of data.operations) {
        if (operation.type === 'delete') shapes.delete(operation.id);
        else shapes.set(operation.shape.id, operation.shape);
      }
      const encoded = encodeShapes([...shapes.values()]);
      this.db.prepare('UPDATE rooms SET revision=revision+1, shapes=?, updated=? WHERE id=?').run(encoded, Date.now(), roomId);
      this.db.exec('COMMIT');
      // A client that was already up to date knows the result of its own edit.
      if (data.since === current.revision) return { roomId, revision: current.revision + 1, ack: true };
      return { roomId, name: current.name, revision: current.revision + 1, shapes: [...shapes.values()] };
    } catch (error) { this.db.exec('ROLLBACK'); throw error; }
  }
  close() { this.db.close(); }
}

/** Short-lived numeric codes that let another device join without the long invite link. */
export class PairingCodes {
  constructor(lifetime = 10 * 60_000) { this.lifetime = lifetime; this.codes = new Map(); this.failures = { count: 0, reset: 0 }; }
  issue(roomId, token, now = Date.now()) {
    for (const [code, item] of this.codes) {
      if (item.expires <= now) this.codes.delete(code);
      else if (item.roomId === roomId && item.token === token && item.expires - now > this.lifetime / 2) return { code, expiresInSeconds: Math.floor((item.expires - now) / 1000) };
    }
    if (this.codes.size >= 2000) throw new ApiError(503, 'Too many active board codes. Use the invite link.');
    let code;
    do { code = String(randomInt(0, 1_000_000)).padStart(6, '0'); } while (this.codes.has(code));
    this.codes.set(code, { roomId, token, expires: now + this.lifetime });
    return { code, expiresInSeconds: Math.floor(this.lifetime / 1000) };
  }
  redeem(code, now = Date.now()) {
    if (this.failures.reset < now) this.failures = { count: 0, reset: now + 60_000 };
    // Bounds guessing across all addresses, not only per address.
    if (this.failures.count >= 200) throw new ApiError(429, 'Too many attempts. Please retry shortly.');
    const item = typeof code === 'string' ? this.codes.get(code) : undefined;
    if (!item || item.expires <= now) { this.failures.count++; if (item) this.codes.delete(code); throw new ApiError(404, 'This board code is not valid.'); }
    return { roomId: item.roomId, token: item.token };
  }
}
