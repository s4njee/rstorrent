// The console chrome's viewport rules: which columns a narrow window drops, and
// in what order.
import { describe, expect, it } from "vitest";
import {
  RESPONSIVE_DROPS,
  droppedColumnsAt,
  responsiveColumns,
} from "./useResponsiveColumns";
import {
  defaultColumnState,
  setColumnVisible,
  visibleColumns,
} from "../components/table/columns";

/** The user's default columns plus the two hidden-by-default date columns. */
function allColumns() {
  let state = defaultColumnState();
  state = setColumnVisible(state, "started", true);
  state = setColumnVisible(state, "finished", true);
  return state;
}

describe("droppedColumnsAt", () => {
  it("drops nothing at the design's assumed width", () => {
    expect(droppedColumnsAt(1600)).toBe(0);
    expect(droppedColumnsAt(1280)).toBe(0);
  });

  it("drops one at a time as the window narrows, in the design's order", () => {
    expect(droppedColumnsAt(899)).toBe(1);
    expect(droppedColumnsAt(859)).toBe(2);
    expect(droppedColumnsAt(819)).toBe(3);
  });
});

describe("responsiveColumns", () => {
  it("keeps every visible column at full width", () => {
    const state = allColumns();
    const expected = visibleColumns(state).map((column) => column.id);
    expect(responsiveColumns(state, 1280)).toEqual(expected);
  });

  it("drops tracker, then added, then ratio", () => {
    // `added` is not a column yet (WC3-S1), so the first drop that bites is the
    // tracker — the point is the order, which the list encodes.
    expect(RESPONSIVE_DROPS[0]).toBe("tracker");
    expect(RESPONSIVE_DROPS[1]).toBe("added");
    expect(RESPONSIVE_DROPS[2]).toBe("ratio");
    // And narrowing takes them in that order, one breakpoint at a time: 859 is
    // past the tracker and the (future) added column, but ratio survives.
    expect(responsiveColumns(allColumns(), 899)).not.toContain("tracker");
    expect(responsiveColumns(allColumns(), 859)).toContain("ratio");
    expect(responsiveColumns(allColumns(), 819)).not.toContain("ratio");
  });

  it("narrows the rendered set without touching the saved state", () => {
    const state = allColumns();
    const before = JSON.stringify(state);

    const at900 = responsiveColumns(state, 899);
    expect(at900).not.toContain("tracker");
    expect(at900).toContain("ratio");

    const at820 = responsiveColumns(state, 819);
    expect(at820).not.toContain("ratio");

    // The caller's state is untouched: widening brings the columns back.
    expect(JSON.stringify(state)).toBe(before);
    expect(responsiveColumns(state, 1280)).toContain("tracker");
  });

  it("respects a column the user hid", () => {
    let state = allColumns();
    state = setColumnVisible(state, "ratio", false);
    expect(responsiveColumns(state, 1600)).not.toContain("ratio");
  });
});
