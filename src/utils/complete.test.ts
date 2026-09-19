import { describe, expect, it } from "vitest";

import {
  completionDestination,
  labelRule,
  planMove,
  preflight,
  resolveCollision,
  resolveDestination,
  routeNewDownload,
  tagRule,
} from "./complete";

const rules = [tagRule("Archive", "/media/archive"), labelRule("video", "/media/video")];

describe("resolveDestination", () => {
  it("prefers the first matching tag, then the label, then the default", () => {
    expect(resolveDestination("video", ["archive"], rules, "/dl")).toBe("/media/archive");
    expect(resolveDestination("VIDEO", [], rules, "/dl")).toBe("/media/video");
    expect(resolveDestination("other", ["other"], rules, "/dl")).toBe("/dl");
  });

  it("ignores rules with an empty destination", () => {
    expect(resolveDestination("video", [], [labelRule("video", "")], "/dl")).toBe("/dl");
  });
});

describe("routeNewDownload", () => {
  it("is the identity with nothing recorded when the feature is off", () => {
    expect(routeNewDownload("", "/dl")).toEqual({ directory: "/dl", finalDir: null });
    expect(routeNewDownload("  ", "/dl")).toEqual({ directory: "/dl", finalDir: null });
  });

  it("loads into incomplete and records the way home", () => {
    expect(routeNewDownload("/dl/.incomplete", "/media/video")).toEqual({
      directory: "/dl/.incomplete",
      finalDir: "/media/video",
    });
  });

  it("records nothing when already home or nothing chosen", () => {
    expect(routeNewDownload("/dl/.incomplete", "/dl/.incomplete")).toEqual({
      directory: "/dl/.incomplete",
      finalDir: null,
    });
    expect(routeNewDownload("/dl/.incomplete", "")).toEqual({ directory: "", finalDir: null });
  });
});

describe("completionDestination", () => {
  it("chains rules, then label paths, then the recorded home", () => {
    const labels = [{ label: "video", savePath: "/media/label-video" }];
    expect(completionDestination("video", [], rules, labels, "/dl")).toBe("/media/video");
    expect(completionDestination("video", [], [], labels, "/dl")).toBe("/media/label-video");
    expect(completionDestination("other", [], [], [], "/media/manual")).toBe("/media/manual");
    expect(completionDestination("other", [], [], [], "/dl")).toBe("/dl");
  });
});

describe("planMove", () => {
  it("returns null when already home or inputs are missing", () => {
    expect(planMove("/dl", "/dl", "Show.S01")).toBeNull();
    expect(planMove("/DL", "/dl", "Show.S01")).toBeNull();
    expect(planMove("/tmp", "", "Show.S01")).toBeNull();
    expect(planMove("", "/dl", "Show.S01")).toBeNull();
  });

  it("plans incomplete → destination moves", () => {
    expect(planMove("/tmp/.incomplete", "/dl", "Show.S01")).toEqual({
      src: "/tmp/.incomplete/Show.S01",
      dst: "/dl/Show.S01",
    });
  });
});

describe("resolveCollision", () => {
  it("errors by default and renames on request", () => {
    const taken = ["show.s01"];
    expect(() => resolveCollision("/dl", "Show.S01", taken, "error")).toThrow(
      "destination already exists",
    );
    expect(resolveCollision("/dl", "Show.S01", taken, "auto-rename")).toBe("/dl/Show.S01 (1)");
    expect(resolveCollision("/dl", "Movie.mkv", taken, "error")).toBe("/dl/Movie.mkv");
    expect(
      resolveCollision("/dl", "Show.S01", ["show.s01", "show.s01 (1)"], "auto-rename"),
    ).toBe("/dl/Show.S01 (2)");
  });
});

describe("preflight", () => {
  it("blocks only on a known shortage", () => {
    expect(preflight(0, 0)).toEqual({ ok: true });
    expect(preflight(100, null)).toEqual({ ok: true, unknown: true });
    expect(preflight(100, 100)).toEqual({ ok: true });
    expect(preflight(101, 100)).toEqual({ ok: false, required: 101, free: 100 });
  });
});
