import { describe, expect, it } from "vitest";
import { SELF } from "cloudflare:test";
import { api } from "./helpers";

const get = (path: string) => SELF.fetch(`https://inkboard.test${path}`, { redirect: "manual" });

describe("website", () => {
  it("serves the home page without signing in", async () => {
    const res = await api(null, "GET", "/");
    expect(res.status).toBe(200);
    expect(res.headers.get("content-type")).toContain("text/html");
    const html = await res.text();
    expect(html).toContain("/download/mac");
    expect(html).toContain("/download/windows");
  });

  it("sends downloads to the newest release", async () => {
    const res = await get("/download/mac");
    expect(res.status).toBe(302);
    expect(res.headers.get("location")).toBe("https://github.com/tejasgowda0012/canvas/releases/latest/download/Inkboard-macOS.dmg");
    expect((await get("/download/amiga")).status).toBe(404);
  });
});
