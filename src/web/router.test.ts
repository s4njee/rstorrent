import { describe, expect, it } from "vitest";
import { normalize } from "./router";

describe("normalize", () => {
  it("maps the three routes and their deep links", () => {
    expect(normalize("/")).toBe("/");
    expect(normalize("/settings")).toBe("/settings");
    expect(normalize("/settings/interface")).toBe("/settings");
    expect(normalize("/stats")).toBe("/stats");
    expect(normalize("/stats/volumes")).toBe("/stats");
  });

  it("falls back to the console for anything unknown", () => {
    expect(normalize("/nope")).toBe("/");
    expect(normalize("")).toBe("/");
    // A path that merely starts with a route word is not that route.
    expect(normalize("/settings-export")).toBe("/settings");
    expect(normalize("/statistics")).toBe("/");
  });
});
