/**
 * Viewport-driven column visibility.
 *
 * The design narrows the table rather than scrolling it sideways: below ~900px
 * the Tracker, Added and Ratio columns drop, in that order. That is a property
 * of the window, not a preference — so it never touches the user's saved column
 * state, and widening the window brings the columns straight back.
 */

import { useEffect, useState } from "react";
import {
  visibleColumns,
  type ColumnId,
  type ColumnState,
} from "../components/table/columns";

/**
 * Columns dropped as the window narrows, in the design's order.
 *
 * Typed as strings because `added` is not a column yet: the table gains it with
 * the console's 12-column set (WC3-S1), and it slots into this list then without
 * a change here.
 */
export const RESPONSIVE_DROPS: readonly string[] = [
  "tracker",
  "added",
  "ratio",
];

/** Widths at which each further column is dropped. */
const BREAKPOINTS: readonly number[] = [900, 860, 820];

/** How many of `RESPONSIVE_DROPS` are hidden at `width`. */
export function droppedColumnsAt(width: number): number {
  return BREAKPOINTS.filter((breakpoint) => width < breakpoint).length;
}

/** The column ids to render at `width`, given the user's saved state. */
export function responsiveColumns(
  state: ColumnState,
  width: number,
): ColumnId[] {
  const dropped = new Set(RESPONSIVE_DROPS.slice(0, droppedColumnsAt(width)));
  return visibleColumns(state)
    .map((column) => column.id)
    .filter((id) => !dropped.has(id));
}

/**
 * The user's visible columns, narrowed to what fits. Recomputes on resize; the
 * caller keeps rendering from the returned list only.
 */
export function useResponsiveColumns(state: ColumnState): ColumnId[] {
  const [width, setWidth] = useState(() =>
    typeof window === "undefined"
      ? Number.POSITIVE_INFINITY
      : window.innerWidth,
  );

  useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);

  return responsiveColumns(state, width);
}
