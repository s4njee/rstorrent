/**
 * Filter sidebar. Clicking a Status/Label/Tracker row sets the active filter
 * (clicking the active row again clears back to "all"). Counts are global
 * (computed over the unfiltered list) per the design.
 *
 * The Smart group (C4) holds saved multi-dimension queries. "+" saves the
 * current view — the active dimension filter plus the search text — under a
 * name, so a query you'd otherwise retype is one click away.
 */

import { useMemo, useState } from "react";
import { useTorrents } from "../../store/torrents";
import { canSaveSmartFilter, useUi, type ActiveFilter } from "../../store/ui";
import { sidebarCounts, smartFilterCounts } from "../../store/selectors";
import { setLabel } from "../../ipc/commands";
import styles from "./FilterSidebar.module.css";
import menuStyles from "../menu/ContextMenu.module.css";

/** The fixed Status rows, in the design's order. */
const STATUS_ROWS: Array<{ key: string; label: string }> = [
  { key: "all", label: "all" },
  { key: "downloading", label: "downloading" },
  { key: "seeding", label: "seeding" },
  { key: "completed", label: "completed" },
  { key: "paused", label: "paused" },
  { key: "stalled", label: "stalled" },
  { key: "error", label: "error" },
];

export function FilterSidebar() {
  const torrents = useTorrents((s) => s.torrents);
  const filter = useUi((s) => s.filter);
  const setFilter = useUi((s) => s.setFilter);
  const search = useUi((s) => s.search);
  const smartFilters = useUi((s) => s.smartFilters);
  const saveSmartFilter = useUi((s) => s.saveSmartFilter);
  const removeSmartFilter = useUi((s) => s.removeSmartFilter);

  const [naming, setNaming] = useState(false);
  const [draftName, setDraftName] = useState("");

  // Label right-click menu (C5) and the inline rename it opens.
  const [labelMenu, setLabelMenu] = useState<{
    x: number;
    y: number;
    value: string;
  } | null>(null);
  const [renaming, setRenaming] = useState<{
    value: string;
    draft: string;
  } | null>(null);

  const counts = useMemo(() => sidebarCounts(torrents), [torrents]);
  const smartCounts = useMemo(
    () => smartFilterCounts(torrents, smartFilters),
    [torrents, smartFilters],
  );
  const canSave = canSaveSmartFilter(filter, search);

  const commitName = () => {
    saveSmartFilter(draftName);
    setDraftName("");
    setNaming(false);
  };

  /** Toggle a filter: re-clicking the active one clears it. */
  const choose = (next: ActiveFilter) => {
    const same =
      next &&
      filter &&
      next.type === filter.type &&
      next.value === filter.value;
    setFilter(same ? null : next);
  };

  const isActive = (type: string, value: string) =>
    (value === "all" && !filter) ||
    (filter?.type === type && filter.value === value);

  /** Hashes currently carrying a given label. */
  const hashesWithLabel = (label: string) =>
    torrents.filter((t) => t.label === label).map((t) => t.hash);

  /** Rewrite (or clear, when `next` is "") a label across every torrent that
   *  has it. If the active filter was that label, retarget it so the view
   *  doesn't strand empty. */
  const applyLabel = (from: string, next: string) => {
    const hashes = hashesWithLabel(from);
    if (hashes.length) void setLabel(hashes, next);
    if (filter?.type === "label" && filter.value === from) {
      setFilter(next ? { type: "label", value: next } : null);
    }
  };

  const commitRename = () => {
    if (!renaming) return;
    const next = renaming.draft.trim();
    if (next && next !== renaming.value) applyLabel(renaming.value, next);
    setRenaming(null);
  };

  return (
    <div className={styles.sidebar}>
      <div className={styles.group} style={{ paddingTop: 2 }}>
        Status
      </div>
      {STATUS_ROWS.map((row) => (
        <div
          key={row.key}
          className={`${styles.row} ${isActive("status", row.key) ? styles.active : ""}`}
          onClick={() =>
            choose(
              row.key === "all" ? null : { type: "status", value: row.key },
            )
          }
        >
          <span className={styles.label}>{row.label}</span>
          <span className={styles.count}>{counts.status[row.key] ?? 0}</span>
        </div>
      ))}

      {counts.labels.length > 0 && <div className={styles.group}>Labels</div>}
      {counts.labels.map((l) =>
        renaming?.value === l.value ? (
          <div key={l.value} className={styles.naming}>
            <input
              className={styles.nameInput}
              autoFocus
              value={renaming.draft}
              onChange={(e) =>
                setRenaming({ value: l.value, draft: e.currentTarget.value })
              }
              onKeyDown={(e) => {
                if (e.key === "Enter") commitRename();
                if (e.key === "Escape") {
                  e.stopPropagation();
                  setRenaming(null);
                }
              }}
              onBlur={commitRename}
            />
          </div>
        ) : (
          <div
            key={l.value}
            className={`${styles.row} ${isActive("label", l.value) ? styles.active : ""}`}
            onClick={() => choose({ type: "label", value: l.value })}
            onContextMenu={(e) => {
              e.preventDefault();
              setLabelMenu({ x: e.clientX, y: e.clientY, value: l.value });
            }}
          >
            <span className={styles.label}>{l.value}</span>
            <span className={styles.count}>{l.count}</span>
          </div>
        ),
      )}

      {counts.trackers.length > 0 && (
        <div className={styles.group}>Trackers</div>
      )}
      {counts.trackers.map((t) => (
        <div
          key={t.value}
          className={`${styles.row} ${isActive("tracker", t.value) ? styles.active : ""}`}
          onClick={() => choose({ type: "tracker", value: t.value })}
          title={t.value}
        >
          <span className={styles.label}>{t.value}</span>
          <span className={styles.count}>{t.count}</span>
        </div>
      ))}

      {/* Native rtorrent views (D12): the daemon's own membership groups. */}
      {counts.views.length > 0 && <div className={styles.group}>Views</div>}
      {counts.views.map((v) => (
        <div
          key={v.value}
          className={`${styles.row} ${isActive("view", v.value) ? styles.active : ""}`}
          onClick={() => choose({ type: "view", value: v.value })}
          title={`rtorrent view: ${v.value}`}
        >
          <span className={styles.label}>{v.value}</span>
          <span className={styles.count}>{v.count}</span>
        </div>
      ))}

      <div className={`${styles.group} ${styles.groupWithAction}`}>
        <span>Smart</span>
        <button
          className={styles.groupAction}
          disabled={!canSave || naming}
          title={
            filter?.type === "smart"
              ? "already viewing a smart filter"
              : canSave
                ? "save this view as a smart filter"
                : "pick a filter or type a search first"
          }
          onClick={() => setNaming(true)}
          aria-label="Save current view as a smart filter"
        >
          +
        </button>
      </div>

      {naming && (
        <div className={styles.naming}>
          <input
            className={styles.nameInput}
            placeholder="name…"
            autoFocus
            value={draftName}
            onChange={(e) => setDraftName(e.currentTarget.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitName();
              if (e.key === "Escape") {
                // Don't let the global Escape also clear the selection.
                e.stopPropagation();
                setNaming(false);
                setDraftName("");
              }
            }}
            onBlur={() => {
              setNaming(false);
              setDraftName("");
            }}
          />
        </div>
      )}

      {smartFilters.map((f) => (
        <div
          key={f.id}
          className={`${styles.row} ${isActive("smart", f.id) ? styles.active : ""}`}
          onClick={() => choose({ type: "smart", value: f.id })}
          title={describeCriteria(f)}
        >
          <span className={styles.label}>{f.name}</span>
          <span className={styles.count}>{smartCounts[f.id] ?? 0}</span>
          <button
            className={styles.removeSmart}
            aria-label={`Remove smart filter ${f.name}`}
            title="remove"
            onClick={(e) => {
              // The row's own click would otherwise re-activate the filter.
              e.stopPropagation();
              removeSmartFilter(f.id);
            }}
          >
            ✕
          </button>
        </div>
      ))}

      {smartFilters.length === 0 && !naming && (
        <div className={styles.smartHint}>filter or search, then + to save</div>
      )}

      {labelMenu && (
        <>
          <div
            className={menuStyles.overlay}
            onMouseDown={() => setLabelMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault();
              setLabelMenu(null);
            }}
          />
          <div
            className={menuStyles.menu}
            style={{
              left: Math.min(labelMenu.x, window.innerWidth - 180),
              top: Math.min(labelMenu.y, window.innerHeight - 80),
            }}
          >
            <div
              className={menuStyles.item}
              onClick={() => {
                setRenaming({ value: labelMenu.value, draft: labelMenu.value });
                setLabelMenu(null);
              }}
            >
              Rename label…
            </div>
            <div
              className={`${menuStyles.item} ${menuStyles.danger}`}
              onClick={() => {
                applyLabel(labelMenu.value, "");
                setLabelMenu(null);
              }}
            >
              Remove label
            </div>
          </div>
        </>
      )}
    </div>
  );
}

/** Human-readable criteria for a smart filter's tooltip. */
function describeCriteria(f: {
  status?: string;
  label?: string;
  tracker?: string;
  text?: string;
}): string {
  const parts = [
    f.status && `status: ${f.status}`,
    f.label && `label: ${f.label}`,
    f.tracker && `tracker: ${f.tracker}`,
    f.text && `text: "${f.text}"`,
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : "matches everything";
}
