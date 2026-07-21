import { describe, it, expect } from "vitest";
import { webStatusLabel } from "./statusText";

describe("webStatusLabel", () => {
  it("passes non-error statuses through unchanged", () => {
    expect(webStatusLabel("downloading", "")).toBe("downloading");
    expect(webStatusLabel("seeding", "")).toBe("seeding");
    expect(webStatusLabel("stalled", "")).toBe("stalled");
    expect(webStatusLabel("checking", "")).toBe("checking");
  });

  it("labels tracker errors 'trk error'", () => {
    expect(webStatusLabel("error", "Tracker: Connection timed out")).toBe(
      "trk error",
    );
    // The design fixture's error row.
    expect(webStatusLabel("error", "")).toBe("trk error");
  });

  it("labels storage errors 'disk error'", () => {
    expect(
      webStatusLabel("error", "Download data missing, files not found"),
    ).toBe("disk error");
    expect(webStatusLabel("error", "No space left on device")).toBe(
      "disk error",
    );
    expect(webStatusLabel("error", "Permission denied")).toBe("disk error");
  });
});
