// @vitest-environment jsdom
import { describe, expect, it } from "vitest";
import { validateConfigValue } from "./SettingsPage";
import type { ConfigKind } from "../ipc/webSettings";

describe("validateConfigValue", () => {
  it("bounds integers like the server does", () => {
    const kind: ConfigKind = { type: "int", unit: "", min: 0, max: 100 };
    expect(validateConfigValue(kind, "0")).toBeNull();
    expect(validateConfigValue(kind, "100")).toBeNull();
    expect(validateConfigValue(kind, "101")).toMatch(/between/);
    expect(validateConfigValue(kind, "-1")).toMatch(/between/);
    expect(validateConfigValue(kind, "")).toMatch(/required/);
    expect(validateConfigValue(kind, "1.5")).toMatch(/whole/);
  });

  it("checks choices and booleans strictly", () => {
    expect(
      validateConfigValue(
        { type: "choice", options: ["auto", "disable"] },
        "auto",
      ),
    ).toBeNull();
    expect(
      validateConfigValue(
        { type: "choice", options: ["auto", "disable"] },
        "maybe",
      ),
    ).toMatch(/one of/);
    expect(validateConfigValue({ type: "bool" }, "1")).toBeNull();
    expect(validateConfigValue({ type: "bool" }, "0")).toBeNull();
    expect(validateConfigValue({ type: "bool" }, "yes")).toMatch(/0 or 1/);
  });

  it("bounds string lengths and refuses read-only writes", () => {
    expect(validateConfigValue({ type: "str", maxLen: 4 }, "6881")).toBeNull();
    expect(validateConfigValue({ type: "str", maxLen: 4 }, "68811")).toMatch(
      /at most/,
    );
    expect(validateConfigValue({ type: "readonly" }, "x")).toMatch(/read-only/);
  });
});
