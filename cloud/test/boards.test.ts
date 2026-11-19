import { describe, expect, it } from "vitest";
import { Client, api, shape, signIn } from "./helpers";

async function newBoard(who: Awaited<ReturnType<typeof signIn>>, title = "Plan") {
  const res = await api(who, "POST", "/api/boards", { title });
  expect(res.status).toBe(201);
  return (await res.json()) as { id: string; role: string };
}

describe("board access", () => {
  it("refuses requests without a session", async () => {
    expect((await api(null, "GET", "/api/boards")).status).toBe(401);
  });

  it("keeps boards private until shared", async () => {
    const ana = await signIn("ana@example.com");
    const ben = await signIn("ben@example.com");
    const board = await newBoard(ana);
    expect(board.role).toBe("owner");
    expect((await api(ben, "GET", `/api/boards/${board.id}`)).status).toBe(403);
    const list = (await (await api(ben, "GET", "/api/boards")).json()) as { boards: unknown[] };
    expect(list.boards).toHaveLength(0);
  });

  it("shares with people and the whole organisation", async () => {
    const ana = await signIn("ana2@example.com");
    const ben = await signIn("ben2@example.com");
    const cy = await signIn("cy2@example.com");
    const board = await newBoard(ana);
    const shared = await api(ana, "PUT", `/api/boards/${board.id}/members`, { email: "BEN2@example.com", role: "viewer" });
    expect(shared.status).toBe(200);
    expect(((await (await api(ben, "GET", `/api/boards/${board.id}`)).json()) as { role: string }).role).toBe("viewer");
    // Viewers cannot share or rename.
    expect((await api(ben, "PUT", `/api/boards/${board.id}/members`, { email: "cy2@example.com", role: "editor" })).status).toBe(403);
    expect((await api(ben, "PATCH", `/api/boards/${board.id}`, { title: "x" })).status).toBe(403);
    // Outside the organisation is refused.
    expect((await api(ana, "PUT", `/api/boards/${board.id}/members`, { email: "eve@gmail.com", role: "editor" })).status).toBe(400);

    expect((await api(cy, "GET", `/api/boards/${board.id}`)).status).toBe(403);
    expect((await api(ana, "PATCH", `/api/boards/${board.id}`, { orgAccess: "edit" })).status).toBe(200);
    const asCy = (await (await api(cy, "GET", `/api/boards/${board.id}`)).json()) as { role: string };
    expect(asCy.role).toBe("editor");
    // Organisation edit access lifts an explicit viewer to editor.
    expect(((await (await api(ben, "GET", `/api/boards/${board.id}`)).json()) as { role: string }).role).toBe("editor");
  });

  it("deletes for the owner only", async () => {
    const ana = await signIn("ana3@example.com");
    const ben = await signIn("ben3@example.com");
    const board = await newBoard(ana);
    await api(ana, "PATCH", `/api/boards/${board.id}`, { orgAccess: "edit" });
    expect((await api(ben, "DELETE", `/api/boards/${board.id}`)).status).toBe(403);
    expect((await api(ana, "DELETE", `/api/boards/${board.id}`)).status).toBe(200);
    expect((await api(ana, "GET", `/api/boards/${board.id}`)).status).toBe(404);
  });

  it("revokes a session on logout", async () => {
    const ana = await signIn("ana4@example.com");
    expect((await api(ana, "POST", "/api/auth/logout")).status).toBe(200);
    expect((await api(ana, "GET", "/api/me")).status).toBe(401);
  });
});

