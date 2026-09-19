import { expect, test, type Page } from "@playwright/test";
import path from "node:path";
import { fileURLToPath } from "node:url";

/**
 * The console's end-to-end smoke (WC10-S1).
 *
 * One serial run against `rstorrent-web` in mock mode: the shell loads and
 * polls, filtering and selection work, a transport action takes, the detail
 * tabs render, the add modal accepts a magnet and a `.torrent`, Settings saves a
 * daemon key, the Stats route renders, a 401 falls back to the login screen, and
 * a disconnected snapshot raises and then clears the banner.
 */

const TORRENT_FIXTURE = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  "fixtures",
  "test.torrent",
);

/** Rows the table renders (the header is not a `[data-hash]` row). */
function rows(page: Page) {
  return page.locator('[role="row"][data-hash]');
}

async function firstHash(page: Page): Promise<string> {
  const hash = await rows(page).first().getAttribute("data-hash");
  if (!hash) throw new Error("no torrent row rendered");
  return hash;
}

const EMPTY_GLOBALS = {
  downRate: 0,
  upRate: 0,
  downRateLimit: 0,
  upRateLimit: 0,
  dhtNodes: 0,
  freeSpace: null,
  diskSize: null,
  turtleActive: false,
};

/** A snapshot the client renders as the disconnected state. */
function disconnectedSnapshot() {
  return {
    revision: 10_000,
    torrents: [],
    globals: EMPTY_GLOBALS,
    connection: {
      phase: "disconnected",
      endpoint: "mock",
      daemonVersion: null,
      error: "rtorrent unreachable",
      retryInSeconds: 5,
    },
  };
}

test.describe.configure({ mode: "serial" });

test("the console loads and polls the mock daemon", async ({ page }) => {
  await page.goto("/");
  await expect(rows(page).first()).toBeVisible();
  // The fixture set is fifteen rows.
  expect(await rows(page).count()).toBe(15);
  // A status word and a rate are rendered, so the poll produced real data.
  await expect(
    page.getByText("Downloading", { exact: true }).first(),
  ).toBeVisible();
});

test("filter, sort and select narrow the table", async ({ page }) => {
  await page.goto("/");
  await expect(rows(page).first()).toBeVisible();

  const filter = page.getByLabel("Filter torrents");
  await filter.fill("debian");
  await expect(rows(page)).toHaveCount(1);
  await filter.fill("");
  await expect(rows(page)).toHaveCount(15);

  // Sorting by a header keeps every row but changes the order.
  await page.getByRole("columnheader", { name: /^Size/ }).click();

  const hash = await firstHash(page);
  await page.locator(`[data-hash="${hash}"]`).click();
  await expect(page.locator(`[data-hash="${hash}"]`)).toHaveAttribute(
    "aria-selected",
    "true",
  );
});

test("a transport action takes (pause shows immediately)", async ({ page }) => {
  await page.goto("/");
  await expect(rows(page).first()).toBeVisible();

  const downloading = rows(page).filter({ hasText: "Downloading" }).first();
  await expect(downloading).toBeVisible();
  const hash = await downloading.getAttribute("data-hash");
  await downloading.click();

  await page.getByRole("button", { name: "Pause", exact: true }).click();
  // Paused-and-loaded reads "Queued" (open, idle); a closed one reads "Stopped".
  await expect(page.locator(`[data-hash="${hash}"]`)).not.toContainText(
    "Downloading",
  );
  await expect(page.locator(`[data-hash="${hash}"]`)).toContainText(
    /Queued|Stopped/,
  );
});

test("search matches a torrent's contained filenames", async ({ page }) => {
  await page.goto("/");
  await expect(rows(page).first()).toBeVisible();

  const filter = page.getByLabel("Filter torrents");
  // "checksum" appears only in the mock's file lists, so a match can only come
  // from the server's filename index (V3-12), not the local field search.
  await filter.fill("checksum");
  await expect(rows(page).first()).toBeVisible({ timeout: 15_000 });
  expect(await rows(page).count()).toBeGreaterThan(0);

  await filter.fill("");
  await expect(rows(page).first()).toBeVisible();
});

test("every detail tab renders for the selected torrent", async ({ page }) => {
  await page.goto("/");
  await expect(rows(page).first()).toBeVisible();
  await rows(page).first().click();

  for (const tab of [
    "Files",
    "Peers",
    "Trackers",
    "Transfer",
    "Pieces",
    "Log",
  ]) {
    await page.getByRole("tab", { name: tab, exact: true }).click();
    // The panel keeps the focused torrent's name above the body.
    await expect(page.getByRole("tabpanel")).toBeVisible();
  }
});

test("the add modal accepts a magnet", async ({ page }) => {
  await page.goto("/");
  await expect(rows(page).first()).toBeVisible();
  const before = await rows(page).count();

  await page.getByRole("button", { name: "Add torrent" }).click();
  const dialog = page.getByRole("dialog", { name: "Add torrent" });
  await expect(dialog).toBeVisible();

  // The toolbar opens the file pane; switch to the magnet pane.
  await dialog.getByRole("button", { name: "Magnet / URL" }).click();
  await page
    .getByLabel(/Magnet URI/)
    .fill(
      "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=e2e-magnet",
    );
  await dialog.getByRole("button", { name: "Add", exact: true }).click();
  await expect(dialog).toBeHidden();
  // The mock's `load_magnet` adds the torrent, so the list grows by one.
  await expect(rows(page)).toHaveCount(before + 1);
});

