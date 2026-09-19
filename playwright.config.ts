import { defineConfig, devices } from "@playwright/test";

/**
 * End-to-end suite (WC10-S1).
 *
 * Drives the real `rstorrent-web` binary against its deterministic mock
 * fixtures (`RSTORRENT_MOCK=1`), so the console, its routes, the add modal and
 * a settings save are exercised through the whole stack. The server is built and
 * started by Playwright; `npm run build:web` must have run first because the
 * binary embeds `dist-web` at compile time (the `e2e` script does it).
 *
 * Serial with one worker: the mock is stateful, so a transport action in one
 * test must not race another.
 */
const PORT = Number(process.env.E2E_PORT ?? 9091);
const BASE_URL = `http://127.0.0.1:${PORT}`;

export default defineConfig({
  testDir: "./e2e",
  timeout: 30_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["list"], ["html", { open: "never" }]] : "list",
  use: {
    baseURL: BASE_URL,
    trace: "retain-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: `RSTORRENT_MOCK=1 cargo run -q -p rstorrent-web -- --listen 127.0.0.1:${PORT}`,
    url: `${BASE_URL}/api/health`,
    reuseExistingServer: !process.env.CI,
    timeout: 240_000,
  },
});
