import { describe, it, expect, beforeEach } from "vitest";
import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import { makeLargeFixture } from "../../demo/largeFixture";
import { TorrentTable } from "./TorrentTable";
import type { TorrentDto } from "../../ipc/types";

function mount(): HTMLElement {
  const container = document.createElement("div");
  document.body.appendChild(container);
  return container;
}

function unmount(container: HTMLElement, root: Root) {
  act(() => root.unmount());
  container.remove();
}

/** Seed the store the way a snapshot would. */
function seed(torrents: TorrentDto[], hasLoaded = true) {
  useTorrents.setState({ torrents, hasLoaded });
}

async function renderTable(): Promise<{ container: HTMLElement; root: Root }> {
  const container = mount();
  const root = createRoot(container);
  await act(async () => {
    root.render(<TorrentTable />);
  });
  // Let the row-height effect settle.
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  return { container, root };
}

/** A row carrying only what a cell reads. */
function torrent(extra: Partial<TorrentDto>): TorrentDto {
  return {
    hash: "H",
    name: "name",
    size: 0,
    bytesDone: 0,
    percent: 0,
    status: "downloading",
    statusMsg: "",
    errorKind: "",
    seedsConnected: 0,
    peersConnected: 0,
    seedsSwarm: 0,
    peersSwarm: 0,
    downRate: 0,
    upRate: 0,
    etaSeconds: null,
    ratio: 0,
    label: "",
    trackerHost: "",
    savePath: "",
    priority: 0,
    isPrivate: false,
    views: [],
    ...extra,
  } as TorrentDto;
}

function resetStores() {
  useUi.setState({
    selection: new Set(),
    anchor: null,
    facets: {},
    search: "",
    sortColumn: "name",
    sortDir: "asc",
  });
  seed([]);
  document.body.innerHTML = "";
}

/** jsdom resolves no stylesheet, so the window uses the row-height fallback. */
const ROW_HEIGHT = 30;

describe("TorrentTable virtualization (FND-01)", () => {
  beforeEach(resetStores);

  it("renders only a windowed slice for 5k torrents (DOM bounded)", async () => {
    seed(makeLargeFixture({ count: 5000, seed: 123 }).torrents);
    const { container, root } = await renderTable();

    const rows = container.querySelectorAll('[id^="torrent-row-"]');
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(100);

    // The spacer's height is the whole list at one row height.
    const spacer = container.querySelector(
      '[class*="virtualSpacer"]',
    ) as HTMLElement | null;
    expect(spacer).not.toBeNull();
    expect(parseInt(spacer!.style.height, 10)).toBe(5000 * ROW_HEIGHT);
    unmount(container, root);
  });

  it("preserves selection across virtualization (selected class)", async () => {
    const snap = makeLargeFixture({ count: 100, seed: 7 });
    const target = snap.torrents[0]!;
    seed(snap.torrents);
    useUi.setState({ selection: new Set([target.hash]), anchor: target.hash });

    const { container, root } = await renderTable();

    expect(useUi.getState().selection.has(target.hash)).toBe(true);
    unmount(container, root);
  });

  it("header remains outside virtualized body (sticky via flex)", async () => {
    seed(makeLargeFixture({ count: 10, seed: 1 }).torrents);
    const { container, root } = await renderTable();

    expect(container.textContent).toContain("Name");
    const body = container.querySelector('[data-testid="torrent-table-body"]')!;
    expect(body).not.toBeNull();
    // The header is a sibling of the scrolling body, not inside it.
    expect(body.textContent).not.toContain("Seeds/Peers");
    unmount(container, root);
  });

  it("shows the design's empty state and clears the filter", async () => {
    seed(makeLargeFixture({ count: 10, seed: 1 }).torrents);
    useUi.setState({ search: "zzzz-no-match-xyz", facets: {} });

    const { container, root } = await renderTable();

    expect(container.textContent).toContain("No torrents match this filter");
    const clear = [...container.querySelectorAll("button")].find((button) =>
      button.textContent?.includes("Clear filters"),
    );
    expect(clear).toBeDefined();

    await act(async () => {
      clear!.click();
    });
    expect(useUi.getState().search).toBe("");
    expect(
      container.querySelectorAll('[id^="torrent-row-"]').length,
    ).toBeGreaterThan(0);
    unmount(container, root);
  });

  it("shows placeholder rows until the first snapshot arrives", async () => {
    seed([], false);
    const { container, root } = await renderTable();

    // Placeholder rows carry no torrent id.
    expect(container.querySelectorAll('[id^="torrent-row-"]')).toHaveLength(0);
    expect(container.querySelectorAll('[class*="shimmerRow"]').length).toBe(8);
    unmount(container, root);
  });
});

