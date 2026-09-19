/**
 * Sortable torrent table with persisted column visibility, live resizing, and
 * fixed-height row virtualization (FND-01).
 *
 * Virtualization keeps the DOM bounded at ~30-50 rows even for a 5k library:
 * the scroll container stays `.body` (so the header remains naturally sticky
 * via flex layout), a spacer sets the total scroll height, and a translated
 * inner renders only the windowed slice. Selection follows native list
 * conventions (click, ⌘-click, ⇧-click range).
 */

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useShallow } from "zustand/react/shallow";
import { useTorrents } from "../../store/torrents";
import { useUi, type SortColumn } from "../../store/ui";
import { selectVisible } from "../../store/selectors";
import { accel } from "../../platform";
import {
  COLUMN_DEFINITIONS,
  columnDefinition,
  gridTemplateColumns,
  type ColumnId,
} from "./columns";
import { Row } from "./Row";
import { useVirtualizer, resolveRowHeight } from "../../hooks/useVirtualizer";
import { useResponsiveColumns } from "../../hooks/useResponsiveColumns";
import styles from "./TorrentTable.module.css";

const SORT_COLUMNS: Partial<Record<ColumnId, SortColumn>> = {
  name: "name",
  size: "size",
  done: "percent",
  status: "status",
  down: "downRate",
  up: "upRate",
  eta: "etaSeconds",
  ratio: "ratio",
  added: "addedAt",
  started: "startedAt",
  finished: "finishedAt",
};

/** Placeholder rows drawn while the first snapshot is in flight. */
const SHIMMER_ROWS = 8;

interface ResizeSession {
  id: ColumnId;
  pointerId: number;
  startX: number;
  startWidth: number;
  moved: boolean;
}

