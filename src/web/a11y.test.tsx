/**
 * The accessibility baseline (REL-01), audited rather than eyeballed.
 *
 * axe-core runs against the mounted console and against each dialog the way a
 * user reaches them — through the store the menus use — and the suite fails on
 * any violation. This is the half of accessibility that breaks silently when
 * someone reworks markup: roles, accessible names, labelling relationships and
 * the structural rules a screen reader depends on.
 *
 * Two honest limits. jsdom has no layout, so axe skips the rules that need one
 * and cannot evaluate colour at all: the contrast rule is switched off here
 * because a skipped rule that reports nothing looks exactly like a pass, and
 * `scripts/check-contrast.mjs` covers colour properly — every theme, every
 * accent pair, in the guardrail that already runs in CI. And the console opens
 * with no torrents in this harness (its poll never runs), so what is audited is
 * the shell — the tour of it with rows, files and charts is the manual pass.
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import axe from "axe-core";
import { setBackend, type Backend } from "../ipc/backend";
import { webSettings } from "../ipc/webSettings";
import { WebApp } from "./WebApp";
import { useUi, type DialogKind } from "../store/ui";

/** A stub backend. The console
 *  loads the daemon log on mount and expects a list, and settings for the
 *  prefs draft — those two calls answer shape-complete, everything else is
 *  unread in this harness. */
const stubBackend: Backend = {
  invoke: (async (cmd: string) => {
    if (cmd === "get_log") return [];
    if (cmd === "get_settings") return webSettings();
    return {};
  }) as Backend["invoke"],
  listen: async () => () => {},
};

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  setBackend(stubBackend);
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

/** Run axe over the mounted tree and describe anything it objects to. */
async function violations(): Promise<string[]> {
  const results = await axe.run(host, {
    rules: { "color-contrast": { enabled: false } },
  });
  return results.violations.map(
    (violation) =>
      `${violation.id} (${violation.impact}): ${violation.nodes
        .map((node) => `${node.target.join(" ")} — ${node.failureSummary}`)
        .join("; ")}`,
  );
}

async function mount() {
  await act(async () => {
    // The console's sign-out is a host concern (the server's session); the
    // audit only needs the shell to render and stay signed in.
    root.render(<WebApp onSignOut={() => {}} />);
  });
}

/** Open a dialog the way the menus do, and let it render. */
async function open(kind: DialogKind) {
  await act(async () => {
    useUi.getState().openDialog(kind);
  });
}

describe("the console", () => {
  it("has no accessibility violations", async () => {
    await mount();
    expect(await violations()).toEqual([]);
  });

  // Every dialog the console can open, on top of the console itself — the
  // composition is what a user actually meets, and a dialog's own chrome
  // (its close control, its labelling) only exists once it is open.
  const dialogs: DialogKind[] = [
    "add-file",
    "add-magnet",
    "create-torrent",
    "stats",
    "remove",
    "recheck",
    "set-tags",
    "rate-limit",
    "moves",
    "session",
  ];

  for (const kind of dialogs) {
    it(`opens ${kind} with no accessibility violations`, async () => {
      await mount();
      await open(kind);
      expect(host.querySelector('[role="dialog"]')).not.toBeNull();
      expect(await violations()).toEqual([]);
    });
  }
});

describe("the settings route", () => {
  function json(body: unknown): Response {
    return {
      ok: true,
      status: 200,
      text: async () => JSON.stringify(body),
      json: async () => body,
    } as unknown as Response;
  }

  beforeEach(() => {
    window.history.pushState({}, "", "/settings");
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const path = String(input);
        if (path.startsWith("/api/config")) {
          return json([
            {
              id: "port_range",
              label: "Listen port range",
              key: "network.port_range",
              section: "connection",
              kind: { type: "str", maxLen: 32 },
              hint: "e.g. 6881-6899",
              value: "6881-6899",
              available: true,
            },
          ]);
        }
        if (path.startsWith("/api/settings/advanced")) {
          return json({
            path: "/home/rt/.rtorrent.rc",
            exists: false,
            block: null,
          });
        }
        if (path.startsWith("/api/settings")) {
          return json({
            ui: {},
            server: {
              displayName: "rt",
              savePath: "/srv/downloads",
              listen: "127.0.0.1:9080",
              pollMs: 1000,
              authMode: "none",
              mock: true,
            },
          });
        }
        // /api/health and anything else.
        return json({ server: { displayName: "rt" }, daemon: null });
      }),
    );
  });

  afterEach(() => {
    window.history.pushState({}, "", "/");
    vi.unstubAllGlobals();
  });

  it("has no accessibility violations", async () => {
    await mount();
    // Let the settings fetches resolve and the page re-render.
    await act(async () => {});
    expect(host.querySelector('[data-testid="settings-page"]')).not.toBeNull();
    expect(await violations()).toEqual([]);
  });
});

describe("the stats route", () => {
  function json(body: unknown): Response {
    return {
      ok: true,
      status: 200,
      text: async () => JSON.stringify(body),
      json: async () => body,
    } as unknown as Response;
  }

  beforeEach(() => {
    window.history.pushState({}, "", "/stats");
    vi.stubGlobal(
      "fetch",
      vi.fn(async (input: RequestInfo | URL) => {
        const path = String(input);
        if (path.startsWith("/api/stats")) {
          return json({
            history: [
              { at: 1, down: 10, up: 5 },
              { at: 2, down: 20, up: 8 },
            ],
            pollMs: 1000,
            sessionDown: 1000,
            sessionUp: 200,
            sessionRatio: 0.2,
            counts: {
              total: 3,
              downloading: 1,
              seeding: 1,
              completed: 0,
              stopped: 1,
              checking: 0,
              errored: 0,
              stalled: 0,
            },
            uptimeSeconds: 100,
            dhtNodes: 42,
            portRange: "6881-6899",
            portStatus: null,
            volumes: [
              { path: "/srv/downloads", free: 412, total: 1114 },
              // A gone path must render as unavailable, not zero.
              { path: "/mnt/archive", free: null, total: null },
            ],
          });
        }
        return json({ server: { displayName: "rt" }, daemon: null });
      }),
    );
  });

  afterEach(() => {
    window.history.pushState({}, "", "/");
    vi.unstubAllGlobals();
  });

  it("has no accessibility violations", async () => {
    await mount();
    await act(async () => {});
    expect(host.querySelector('[data-testid="stats-page"]')).not.toBeNull();
    expect(host.textContent).toContain("unavailable");
    expect(await violations()).toEqual([]);
  });
});
