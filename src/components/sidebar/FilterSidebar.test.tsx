// The sidebar's rows: the design's status vocabulary, label swatches, and the
// unlabeled facet.
import { beforeEach, describe, expect, it } from "vitest";
import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { FilterSidebar } from "./FilterSidebar";
import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import type { TorrentDto } from "../../ipc/types";

function torrent(extra: Partial<TorrentDto>): TorrentDto {
  return {
    hash: Math.random().toString(36).slice(2),
    name: "n",
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

function render(): { container: HTMLElement; root: Root } {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => {
    root.render(<FilterSidebar />);
  });
  return { container, root };
}

/** The rendered row whose label is `text`. */
function rowWith(
  container: HTMLElement,
  text: string,
): HTMLElement | undefined {
  return [...container.querySelectorAll<HTMLElement>('[class*="row"]')].find(
    (row) => row.querySelector('[class*="label"]')?.textContent === text,
  );
}

beforeEach(() => {
  useUi.setState({
    facets: {},
    search: "",
    selection: new Set(),
    smartFilters: [],
  });
  useTorrents.setState({
    torrents: [
      torrent({ status: "downloading", label: "iso" }),
      torrent({ status: "downloading", label: "iso" }),
      torrent({ status: "seeding", percent: 100, label: "iso" }),
      torrent({ status: "paused", label: "archive" }),
      torrent({ status: "checking", label: "archive" }),
      torrent({ status: "error", errorKind: "tracker_timeout" }),
    ],
  });
  document.body.innerHTML = "";
});

describe("status rows", () => {
  it("uses the console's words, in the design's order", () => {
    const { container, root } = render();
    const labels = [...container.querySelectorAll('[class*="row"]')]
      .map((row) => row.querySelector('[class*="label"]')?.textContent)
      .filter((text): text is string => Boolean(text));

    expect(labels.slice(0, 8)).toEqual([
      "All",
      "Downloading",
      "Seeding",
      "Stopped",
      "Checking",
      "Errored",
      "Completed",
      "Stalled",
    ]);
    act(() => root.unmount());
  });

  it("counts each state, with Completed as a percent superset", () => {
    const { container, root } = render();
    expect(rowWith(container, "All")?.textContent).toContain("6");
    expect(rowWith(container, "Downloading")?.textContent).toContain("2");
    expect(rowWith(container, "Seeding")?.textContent).toContain("1");
    expect(rowWith(container, "Stopped")?.textContent).toContain("1");
    expect(rowWith(container, "Checking")?.textContent).toContain("1");
    expect(rowWith(container, "Errored")?.textContent).toContain("1");
    // The seeding row is complete, so Completed counts it too.
    expect(rowWith(container, "Completed")?.textContent).toContain("1");
    expect(rowWith(container, "Stalled")?.textContent).toContain("0");
    act(() => root.unmount());
  });

  it("filters on the daemon's status, not its own word", () => {
    // "Stopped" is how the console words `paused`, and that is what must be set.
    const { container, root } = render();
    const stopped = rowWith(container, "Stopped")!;
    act(() => stopped.click());

    expect(useUi.getState().facets.status).toBe("paused");
    act(() => root.unmount());
  });

  it("clears its own dimension when clicked twice", () => {
    const { container, root } = render();
    const seeding = rowWith(container, "Seeding")!;
    act(() => seeding.click());
    expect(useUi.getState().facets.status).toBe("seeding");
    act(() => seeding.click());
    expect(useUi.getState().facets.status).toBeUndefined();
    act(() => root.unmount());
  });
});

describe("labels", () => {
  it("gives each label a swatch keyed to its family", () => {
    const { container, root } = render();
    const iso = rowWith(container, "iso")!;
    const square = iso.querySelector('[class*="square"]')!;
    expect(square.getAttribute("data-label")).toBe("iso");

    // A label with no built-in family still gets a square, in the neutral hue.
    const archive = rowWith(container, "archive")!;
    expect(
      archive.querySelector('[class*="square"]')?.getAttribute("data-label"),
    ).toBe("archive");
    act(() => root.unmount());
  });

  it("offers an unlabeled row that filters to torrents with no label", () => {
    const { container, root } = render();
    const unlabeled = rowWith(container, "unlabeled")!;
    expect(unlabeled).toBeDefined();
    expect(unlabeled.textContent).toContain("1");

    act(() => unlabeled.click());
    // The empty string is the facet value, so it is not confused with "no filter".
    expect(useUi.getState().facets.label).toBe("");
    act(() => root.unmount());
  });

  it("hides the unlabeled row when every torrent has a label", () => {
    useTorrents.setState({
      torrents: [torrent({ status: "seeding", label: "iso" })],
    });
    const { container, root } = render();
    expect(rowWith(container, "unlabeled")).toBeUndefined();
    act(() => root.unmount());
  });
});
