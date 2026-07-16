/**
 * The torrent table: sortable header + scrollable body of rows.
 *
 * Selection follows native list conventions (click, ⌘-click toggle, ⇧-click
 * range). The visible row list (filter + search + sort) comes from the pure
 * selectors, memoized against the store inputs. Empty and no-match states are
 * rendered in place of the body.
 */

import { useMemo } from "react";
import { useShallow } from "zustand/react/shallow";
import { useTorrents } from "../../store/torrents";
import { useUi, type SortColumn } from "../../store/ui";
import { selectVisible } from "../../store/selectors";
import { Row } from "./Row";
import styles from "./TorrentTable.module.css";

/** Header columns; `sort` marks which are clickable and by what key. */
const COLUMNS: Array<{ label: string; sort?: SortColumn; right?: boolean }> = [
  { label: "Name", sort: "name" },
  { label: "Size", sort: "size", right: true },
  { label: "Done", sort: "percent" },
  { label: "Status", sort: "status" },
  { label: "S", right: true },
  { label: "P", right: true },
  { label: "Down", sort: "downRate", right: true },
  { label: "Up", sort: "upRate", right: true },
  { label: "ETA", sort: "etaSeconds", right: true },
  { label: "Ratio", sort: "ratio", right: true },
  { label: "Label" },
  { label: "Tracker" },
];

export function TorrentTable() {
  const torrents = useTorrents((s) => s.torrents);
  const connection = useTorrents((s) => s.connection);
  const { filter, search, sortColumn, sortDir } = useUi(
    useShallow((s) => ({
      filter: s.filter,
      search: s.search,
      sortColumn: s.sortColumn,
      sortDir: s.sortDir,
    })),
  );
  const selection = useUi((s) => s.selection);
  const setSort = useUi((s) => s.setSort);
  const select = useUi((s) => s.select);
  const toggle = useUi((s) => s.toggle);
  const selectRange = useUi((s) => s.selectRange);
  const openContextMenu = useUi((s) => s.openContextMenu);

  const visible = useMemo(
    () => selectVisible(torrents, filter, search, sortColumn, sortDir),
    [torrents, filter, search, sortColumn, sortDir],
  );

  /** Row mouse-down: apply click / ⌘-toggle / ⇧-range selection. */
  const onRowMouseDown = (hash: string, e: React.MouseEvent) => {
    if (e.shiftKey) {
      selectRange(
        hash,
        visible.map((t) => t.hash),
      );
    } else if (e.metaKey || e.ctrlKey) {
      toggle(hash);
    } else {
      select(hash);
    }
  };

  /** Right-click selects the row (if not already selected) then opens the menu. */
  const onRowContextMenu = (hash: string, e: React.MouseEvent) => {
    e.preventDefault();
    if (!selection.has(hash)) select(hash);
    openContextMenu(e.clientX, e.clientY);
  };

  const header = (
    <div className={`${styles.grid} ${styles.header}`}>
      {COLUMNS.map((c) => (
        <span
          key={c.label}
          className={`${c.sort ? styles.sortable : ""} ${c.right ? styles.right : ""}`}
          onClick={() => c.sort && setSort(c.sort)}
        >
          {c.label}
          {c.sort === sortColumn && (
            <span className={styles.arrow}>
              {sortDir === "asc" ? "▲" : "▼"}
            </span>
          )}
        </span>
      ))}
    </div>
  );

  // Empty states: distinguish "no torrents at all" from "filter matched none".
  let body: React.ReactNode;
  if (visible.length === 0 && connection.phase === "connected") {
    if (torrents.length === 0) {
      body = (
        <div className={styles.empty}>
          no torrents
          <br />
          ⌘O to add a .torrent, ⌘⇧O for a magnet
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
        {visible.map((t, i) => (
          <Row
            key={t.hash}
            torrent={t}
            alt={i % 2 === 1}
            selected={selection.has(t.hash)}
            onMouseDown={onRowMouseDown}
            onContextMenu={onRowContextMenu}
          />
        ))}
      </div>
    );
  }

  return (
    <div className={styles.table}>
      {header}
      {body}
    </div>
  );
}
