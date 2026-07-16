import { describe, expect, it } from "vitest";
import {
  COLUMN_DEFINITIONS,
  defaultColumnState,
  deserializeColumnState,
  gridTemplateColumns,
  resizeColumn,
  serializeColumnState,
  setColumnVisible,
  toggleColumn,
  visibleColumns,
} from "./columns";

function memoryLocalStorage() {
  const values = new Map<string, string>();
  return {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => values.set(key, value),
    removeItem: (key: string) => values.delete(key),
  };
}

describe("torrent table column state", () => {
  it("clamps resized widths to each column minimum", () => {
    let state = resizeColumn(defaultColumnState(), "size", 8);
    expect(state.widths.size).toBe(40);

    state = resizeColumn(state, "name", 80);
    expect(state.widths.name).toBe(120);
  });

  it("hides and shows columns but never hides Name", () => {
    let state = setColumnVisible(defaultColumnState(), "tracker", false);
    expect(state.visibility.tracker).toBe(false);
    expect(
      visibleColumns(state).some((column) => column.id === "tracker"),
    ).toBe(false);

    state = toggleColumn(state, "tracker");
    expect(state.visibility.tracker).toBe(true);

    state = setColumnVisible(state, "name", false);
    expect(state.visibility.name).toBe(true);
    expect(toggleColumn(state, "name").visibility.name).toBe(true);
  });

  it("generates a grid template with hidden columns excluded", () => {
    let state = resizeColumn(defaultColumnState(), "name", 240);
    state = resizeColumn(state, "size", 88);
    state = setColumnVisible(state, "done", false);

    const template = gridTemplateColumns(state);
    expect(template).toBe(
      "minmax(240px, 1fr) 88px 84px 52px 52px 76px 76px 62px 46px 72px 110px",
    );
    expect(visibleColumns(state)).toHaveLength(COLUMN_DEFINITIONS.length - 1);
  });

  it("round-trips persisted widths and visibility", () => {
    const localStorage = memoryLocalStorage();
    let state = resizeColumn(defaultColumnState(), "up", 123);
    state = setColumnVisible(state, "label", false);

    localStorage.setItem("test.columns", serializeColumnState(state));
    expect(
      deserializeColumnState(localStorage.getItem("test.columns")),
    ).toEqual(state);
    localStorage.removeItem("test.columns");
  });

  it("falls back to defaults for missing or corrupt persisted data", () => {
    const localStorage = memoryLocalStorage();
    const defaults = defaultColumnState();
    expect(
      deserializeColumnState(localStorage.getItem("missing.columns")),
    ).toEqual(defaults);
    localStorage.setItem("test.columns", "not json");
    expect(
      deserializeColumnState(localStorage.getItem("test.columns")),
    ).toEqual(defaults);
    localStorage.removeItem("test.columns");
    expect(deserializeColumnState(JSON.stringify({ version: 99 }))).toEqual(
      defaults,
    );
  });

  it("normalizes missing fields and invalid persisted values", () => {
    const restored = deserializeColumnState(
      JSON.stringify({
        version: 1,
        widths: { name: 1, down: "wide" },
        visibility: { name: false, down: false },
      }),
    );

    expect(restored.widths.name).toBe(120);
    expect(restored.widths.down).toBe(76);
    expect(restored.visibility.name).toBe(true);
    expect(restored.visibility.down).toBe(false);
    expect(restored.visibility.tracker).toBe(true);
  });
});
