/**
 * SessionDialog (V3-22): export downloads the manifest, upload → preview →
 * selective import runs through the stub backend with no daemon.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { setBackend, type Backend } from "../../ipc/backend";
import { SessionDialog } from "./SessionDialog";

const MANIFEST = JSON.stringify({
  format: "rstorrent-session/1",
  exportedAt: 1700000000,
  client: "rstorrent",
  clientVersion: "test",
  global: { downKb: 0, upKb: 0 },
  torrents: [
    {
      hash: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
      name: "Show",
      savePath: "/dl/Show",
      label: "video",
    },
  ],
});

const calls: string[] = [];

const stubBackend: Backend = {
  invoke: (async (cmd: string) => {
    calls.push(cmd);
    if (cmd === "export_session_text") return MANIFEST;
    if (cmd === "validate_session") {
      return {
        torrentCount: 2,
        restorableCount: 1,
        errors: [],
        warnings: [],
        items: [
          {
            hash: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            name: "Show",
            action: "add",
            dstDir: "/dl/Show",
            reason: "",
          },
          {
            hash: "BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB",
            name: "Other",
            action: "have",
            dstDir: "",
            reason: "already loaded",
          },
        ],
      };
    }
    if (cmd === "import_session") return undefined;
    if (cmd === "import_status") {
      return {
        running: false,
        added: 1,
        resumed: 0,
        skipped: 1,
        failed: [],
        done: true,
      };
    }
    return {};
  }) as Backend["invoke"],
  listen: async () => () => {},
};

let host: HTMLDivElement;
let root: Root;

function button(label: string): HTMLButtonElement {
  const found = [...host.querySelectorAll("button")].find(
    (b) => b.textContent === label,
  );
  if (!found) throw new Error(`button ${label} not found`);
  return found as HTMLButtonElement;
}

async function click(label: string) {
  await act(async () => {
    button(label).click();
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

beforeEach(() => {
  calls.length = 0;
  setBackend(stubBackend);
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  vi.stubGlobal("URL", {
    ...URL,
    createObjectURL: () => "blob:stub",
    revokeObjectURL: () => {},
  });
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.unstubAllGlobals();
});

async function mount() {
  await act(async () => {
    root.render(<SessionDialog />);
  });
}

async function uploadManifest() {
  await mount();
  const input = host.querySelector('input[type="file"]') as HTMLInputElement;
  // jsdom has no File.text(); the production path uses it (universal in real
  // browsers), so install it for the harness.
  Object.defineProperty(File.prototype, "text", {
    value: async function (this: File) {
      return MANIFEST;
    },
    configurable: true,
  });
  const file = new File([MANIFEST], "session-manifest.json", {
    type: "application/json",
  });
  Object.defineProperty(input, "files", { value: [file] });
  await act(async () => {
    input.dispatchEvent(new Event("change", { bubbles: true }));
    // Let the async file read settle before continuing.
    for (let i = 0; i < 5; i++) {
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
  });
}

describe("SessionDialog", () => {
  it("downloads the exported manifest", async () => {
    await mount();
    const anchorClick = vi
      .spyOn(HTMLAnchorElement.prototype, "click")
      .mockImplementation(() => {});
    await click("Download manifest");
    expect(calls).toContain("export_session_text");
    expect(host.textContent).toContain("exported 1 torrent(s)");
    anchorClick.mockRestore();
  });

  it("previews then selectively imports", async () => {
    await uploadManifest();
    await click("Preview");
    expect(calls).toContain("validate_session");
    expect(host.textContent).toContain("Will add");
    expect(host.textContent).toContain("Already here");

    // Only the addable entry is checked by default; unchecking it disables import.
    const checkbox = host.querySelector(
      'input[type="checkbox"]',
    ) as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
    await act(async () => {
      checkbox.click();
    });
    expect(button("Import 0 stopped").disabled).toBe(true);

    await act(async () => {
      checkbox.click();
    });
    await click("Import 1 stopped");
    expect(calls).toContain("import_session");
    expect(host.textContent).toContain("finished: 1 added");
  });
});