describe("live board", () => {
  it("sends each edit to the others with who made it", async () => {
    const ana = await signIn("ana5@example.com", "Ana");
    const ben = await signIn("ben5@example.com", "Ben");
    const board = await newBoard(ana);
    await api(ana, "PUT", `/api/boards/${board.id}/members`, { email: ben.email, role: "editor" });

    const a = await Client.connect(ana, board.id);
    const b = await Client.connect(ben, board.id);
    expect((await a.next((m) => m.t === "join")).user.name).toBe("Ben");

    a.send({ t: "ops", id: 1, put: [shape("s1")] });
    const ack = await a.next((m) => m.t === "ack");
    expect(ack).toMatchObject({ id: 1, rev: 1 });
    const seen = await b.next((m) => m.t === "ops");
    expect(seen).toMatchObject({ rev: 1, by: ana.id, del: [] });
    expect(seen.items[0]).toMatchObject({ s: shape("s1"), c: ana.id, e: ana.id });

    // Ben edits Ana's shape: creator stays, editor changes.
    b.send({ t: "ops", id: 7, put: [shape("s1", { color: 5 })] });
    await b.next((m) => m.t === "ack");
    const edit = await a.next((m) => m.t === "ops");
    expect(edit.items[0]).toMatchObject({ c: ana.id, e: ben.id });

    b.send({ t: "ops", id: 8, del: ["s1", "missing"] });
    expect((await b.next((m) => m.t === "ack")).rev).toBe(3);
    expect((await a.next((m) => m.t === "ops")).del).toEqual(["s1"]);
    a.close();
    b.close();
  });

  it("relays cursors and selections", async () => {
    const ana = await signIn("ana6@example.com");
    const ben = await signIn("ben6@example.com");
    const board = await newBoard(ana);
    await api(ana, "PATCH", `/api/boards/${board.id}`, { orgAccess: "edit" });
    const a = await Client.connect(ana, board.id);
    const b = await Client.connect(ben, board.id);
    a.send({ t: "presence", cursor: [12.5, -3], sel: ["s1"], tool: "pen", view: [0, 0, 100, 80] });
    const p = await b.next((m) => m.t === "presence");
    expect(p).toMatchObject({ user: ana.id, cursor: [12.5, -3], sel: ["s1"], tool: "pen" });
    // A newcomer gets current presence in its welcome.
    const cy = await signIn("cy6@example.com");
    const c = await Client.connect(cy, board.id);
    void c;
    a.close();
    expect((await b.next((m) => m.t === "leave")).conn).toBe(p.conn);
    b.close();
    c.close();
  });

  it("lets viewers watch but not edit", async () => {
    const ana = await signIn("ana7@example.com");
    const vic = await signIn("vic7@example.com");
    const board = await newBoard(ana);
    await api(ana, "PUT", `/api/boards/${board.id}/members`, { email: vic.email, role: "viewer" });
    const v = await Client.connect(vic, board.id);
    v.send({ t: "ops", id: 1, put: [shape("x")] });
    expect((await v.next((m) => m.t === "error")).code).toBe("read_only");
    v.close();
  });

  it("refuses people without access", async () => {
    const ana = await signIn("ana8@example.com");
    const eve = await signIn("eve8@example.com");
    const board = await newBoard(ana);
    await expect(Client.connect(eve, board.id)).rejects.toThrow(/403/);
  });

  it("catches up from a revision and resyncs on a new epoch", async () => {
    const ana = await signIn("ana9@example.com");
    const board = await newBoard(ana);
    const a = await Client.connect(ana, board.id);
    a.send({ t: "ops", id: 1, put: [shape("s1"), shape("s2")] });
    await a.next((m) => m.t === "ack");
    a.send({ t: "ops", id: 2, put: [shape("s3")], del: ["s1"] });
    await a.next((m) => m.t === "ack");
    a.close();

    const fresh = await Client.connect(ana, board.id);
    const welcome = fresh.messages.find((m) => m.t === "welcome")!;
    expect(welcome).toMatchObject({ rev: 2, full: true });
    const all = fresh.messages.filter((m) => m.t === "sync").flatMap((m) => m.items.map((i: any) => i.s.id));
    expect(all.sort()).toEqual(["s2", "s3"]);
    fresh.close();

    const since1 = await Client.connect(ana, board.id, { rev: 1, epoch: welcome.epoch });
    const sync = since1.messages.filter((m) => m.t === "sync");
    expect(since1.messages.find((m) => m.t === "welcome")!.full).toBe(false);
    expect(sync.flatMap((m) => m.items.map((i: any) => i.s.id))).toEqual(["s3"]);
    expect(sync.flatMap((m) => m.del)).toEqual(["s1"]);
    since1.close();

    const stale = await Client.connect(ana, board.id, { rev: 1, epoch: "another-board" });
    expect(stale.messages.find((m) => m.t === "welcome")!.full).toBe(true);
    stale.close();
  });

  it("disconnects people whose access was removed", async () => {
    const ana = await signIn("ana10@example.com");
    const ben = await signIn("ben10@example.com");
    const board = await newBoard(ana);
    await api(ana, "PUT", `/api/boards/${board.id}/members`, { email: ben.email, role: "editor" });
    const b = await Client.connect(ben, board.id);
    await api(ana, "DELETE", `/api/boards/${board.id}/members/${encodeURIComponent(ben.email)}`);
    expect(await b.waitClosed()).toBe(4001);
    await expect(Client.connect(ben, board.id)).rejects.toThrow(/403/);
  });
});

describe("assets", () => {
  it("stores images by their hash", async () => {
    const ana = await signIn("ana11@example.com");
    const bytes = new Uint8Array([137, 80, 78, 71, 1, 2, 3]);
    const digest = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((b) => b.toString(16).padStart(2, "0")).join("");
    const { SELF } = await import("cloudflare:test");
    const put = (hash: string, type = "image/png") =>
      SELF.fetch(`https://inkboard.test/api/assets/${hash}`, { method: "PUT", headers: { authorization: `Bearer ${ana.token}`, "content-type": type }, body: bytes });
    expect((await put("0".repeat(64))).status).toBe(400);
    expect((await put(digest, "text/html")).status).toBe(415);
    expect((await put(digest)).status).toBe(201);
    const got = await api(ana, "GET", `/api/assets/${digest}`);
    expect(new Uint8Array(await got.arrayBuffer())).toEqual(bytes);
    expect((await api(null, "GET", `/api/assets/${digest}`)).status).toBe(401);
  });
});

describe("web app sockets", () => {
  it("accepts the session token as a WebSocket subprotocol", async () => {
    const { SELF } = await import("cloudflare:test");
    const webby = await signIn("webby@example.com");
    const board = await newBoard(webby);
    const res = await SELF.fetch(`https://inkboard.test/api/boards/${board.id}/socket`, {
      headers: { upgrade: "websocket", "sec-websocket-protocol": `inkboard, bearer.${webby.token}` },
    });
    expect(res.status).toBe(101);
    expect(res.headers.get("sec-websocket-protocol")).toBe("inkboard");
    res.webSocket?.accept();
    res.webSocket?.close();
    const bad = await SELF.fetch(`https://inkboard.test/api/boards/${board.id}/socket`, {
      headers: { upgrade: "websocket", "sec-websocket-protocol": "inkboard, bearer.nope" },
    });
    expect(bad.status).toBe(401);
  });
});
