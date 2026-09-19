import { describe, expect, it } from "vitest";
import {
  COLUMN_DEFINITIONS,
  defaultColumnState,
  deserializeColumnState,
  gridTemplateColumns,
  moveColumn,
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

    // The name is flexible with a 200px floor, and locked against resizing.
    state = resizeColumn(state, "name", 80);
    expect(state.widths.name).toBe(200);
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
    // The selection box is fixed too — hiding it would strand the checkboxes.
    expect(setColumnVisible(state, "select", false).visibility.select).toBe(
      true,
    );
  });

  it("generates a grid template with hidden columns excluded", () => {
    let state = resizeColumn(defaultColumnState(), "size", 88);
    state = setColumnVisible(state, "done", false);

    const template = gridTemplateColumns(state);
    // The design's order: select, name (flexible), then the data columns.
    expect(template).toBe(
      "26px minmax(200px, 1fr) 88px 96px 66px 78px 78px 64px 54px 88px 86px 120px",
    );
    // Default-visible columns (Started/Finished ship hidden), minus the one
    // we hid here (done).
    const defaultVisible = COLUMN_DEFINITIONS.filter(
      (c) => !c.defaultHidden,
    ).length;
    expect(visibleColumns(state)).toHaveLength(defaultVisible - 1);
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

    expect(restored.widths.name).toBe(200);
    expect(restored.widths.down).toBe(78);
    expect(restored.visibility.name).toBe(true);
    expect(restored.visibility.down).toBe(false);
    expect(restored.visibility.tracker).toBe(true);
  });

  it("loads a column set saved before the console's columns", () => {
    // A pre-port payload: separate S and P columns, no Added, no select box.
    const restored = deserializeColumnState(
      JSON.stringify({
        version: 1,
        widths: { name: 240, seeds: 52, peers: 52, tracker: 130 },
        visibility: { name: true, seeds: true, peers: true, tracker: false },
      }),
    );

    // Unknown ids are dropped, the new ones appended, and the widths that
    // survive are kept. Name is flexible, so its persisted width is ignored.
    expect(restored.widths.name).toBe(200);
    expect(restored.widths.tracker).toBe(130);
    expect(restored.visibility.tracker).toBe(false);
    expect(restored.order).toContain("seedsPeers");
    expect(restored.order).toContain("added");
    expect(restored.order).not.toContain("seeds");
  });

  it("keeps the selection box and the name at the front when reordering", () => {
    const state = defaultColumnState();
    // Moving Ratio before Tracker is a plain reorder.
    const moved = moveColumn(state, "ratio", "tracker");
    expect(moved.order.indexOf("ratio")).toBeLessThan(
      moved.order.indexOf("tracker"),
    );

    // Neither the select box nor the name can be dragged…
    expect(moveColumn(state, "select", "tracker").order).toEqual(state.order);
    expect(moveColumn(state, "name", "tracker").order).toEqual(state.order);

    // …and nothing lands in front of them.
    const toFront = moveColumn(state, "ratio", "name");
    expect(toFront.order.slice(0, 2)).toEqual(["select", "name"]);
  });

  it("round-trips the column order", () => {
    const moved = moveColumn(defaultColumnState(), "ratio", "tracker");
    const restored = deserializeColumnState(serializeColumnState(moved));
    expect(restored.order).toEqual(moved.order);
    expect(restored).toEqual(moved);
  });

  it("pins the locked pair even if persisted state puts them elsewhere", () => {
    const restored = deserializeColumnState(
      JSON.stringify({
        version: 1,
        order: ["tracker", "ratio", "name", "select"],
        widths: {},
        visibility: {},
      }),
    );
    expect(restored.order.slice(0, 2)).toEqual(["select", "name"]);
  });
});
