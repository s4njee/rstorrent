/**
 * Toolbar: add-file · add-magnet · remove | resume · pause | move-up · move-down,
 * with a right-aligned filter box. Action buttons are disabled when the
 * selection is empty; the two add buttons are always enabled.
 */

import { useUi } from "../../store/ui";
import { accel } from "../../platform";
import * as actions from "../../actions";
import {
  AddIcon,
  MagnetIcon,
  RemoveIcon,
  PlayIcon,
  PauseIcon,
  UpIcon,
  DownIcon,
} from "../icons";
import styles from "./Toolbar.module.css";

export function Toolbar() {
  const openDialog = useUi((s) => s.openDialog);
  const search = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const hasSelection = useUi((s) => s.selection.size > 0);

  return (
    <div className={styles.bar}>
      <button
        className={`${styles.btn} ${styles.add}`}
        title={`Add torrent file (${accel("O")})`}
        onClick={() => openDialog("add-file")}
      >
        <AddIcon />
      </button>
      <button
        className={styles.btn}
        title={`Add magnet link (${accel("O", { shift: true })})`}
        onClick={() => openDialog("add-magnet")}
      >
        <MagnetIcon />
      </button>
      <button
        className={styles.btn}
        title="Remove (⌫)"
        disabled={!hasSelection}
        onClick={() => actions.requestRemove()}
      >
        <RemoveIcon />
      </button>

      <span className={styles.sep} />

      <button
        className={styles.btn}
        title="Resume"
        disabled={!hasSelection}
        onClick={() => actions.resume()}
      >
        <PlayIcon />
      </button>
      <button
        className={styles.btn}
        title="Pause"
        disabled={!hasSelection}
        onClick={() => actions.pause()}
      >
        <PauseIcon />
      </button>

      <span className={styles.sep} />

      <button
        className={styles.btn}
        title="Move up (priority)"
        disabled={!hasSelection}
        onClick={() => actions.queueUp()}
      >
        <UpIcon />
      </button>
      <button
        className={styles.btn}
        title="Move down (priority)"
        disabled={!hasSelection}
        onClick={() => actions.queueDown()}
      >
        <DownIcon />
      </button>

      <span className={styles.grow} />

      <input
        id="filter-input"
        className={styles.filter}
        placeholder="/ filter"
        value={search}
        onChange={(e) => setSearch(e.currentTarget.value)}
        spellCheck={false}
      />
    </div>
  );
}
