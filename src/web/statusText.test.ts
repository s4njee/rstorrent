import { describe, it, expect } from "vitest";
import { webStatusLabel } from "./statusText";

describe("webStatusLabel", () => {
  it("passes non-error statuses through unchanged", () => {
    expect(webStatusLabel("downloading", "")).toBe("downloading");
    expect(webStatusLabel("seeding", "")).toBe("seeding");
    expect(webStatusLabel("stalled", "")).toBe("stalled");
    expect(webStatusLabel("checking", "")).toBe("checking");
  });

  it("labels tracker errors 'trk error' via fallback", () => {
    expect(webStatusLabel("error", "Tracker: Connection timed out")).toBe(
      "trk error",
    );
    expect(webStatusLabel("error", "")).toBe("trk error");
  });

  it("labels storage errors 'disk error' via fallback", () => {
    expect(
      webStatusLabel("error", "Download data missing, files not found"),
    ).toBe("disk error");
    expect(webStatusLabel("error", "No space left on device")).toBe(
      "disk error",
    );
    expect(webStatusLabel("error", "Permission denied")).toBe("disk error");
  });

  it("uses D19 errorKind when present", () => {
    expect(webStatusLabel("error", "", "unregistered")).toBe("unregistered");
    expect(webStatusLabel("error", "", "tracker_timeout")).toBe("timeout");
    expect(webStatusLabel("error", "", "tracker_error")).toBe("trk error");
    expect(webStatusLabel("error", "", "missing_files")).toBe("missing files");
    expect(webStatusLabel("error", "", "no_space")).toBe("no space");
    expect(webStatusLabel("error", "", "permission")).toBe("permission");
    expect(webStatusLabel("error", "", "disk_error")).toBe("disk error");
  });
});