test("the add modal accepts a .torrent file", async ({ page }) => {
  await page.goto("/");
  await expect(rows(page).first()).toBeVisible();
  const before = await rows(page).count();

  await page.getByRole("button", { name: "Add torrent" }).click();
  const dialog = page.getByRole("dialog", { name: "Add torrent" });
  await dialog
    .getByRole("button", { name: ".torrent file", exact: true })
    .click();

  await page.locator('input[type="file"]').setInputFiles(TORRENT_FIXTURE);
  await expect(dialog.getByText("test.torrent")).toBeVisible();
  // The tree from the inspected metadata is shown for a single file.
  await expect(dialog.getByText(/Contents ·/)).toBeVisible();

  await dialog.getByRole("button", { name: "Add", exact: true }).click();
  await expect(dialog).toBeHidden();
  // The mock reads the torrent's own metadata, so the row carries its real name.
  await expect(rows(page)).toHaveCount(before + 1);
  await expect(page.getByText("test.bin").first()).toBeVisible();
});

test("adding the same .torrent again is caught before it is added", async ({
  page,
}) => {
  await page.goto("/");
  await expect(rows(page).first()).toBeVisible();
  const before = await rows(page).count();

  await page.getByRole("button", { name: "Add torrent" }).click();
  const dialog = page.getByRole("dialog", { name: "Add torrent" });
  await dialog
    .getByRole("button", { name: ".torrent file", exact: true })
    .click();
  await page.locator('input[type="file"]').setInputFiles(TORRENT_FIXTURE);
  await expect(dialog.getByText("test.torrent")).toBeVisible();

  // It is already in the library from the previous test: the warning shows and
  // Add is disabled, so the duplicate can never be submitted (V3-13).
  await expect(dialog.getByText(/Already in your library/)).toBeVisible();
  await expect(
    dialog.getByRole("button", { name: "Add", exact: true }),
  ).toBeDisabled();

  await dialog.getByRole("button", { name: "Cancel", exact: true }).click();
  await expect(rows(page)).toHaveCount(before);
});

test("Settings reads and saves a daemon key", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByTestId("settings-page")).toBeVisible();

  await page.getByRole("button", { name: "Connection", exact: true }).click();
  const port = page.getByLabel("Listen port range");
  await expect(port).toHaveValue("6881-6899");
  await port.fill("7000-7010");

  await page.getByRole("button", { name: "Save", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("saved 1 setting");
});

test("the Stats route renders history, cards and volumes", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "Statistics" }).click();
  await expect(page.getByTestId("stats-page")).toBeVisible();
  await expect(page.getByRole("heading", { name: "Throughput" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Volumes" })).toBeVisible();
  // Mock mode serves one healthy and one gone volume.
  await expect(page.getByText("unavailable")).toBeVisible();
});

test("a 401 falls back to the login screen", async ({ page }) => {
  await page.route("**/api/health", (route) =>
    route.fulfill({ status: 401, body: "{}" }),
  );
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Sign in" })).toBeVisible();
});

test("tags can be added to a torrent and filtered from the sidebar", async ({
  page,
}) => {
  await page.goto("/");
  await expect(rows(page).first()).toBeVisible();
  const hash = await firstHash(page);
  await page.locator(`[data-hash="${hash}"]`).click();

  await page.getByRole("button", { name: "Edit tags", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Edit tags" });
  await expect(dialog).toBeVisible();
  const input = dialog.getByLabel("Add tags");
  await input.fill("e2e-tag");
  await input.press("Enter");
  await dialog.getByRole("button", { name: "Apply", exact: true }).click();
  await expect(dialog).toBeHidden();

  // The tag reaches the daemon and comes back on the next poll, so the row
  // shows the chip and the sidebar gains a Tags group.
  await expect(page.locator(`[data-hash="${hash}"]`)).toContainText("e2e-tag");
  await expect(
    page.getByRole("button", { name: /e2e-tag/ }).first(),
  ).toBeVisible();
});

test("a disconnected snapshot raises the banner and recovers", async ({
  page,
}) => {
  await page.goto("/");
  await expect(rows(page).first()).toBeVisible();

  const body = JSON.stringify(disconnectedSnapshot());
  const headers = { "content-type": "application/json" };
  await page.route("**/api/state", (route) =>
    route.fulfill({ status: 200, headers, body }),
  );
  await page.route("**/api/delta*", (route) =>
    route.fulfill({ status: 409, headers, body }),
  );

  await expect(
    page.getByText("Lost connection to rTorrent — retrying…"),
  ).toBeVisible({ timeout: 15_000 });

  await page.unroute("**/api/state");
  await page.unroute("**/api/delta*");
  await expect(rows(page).first()).toBeVisible({ timeout: 15_000 });
});