export function TorrentTable() {
  const torrents = useTorrents((state) => state.torrents);
  const { facets, search, sortColumn, sortDir, columns } = useUi(
    useShallow((state) => ({
      facets: state.facets,
      search: state.search,
      sortColumn: state.sortColumn,
      sortDir: state.sortDir,
      columns: state.columns,
    })),
  );
  const selection = useUi((state) => state.selection);
  const smartFilters = useUi((state) => state.smartFilters);
  const fileMatches = useUi((state) => state.fileMatches);
  const setSort = useUi((state) => state.setSort);
  const resizeColumn = useUi((state) => state.resizeColumn);
  const select = useUi((state) => state.select);
  const toggle = useUi((state) => state.toggle);
  const selectRange = useUi((state) => state.selectRange);
  const openContextMenu = useUi((state) => state.openContextMenu);
  const openColumnMenu = useUi((state) => state.openColumnMenu);
  const moveColumn = useUi((state) => state.moveColumn);
  const selectAll = useUi((state) => state.selectAll);
  const clearSelection = useUi((state) => state.clearSelection);

  const hasLoaded = useTorrents((state) => state.hasLoaded);

  const resizeSession = useRef<ResizeSession | null>(null);
  const suppressSort = useRef(false);
  /** The column being dragged, for the header's reorder gesture. */
  const [dragging, setDragging] = useState<ColumnId | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  const visible = useMemo(
    () =>
      selectVisible(
        torrents,
        facets,
        search,
        sortColumn,
        sortDir,
        smartFilters,
        fileMatches,
      ),
    [torrents, facets, search, sortColumn, sortDir, smartFilters, fileMatches],
  );

  // Rows that appeared since the previous render fade in (WC9-S5). Tracked on
  // the full filtered set, not the virtual slice, so scrolling never re-fades a
  // row that was already there.
  const renderedHashes = useRef<Set<string>>(new Set());
  const entering = new Set<string>();
  for (const torrent of visible) {
    if (!renderedHashes.current.has(torrent.hash)) entering.add(torrent.hash);
  }
  useEffect(() => {
    renderedHashes.current = new Set(visible.map((t) => t.hash));
  }, [visible]);
  // A narrow window drops Tracker, then Added, then Ratio (design, §Responsive).
  // That is a property of the viewport, not a preference, so it never rewrites
  // the saved column state — widening the window brings the columns back.
  const visibleColumnIds = useResponsiveColumns(columns);
  const shownColumns = useMemo(
    () =>
      COLUMN_DEFINITIONS.filter((column) =>
        visibleColumnIds.includes(column.id),
      ),
    [visibleColumnIds],
  );
  const gridTemplate = useMemo(
    () => gridTemplateColumns(columns, visibleColumnIds),
    [columns, visibleColumnIds],
  );

  // Resolve the row height from CSS (--row-height) so the window follows the
  // density setting. The fallback is the design's 30px, for jsdom and for the
  // first paint before the stylesheet has been read.
  const [rowHeight, setRowHeight] = useState(resolveRowHeight());
  useEffect(() => {
    setRowHeight(resolveRowHeight());
  }, []);

  const { range, scrollToIndex } = useVirtualizer(scrollRef, {
    count: visible.length,
    rowHeight,
    overscan: 10,
  });

  // Reset scroll to top when filter/search/sort changes (avoid blank viewport
  // after narrowing from 5k to a handful).
  const filterKey = `${JSON.stringify(facets)}|${search}|${sortColumn}|${sortDir}`;
  const prevFilterKey = useRef(filterKey);
  useEffect(() => {
    if (prevFilterKey.current !== filterKey) {
      prevFilterKey.current = filterKey;
      if (scrollRef.current) scrollRef.current.scrollTop = 0;
    }
  }, [filterKey]);

  // Handle "scroll to torrent" requests (notification click, keyboard nav).
  // App.tsx dispatches a CustomEvent; the table scrolls the hash into view.
  useEffect(() => {
    const onScrollTo = (event: Event) => {
      const detail = (event as CustomEvent<string>).detail;
      if (!detail) return;
      const idx = visible.findIndex((t) => t.hash === detail);
      if (idx !== -1) scrollToIndex(idx, "auto");
    };
    window.addEventListener("scrollToTorrent" as never, onScrollTo as never);
    return () =>
      window.removeEventListener(
        "scrollToTorrent" as never,
        onScrollTo as never,
      );
  }, [visible, scrollToIndex]);

  // Also handle hash param for deep-linked scroll (e.g. tests).
  useEffect(() => {
    const hash = window.location.hash.slice(1);
    if (!hash) return;
    const idx = visible.findIndex((t) => t.hash === hash);
    if (idx !== -1) scrollToIndex(idx, "center");
  }, [visible, scrollToIndex]);

  /** Row mouse-down: apply click / ⌘-toggle / ⇧-range selection. */
  const onRowMouseDown = useCallback(
    (hash: string, event: React.MouseEvent) => {
      if (event.shiftKey) {
        selectRange(
          hash,
          visible.map((torrent) => torrent.hash),
        );
      } else if (event.metaKey || event.ctrlKey) {
        toggle(hash);
      } else {
        select(hash);
      }
    },
    [select, selectRange, toggle, visible],
  );

  /** Right-click selects the row (if needed), then opens its action menu. */
  const onRowContextMenu = useCallback(
    (hash: string, event: React.MouseEvent) => {
      event.preventDefault();
      if (!selection.has(hash)) select(hash);
      openContextMenu(event.clientX, event.clientY);
    },
    [openContextMenu, select, selection],
  );

  const startResize = (
    id: ColumnId,
    event: React.PointerEvent<HTMLSpanElement>,
  ) => {
    event.preventDefault();
    event.stopPropagation();
    suppressSort.current = true;
    event.currentTarget.setPointerCapture(event.pointerId);
    const cellWidth =
      event.currentTarget.parentElement?.getBoundingClientRect().width ??
      columns.widths[id];
    resizeSession.current = {
      id,
      pointerId: event.pointerId,
      startX: event.clientX,
      startWidth: cellWidth,
      moved: false,
    };
  };

  const moveResize = (event: React.PointerEvent<HTMLSpanElement>) => {
    const session = resizeSession.current;
    if (!session || session.pointerId !== event.pointerId) return;
    const delta = event.clientX - session.startX;
    if (Math.abs(delta) >= 2) session.moved = true;
    resizeColumn(session.id, session.startWidth + delta);
  };

  const finishResize = (event: React.PointerEvent<HTMLSpanElement>) => {
    const session = resizeSession.current;
    if (!session || session.pointerId !== event.pointerId) return;
    event.stopPropagation();
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    resizeSession.current = null;
    suppressSort.current = session.moved;
    window.setTimeout(() => {
      suppressSort.current = false;
    }, 0);
  };

  /** Back to the whole list: every dimension cleared, search box emptied. */
  const resetFilters = () => {
    const ui = useUi.getState();
    ui.clearFacets();
    ui.setSearch("");
  };

  const onHeaderContextMenu = (event: React.MouseEvent) => {
    event.preventDefault();
    openColumnMenu(event.clientX, event.clientY);
  };

  const order = visible.map((torrent) => torrent.hash);
  const allSelected =
    order.length > 0 && order.every((hash) => selection.has(hash));
  const someSelected = order.some((hash) => selection.has(hash));

  const header = (
    <div
      className={`${styles.grid} ${styles.header}`}
      role="row"
      onContextMenu={onHeaderContextMenu}
    >
      {shownColumns.map((column) => {
        const definition = columnDefinition(column.id);
        const sort = SORT_COLUMNS[column.id];

        // The selection box: one control for the whole visible set.
        if (column.id === "select") {
          return (
            <span
              key={column.id}
              role="columnheader"
              className={styles.selectCell}
            >
              <button
                type="button"
                role="checkbox"
                aria-checked={
                  allSelected ? true : someSelected ? "mixed" : false
                }
                aria-label={
                  allSelected ? "Clear selection" : "Select all torrents"
                }
                className={`${styles.checkbox} ${allSelected ? styles.checkboxOn : ""}`}
                onClick={() =>
                  allSelected ? clearSelection() : selectAll(order)
                }
              >
                {allSelected ? "✓" : someSelected ? "–" : ""}
              </button>
            </span>
          );
        }

        return (
          <span
            key={column.id}
            role="columnheader"
            // Which column the table is ordered by, and which way, is the
            // header's own business — the caret only says it to the eye.
            aria-sort={
              sort
                ? sort === sortColumn
                  ? sortDir === "asc"
                    ? "ascending"
                    : "descending"
                  : "none"
                : undefined
            }
            className={`${sort ? styles.sortable : ""} ${
              definition.numeric ? styles.right : ""
            } ${dragging === column.id ? styles.dragging : ""}`}
            draggable={!definition.locked}
            onDragStart={() => setDragging(column.id)}
            onDragEnd={() => setDragging(null)}
            onDragOver={(event) => {
              // Only a dragged header can reorder; text drops are unrelated.
              if (dragging) event.preventDefault();
            }}
            onDrop={(event) => {
              if (!dragging) return;
              event.preventDefault();
              moveColumn(dragging, column.id);
              setDragging(null);
            }}
            onClick={() => {
              if (sort && !suppressSort.current) setSort(sort);
            }}
          >
            {column.label}
            {sort === sortColumn && (
              <span className={styles.arrow}>
                {sortDir === "asc" ? "▲" : "▼"}
              </span>
            )}
            {!definition.locked && (
              <span
                className={styles.resizeHandle}
                title={`Resize ${column.label} column`}
                onPointerDown={(event) => startResize(column.id, event)}
                onPointerMove={moveResize}
                onPointerUp={finishResize}
                onPointerCancel={finishResize}
                onClick={(event) => event.stopPropagation()}
              />
            )}
          </span>
        );
      })}
    </div>
  );

  let body: React.ReactNode;
  // A rowgroup must contain rows, so the empty and loading states are marked as
  // the layout they are rather than pretending to be structure.
  const bodyProps = {
    className: styles.body,
    ref: scrollRef,
    "data-testid": "torrent-table-body",
    role: (visible.length > 0 ? "rowgroup" : "presentation") as
      "rowgroup" | "presentation",
  };

  if (!hasLoaded) {
    // First load: the table chrome with shimmering placeholder rows, as the
    // design asks, rather than a spinner over an empty pane.
    body = (
      <div {...bodyProps}>
        {Array.from({ length: SHIMMER_ROWS }, (_, index) => (
          <div
            key={index}
            className={`${styles.grid} ${styles.row} ${
              index % 2 === 1 ? styles.alt : ""
            } ${styles.shimmerRow}`}
            aria-hidden="true"
          >
            {visibleColumnIds.map((id) => (
              <span key={id} className={styles.shimmerCell} />
            ))}
          </div>
        ))}
      </div>
    );
  } else if (visible.length === 0) {
    body = (
      <div className={styles.empty}>
        {torrents.length === 0 ? (
          <>
            <span>no torrents yet</span>
            <span className={styles.emptyHint}>
              {accel("O")} to add a .torrent, {accel("O", { shift: true })} for
              a magnet
            </span>
          </>
        ) : (
          <>
            <span>No torrents match this filter</span>
            <button
              type="button"
              className={styles.emptyAction}
              onClick={resetFilters}
            >
              Clear filters
            </button>
          </>
        )}
      </div>
    );
  } else {
    const slice = visible.slice(range.start, range.end + 1);
    body = (
      <div {...bodyProps}>
        <div
          className={styles.virtualSpacer}
          role="presentation"
          style={{ height: range.totalHeight }}
        >
          <div
            className={styles.virtualInner}
            role="presentation"
            style={{ transform: `translateY(${range.offsetY}px)` }}
          >
            {slice.map((torrent, idx) => {
              const actualIndex = range.start + idx;
              return (
                <Row
                  key={torrent.hash}
                  torrent={torrent}
                  // +1 for the header row, which is row 1 to a screen reader.
                  rowIndex={actualIndex + 2}
                  alt={actualIndex % 2 === 1}
                  selected={selection.has(torrent.hash)}
                  visibleColumnIds={visibleColumnIds}
                  onMouseDown={onRowMouseDown}
                  onContextMenu={onRowContextMenu}
                  onToggleSelect={toggle}
                  entering={entering.has(torrent.hash)}
                />
              );
            })}
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      className={styles.table}
      role="grid"
      aria-label="Torrents"
      // Rows are the visible set plus the header; columns are the shown ones,
      // so a screen reader can say "row 12 of 240" as the filter changes.
      aria-rowcount={visible.length + 1}
      aria-colcount={shownColumns.length}
      style={{ "--torrent-grid-template": gridTemplate } as React.CSSProperties}
    >
      {header}
      {body}
    </div>
  );
}
