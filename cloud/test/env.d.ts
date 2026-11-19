import type { Env as InkEnv } from "../src/env";

declare global {
  namespace Cloudflare {
    interface Env extends InkEnv {
      TEST_MIGRATIONS: import("cloudflare:test").D1Migration[];
    }
  }
}
