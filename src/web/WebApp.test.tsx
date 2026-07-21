/**
 * Render smoke for the web shell: proves the shell composes and renders the
 * shared components (app bar, filter sidebar, footer, disk card) without a
 * render-time crash — e.g. a component reaching for a Tauri API in the browser.
 *
 * This is not a visual/layout check (that's the manual parity pass), and it does
 * not drive live data: under vitest the store is a separate module instance from
 * WebApp's, so the render shows the initial (connecting) state. What it verifies
 * is the integration-bug class a headless run would otherwise catch: that the
 * composed tree renders at all.
 */

import { describe, it, expect, beforeEach } from "vitest";
import { renderToString } from "react-dom/server";
import { setBackend, type Backend } from "../ipc/backend";
import { WebApp } from "./WebApp";

const stubBackend: Backend = {
  invoke: async () => ({}) as never,
  listen: async () => () => {},
  capabilities: {
    localFs: false,
    nativeDialogs: false,
    keychain: false,
    menus: false,
    deepLinks: false,
    clipboardRead: true,
  },
};

beforeEach(() => {
  setBackend(stubBackend);
});

describe("WebApp render smoke", () => {
  it("renders the shell chrome without throwing", () => {
    const html = renderToString(<WebApp onSignOut={() => {}} />);
    // App bar wordmark, settings control, filter sidebar, and footer all render.
    expect(html).toContain("rtorrent");
    expect(html).toContain('title="Status"');
    expect(html).toContain("Status"); // sidebar status group
    expect(html).toContain("dht:"); // footer
  });

  it("shows the connecting state before the first snapshot", () => {
    const html = renderToString(<WebApp onSignOut={() => {}} />);
    expect(html).toContain("connecting to rtorrent");
  });
});
