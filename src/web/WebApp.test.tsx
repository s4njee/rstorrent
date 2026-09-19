/**
 * Render smoke for the web shell: proves the shell composes and renders the
 * shared chrome (top bar, action toolbar, filter sidebar, status bar) without a
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
    // The shared console chrome: the mark and wordmark, rates, filter, the
    // toolbar's verbs, the sidebar's groups, and the status bar.
    expect(html).toContain("Blackbird");
    expect(html).toContain('src="/blackbird.jpg"');
    expect(html).toContain("Filter torrents");
    expect(html).toContain("Add torrent");
    expect(html).toContain("Force recheck");
    expect(html).toContain("Priority ↑");
    expect(html).toContain("Status"); // sidebar status group
    expect(html).toContain("selected ·"); // toolbar readout
  });

  it("shows the connecting state before the first snapshot", () => {
    const html = renderToString(<WebApp onSignOut={() => {}} />);
    expect(html).toContain("connecting to rtorrent");
    // …and explains it in the banner above the toolbar, not just the card.
    expect(html).toContain("Connecting to rTorrent");
  });
});