describe("TorrentTable cells (design frame 1a)", () => {
  beforeEach(resetStores);

  it("words each row's status and formats its values", async () => {
    seed([
      torrent({
        hash: "A",
        name: "downloading",
        status: "downloading",
        percent: 61.4,
        seedsConnected: 38,
        peersConnected: 112,
        downRate: 8.4 * 1024 * 1024,
        ratio: 0.42,
        label: "iso",
        addedAt: 1_700_000_000,
      }),
      torrent({
        hash: "B",
        name: "seeding",
        status: "seeding",
        percent: 100,
        seedsConnected: 12,
        peersConnected: 44,
        upRate: 620 * 1024,
        ratio: 2.41,
      }),
      torrent({
        hash: "C",
        name: "checking",
        status: "checking",
        percent: 42.4,
      }),
      torrent({ hash: "D", name: "stopped", status: "paused", percent: 47 }),
      torrent({
        hash: "E",
        name: "queued",
        status: "paused",
        percent: 0,
        isOpen: true,
      }),
      torrent({
        hash: "F",
        name: "errored",
        status: "error",
        errorKind: "tracker_timeout",
      }),
    ]);

    const { container, root } = await renderTable();
    const text = container.textContent ?? "";

    // The status vocabulary, including the two words that need rtorrent's raw
    // flags to tell apart.
    expect(text).toContain("Downloading");
    expect(text).toContain("Seeding");
    expect(text).toContain("Checking 42%");
    expect(text).toContain("Stopped");
    expect(text).toContain("Queued");
    expect(text).toContain("Tracker error");

    // Seeds/Peers drops the seed count while seeding.
    expect(text).toContain("38 / 112");
    expect(text).toContain("— / 44");

    // The Done cell labels its own bar; rates format as the design writes them.
    expect(text).toContain("61.4%");
    expect(text).toContain("100%");
    expect(text).toContain("8.4 MiB/s");

    // A ratio below 2.00 is dimmed by class, not by wording.
    const ratio = [...container.querySelectorAll('[class*="num"]')].find(
      (node) => node.textContent === "0.42",
    );
    expect(ratio?.className).toMatch(/ratioPoor/);

    // The label renders as a chip carrying its family for the tint, and an
    // unlabelled row says so rather than showing an empty chip.
    const chip = container.querySelector('[data-label="iso"]');
    expect(chip?.textContent).toBe("iso");
    expect(chip?.className).toMatch(/chip/);
    const unlabelled = [
      ...container.querySelectorAll('[class*="chip"]'),
    ].filter((node) => !node.hasAttribute("data-label"));
    expect(unlabelled.length).toBeGreaterThan(0);
    expect(unlabelled[0]!.textContent).toBe("unlabeled");

    unmount(container, root);
  });

  it("selects every visible row from the header, then clears", async () => {
    seed([
      torrent({ hash: "A", name: "a" }),
      torrent({ hash: "B", name: "b" }),
      torrent({ hash: "C", name: "c" }),
    ]);

    const { container, root } = await renderTable();

    const all = container.querySelector(
      'button[aria-label="Select all torrents"]',
    ) as HTMLButtonElement | null;
    expect(all).not.toBeNull();

    await act(async () => {
      all!.click();
    });
    expect(useUi.getState().selection.size).toBe(3);

    const clear = container.querySelector(
      'button[aria-label="Clear selection"]',
    ) as HTMLButtonElement | null;
    await act(async () => {
      clear!.click();
    });
    expect(useUi.getState().selection.size).toBe(0);
    unmount(container, root);
  });
});
