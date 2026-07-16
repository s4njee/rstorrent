/** Header context menu for torrent-table column visibility and reset. */

import { COLUMN_DEFINITIONS } from "../table/columns";
import { useUi } from "../../store/ui";
import styles from "./ContextMenu.module.css";

export function ColumnMenu() {
  const menu = useUi((state) => state.columnMenu);
  const columns = useUi((state) => state.columns);
  const toggleColumn = useUi((state) => state.toggleColumn);
  const resetColumns = useUi((state) => state.resetColumns);
  const close = useUi((state) => state.closeColumnMenu);

  if (!menu) return null;

  const x = Math.max(4, Math.min(menu.x, window.innerWidth - 190));
  const y = Math.max(4, Math.min(menu.y, window.innerHeight - 370));

  return (
    <>
      <div
        className={styles.overlay}
        onMouseDown={close}
        onContextMenu={(event) => event.preventDefault()}
      />
      <div
        className={`${styles.menu} ${styles.columnMenu}`}
        style={{ left: x, top: y }}
        role="menu"
        aria-label="Torrent table columns"
      >
        {COLUMN_DEFINITIONS.map((column) => {
          const locked = column.id === "name";
          const visible = columns.visibility[column.id];
          return (
            <div
              key={column.id}
              className={`${styles.item} ${locked ? styles.disabled : ""}`}
              role="menuitemcheckbox"
              aria-checked={visible}
              aria-disabled={locked}
              onClick={() => {
                if (!locked) toggleColumn(column.id);
              }}
            >
              <span className={styles.check}>{visible ? "✓" : ""}</span>
              {column.label}
            </div>
          );
        })}
        <div className={styles.sep} />
        <div
          className={styles.item}
          role="menuitem"
          onClick={() => {
            resetColumns();
            close();
          }}
        >
          <span className={styles.check} />
          Reset columns
        </div>
      </div>
    </>
  );
}
