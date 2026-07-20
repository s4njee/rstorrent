/**
 * Sortable torrent table with persisted column visibility and live resizing.
 * Selection follows native list conventions (click, ⌘-click, ⇧-click range).
 */

import { useCallback, useMemo, useRef } from "react";
import { useShallow } from "zustand/react/shallow";
import { useTorrents } from "../../store/torrents";
import { useUi, type SortColumn } from "../../store/ui";
import { selectVisible } from "../../store/selectors";
import { accel } from "../../platform";
import {
  COLUMN_DEFINITIONS,
  gridTemplateColumns,
  type ColumnId,
} from "./columns";
import { Row } from "./Row";
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
  started: "startedAt",
  finished: "finishedAt",
};

const RIGHT_COLUMNS = new Set<ColumnId>([
  "size",
  "seeds",
  "peers",
  "down",
  "up",
  "eta",
  "ratio",
]);

interface ResizeSession {
  id: ColumnId;
  pointerId: number;
  startX: number;
  startWidth: number;
  moved: boolean;
}

export function TorrentTable() {
  const torrents = useTorrents((state) => state.torrents);
  const connection = useTorrents((state) => state.connection);
  const { filter, search, sortColumn, sortDir, columns } = useUi(
    useShallow((state) => ({
      filter: state.filter,
      search: state.search,
      sortColumn: state.sortColumn,
      sortDir: state.sortDir,
      columns: state.columns,
    })),
  );
  const selection = useUi((state) => state.selection);
  const smartFilters = useUi((state) => state.smartFilters);
  const setSort = useUi((state) => state.setSort);
  const resizeColumn = useUi((state) => state.resizeColumn);
  const select = useUi((state) => state.select);
  const toggle = useUi((state) => state.toggle);
  const selectRange = useUi((state) => state.selectRange);
  const openContextMenu = useUi((state) => state.openContextMenu);
  const openColumnMenu = useUi((state) => state.openColumnMenu);

  const resizeSession = useRef<ResizeSession | null>(null);
  const suppressSort = useRef(false);

  const visible = useMemo(
    () =>
      selectVisible(
        torrents,
        filter,
        search,
        sortColumn,
        sortDir,
        smartFilters,
      ),
    [torrents, filter, search, sortColumn, sortDir, smartFilters],
  );
  const columnVisibility = columns.visibility;
  const shownColumns = useMemo(
    () => COLUMN_DEFINITIONS.filter((column) => columnVisibility[column.id]),
    [columnVisibility],
  );
  const visibleColumnIds = useMemo(
    () => shownColumns.map((column) => column.id),
    [shownColumns],
  );
  const gridTemplate = useMemo(() => gridTemplateColumns(columns), [columns]);

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

  const onHeaderContextMenu = (event: React.MouseEvent) => {
    event.preventDefault();
    openColumnMenu(event.clientX, event.clientY);
  };

  const header = (
    <div
      className={`${styles.grid} ${styles.header}`}
      onContextMenu={onHeaderContextMenu}
    >
      {shownColumns.map((column) => {
        const sort = SORT_COLUMNS[column.id];
        return (
          <span
            key={column.id}
            className={`${sort ? styles.sortable : ""} ${
              RIGHT_COLUMNS.has(column.id) ? styles.right : ""
            }`}
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
            <span
              className={styles.resizeHandle}
              title={`Resize ${column.label} column`}
              onPointerDown={(event) => startResize(column.id, event)}
              onPointerMove={moveResize}
              onPointerUp={finishResize}
              onPointerCancel={finishResize}
              onClick={(event) => event.stopPropagation()}
            />
          </span>
        );
      })}
    </div>
  );

  let body: React.ReactNode;
  if (visible.length === 0 && connection.phase === "connected") {
    if (torrents.length === 0) {
      body = (
        <div className={styles.empty}>
          no torrents
          <br />
          {accel("O")} to add a .torrent, {accel("O", { shift: true })} for a
          magnet
        </div>
      );
    } else {
      body = (
        <div className={styles.empty}>
          no torrents match
          <br />
          <a onClick={() => useUi.getState().setFilter(null)}>clear filter</a>
        </div>
      );
    }
  } else {
    body = (
      <div className={styles.body}>
        {visible.map((torrent, index) => (
          <Row
            key={torrent.hash}
            torrent={torrent}
            alt={index % 2 === 1}
            selected={selection.has(torrent.hash)}
            visibleColumnIds={visibleColumnIds}
            onMouseDown={onRowMouseDown}
            onContextMenu={onRowContextMenu}
          />
        ))}
      </div>
    );
  }

  return (
    <div
      className={styles.table}
      style={{ "--torrent-grid-template": gridTemplate } as React.CSSProperties}
    >
      {header}
      {body}
    </div>
  );
}
