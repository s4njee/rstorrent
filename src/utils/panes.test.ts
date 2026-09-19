// The detail panel's pane vocabulary: the design's five tabs, the log we keep
// as a sixth, which panes share the body with the facts rail, and the daemon tab
// each one polls.
import { describe, expect, it } from "vitest";
import {
  DEFAULT_PANE,
  PANES,
  PANE_IDS,
  daemonTabFor,
  paneDef,
  paneHasRail,
  parsePane,
} from "./panes";

describe("panes", () => {
  it("is the design's five tabs, with the log last", () => {
    expect(PANES.map((pane) => pane.label)).toEqual([
      "Files",
      "Peers",
      "Trackers",
      "Transfer",
      "Pieces",
      "Log",
    ]);
  });

  it("opens on the design's active tab", () => {
    expect(DEFAULT_PANE).toBe("files");
  });

  it("carries the facts rail only where the design draws one", () => {
    expect(PANE_IDS.filter((pane) => paneHasRail(pane))).toEqual([
      "files",
      "transfer",
    ]);
  });

  it("asks the daemon for the tab a pane renders", () => {
    expect(daemonTabFor("files")).toBe("content");
    expect(daemonTabFor("peers")).toBe("peers");
    expect(daemonTabFor("trackers")).toBe("trackers");
    expect(daemonTabFor("transfer")).toBe("general");
    // Pieces reads the same payload as Transfer: the piece map travels with it.
    expect(daemonTabFor("pieces")).toBe("general");
    expect(daemonTabFor("log")).toBe("log");
  });

  it("parses a persisted pane, taking the pre-console names with it", () => {
    expect(parsePane("pieces")).toBe("pieces");
    expect(parsePane("content")).toBe("files");
    expect(parsePane("general")).toBe("transfer");
    // `speed` was the old per-torrent chart, which the design folds into Transfer.
    expect(parsePane("speed")).toBe("transfer");
  });

  it("rejects a persisted value it cannot place", () => {
    expect(parsePane("nope")).toBeNull();
    expect(parsePane("")).toBeNull();
    expect(parsePane(7)).toBeNull();
    expect(parsePane(null)).toBeNull();
    expect(parsePane(undefined)).toBeNull();
  });

  it("falls back to the first pane for an unknown id", () => {
    expect(paneDef("nope" as never).id).toBe("files");
  });
});
