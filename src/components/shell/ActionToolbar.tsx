/**
 * The action toolbar (design frame 1a): transport controls, the verbs that act
 * on the selection, and the readout that says how much of the list is showing.
 *
 * Replaces the desktop's icon-only `Toolbar` and the web's `ActionStrip` — one
 * bar for both shells, so the two cannot drift. Every button but the readout is
 * disabled with an empty selection, which is what the design's dimmed state is
 * for.
 */

import { useTorrents } from "../../store/torrents";
import { useUi } from "../../store/ui";
import { selectVisible } from "../../store/selectors";
import * as actions from "../../actions";
import { PlayIcon, PauseIcon, StopIcon } from "../icons";
import styles from "./ActionToolbar.module.css";
import appStyles from "../../App.module.css";

export function ActionToolbar() {
  const torrents = useTorrents((s) => s.torrents);
  const selection = useUi((s) => s.selection);
  const facets = useUi((s) => s.facets);
  const search = useUi((s) => s.search);
  const sortColumn = useUi((s) => s.sortColumn);
  const sortDir = useUi((s) => s.sortDir);
  const smartFilters = useUi((s) => s.smartFilters);
  const fileMatches = useUi((s) => s.fileMatches);
  const openDialog = useUi((s) => s.openDialog);
  const toggleSidebar = useUi((s) => s.toggleSidebar);

  const selected = selection.size;
  const hasSelection = selected > 0;
  const shown = selectVisible(
    torrents,
    facets,
    search,
    sortColumn,
    sortDir,
    smartFilters,
    fileMatches,
  ).length;

  return (
    <div className={styles.bar}>
      {/* Narrow windows only: the sidebar is otherwise always on screen. */}
      <button
        type="button"
        className={`${styles.btn} ${appStyles.sidebarToggle}`}
        onClick={toggleSidebar}
        title="Filters"
      >
        Filters
      </button>
      <button
        type="button"
        className={`${styles.btn} ${styles.transport}`}
        disabled={!hasSelection}
        onClick={() => actions.resume()}
        title="Start (Space)"
      >
        <PlayIcon size={10} />
        Start
      </button>
      <button
        type="button"
        className={`${styles.btn} ${styles.transport}`}
        disabled={!hasSelection}
        onClick={() => actions.pause()}
        title="Pause (Space)"
      >
        <PauseIcon size={10} />
        Pause
      </button>
      <button
        type="button"
        className={`${styles.btn} ${styles.transport}`}
        disabled={!hasSelection}
        onClick={() => actions.stop()}
        title="Stop — closes the torrent in the daemon"
      >
        <StopIcon size={10} />
        Stop
      </button>

      <span className={styles.divider} />

      <button
        type="button"
        className={styles.btn}
        disabled={!hasSelection}
        onClick={() => actions.recheck()}
        title="Force recheck"
      >
        Force recheck
      </button>
      <button
        type="button"
        className={styles.btn}
        disabled={!hasSelection}
        onClick={() => openDialog("set-label")}
        title="Set label"
      >
        Set label
      </button>
      <button
        type="button"
        className={styles.btn}
        disabled={!hasSelection}
        onClick={() => openDialog("set-tags")}
        title="Edit tags"
      >
        Edit tags
      </button>
      <button
        type="button"
        className={styles.btn}
        disabled={selected !== 1}
        onClick={() => openDialog("set-location")}
        title="Move data — one torrent at a time"
      >
        Move data
      </button>
      <button
        type="button"
        className={styles.btn}
        disabled={!hasSelection}
        onClick={() => actions.queueTop()}
        title="Move to top (priority band + queue sequence)"
      >
        Top
      </button>
      <button
        type="button"
        className={styles.btn}
        disabled={!hasSelection}
        onClick={() => actions.queueUp()}
        title="Move up one rank (swaps priority band + sequence with the neighbour)"
      >
        Priority ↑
      </button>
      <button
        type="button"
        className={styles.btn}
        disabled={!hasSelection}
        onClick={() => actions.queueDown()}
        title="Move down one rank"
      >
        Priority ↓
      </button>
      <button
        type="button"
        className={styles.btn}
        disabled={!hasSelection}
        onClick={() => actions.queueBottom()}
        title="Move to bottom"
      >
        Bottom
      </button>
      <button
        type="button"
        className={styles.btn}
        disabled={!hasSelection}
        onClick={() => actions.requestRemove()}
        title="Remove (⌫)"
      >
        Remove
      </button>

      <span className={styles.grow} />

      <span className={styles.readout}>
        {selected} selected · {shown} of {torrents.length} shown
      </span>
    </div>
  );
}
