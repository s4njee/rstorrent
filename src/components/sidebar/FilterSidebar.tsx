/**
 * Filter sidebar. Clicking a Status/Label/Tracker row sets that dimension of
 * the filter; clicking the active row again clears just that dimension, so
 * "downloading" and "iso" can be asked at once (the design's filters AND).
 *
 * Counts are global (computed over the unfiltered list) per the design. The
 * Smart group (C4) holds saved multi-dimension queries: "+" saves the current
 * view — the active facets plus the search text — under a name, so a query
 * you'd otherwise retype is one click away.
 */

import { useMemo, useState, type ReactNode } from "react";
import { useTorrents } from "../../store/torrents";
import { canSaveSmartFilter, useUi, type FacetKind } from "../../store/ui";
import { sidebarCounts, smartFilterCounts } from "../../store/selectors";
import { setLabel } from "../../ipc/commands";
import { tagColour } from "../../utils/tags";
import styles from "./FilterSidebar.module.css";
import menuStyles from "../menu/ContextMenu.module.css";

/**
 * The fixed Status rows, in the design's order and words.
 *
 * `value` is the rtorrent status the row filters on, `label` the console's word
 * for it — they differ where the design renames a state ("Errored" for `error`,
 * "Stopped" for `paused`). Stopped covers the console's Stopped *and* Queued
 * rows: rtorrent reports both as `paused`, and the design's sidebar has one row
 * for the pair.
 *
 * Completed and Stalled trail the design's six: both are states this daemon
 * really reports, and the design's list is what its prototype happened to show.
 */
const STATUS_ROWS: Array<{ value: string; label: string }> = [
  { value: "all", label: "All" },
  { value: "downloading", label: "Downloading" },
  { value: "seeding", label: "Seeding" },
  { value: "paused", label: "Stopped" },
  { value: "checking", label: "Checking" },
  { value: "error", label: "Errored" },
  { value: "completed", label: "Completed" },
  { value: "stalled", label: "Stalled" },
];

/**
 * The label facet's value for torrents with no label: the empty string. It
 * cannot collide with a real label (rtorrent labels are non-empty) and it rides
 * the ordinary label facet, so "unlabeled" behaves like any other row.
 */
const UNLABELED = "";

/** Error buckets (D19) — shown when there is at least one error. */
const ERROR_LABEL: Record<string, string> = {
  unregistered: "unregistered",
  tracker_timeout: "timeout",
  tracker_error: "trk error",
  missing_files: "missing files",
  no_space: "no space",
  permission: "permission",
  disk_error: "disk error",
  other: "other",
};

/**
 * @param footer Optional content pinned after the filter groups (the web shell
 *   passes the disk card here; the desktop passes nothing).
 */
export function FilterSidebar({ footer }: { footer?: ReactNode } = {}) {
  const torrents = useTorrents((s) => s.torrents);
  const facets = useUi((s) => s.facets);
  const setFacet = useUi((s) => s.setFacet);
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
  const canSave = canSaveSmartFilter(facets, search);

  const commitName = () => {
    saveSmartFilter(draftName);
    setDraftName("");
    setNaming(false);
  };

  /** Toggle a filter: re-clicking the active one clears it. */
  /**
   * Toggle one dimension. Clicking the row that is already active clears that
   * dimension alone, leaving the others in place.
   */
  const choose = (kind: FacetKind, value: string | null) => {
    setFacet(kind, value);
  };

  /** Is this row the active value for its dimension? "all" means none set. */
  const isActive = (kind: FacetKind, value: string) =>
    value === "all" ? !facets[kind] : facets[kind] === value;

  /** Hashes currently carrying a given label. */
  const hashesWithLabel = (label: string) =>
    torrents.filter((t) => t.label === label).map((t) => t.hash);

  /** Rewrite (or clear, when `next` is "") a label across every torrent that
   *  has it. If the active filter was that label, retarget it so the view
   *  doesn't strand empty. */
  const applyLabel = (from: string, next: string) => {
    const hashes = hashesWithLabel(from);
    if (hashes.length) void setLabel(hashes, next);
    if (facets.label === from) {
      setFacet("label", next || null);
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
      <div className={styles.list}>
        <div className={styles.group} style={{ paddingTop: 2 }}>
          Status
        </div>
        {STATUS_ROWS.map((row) => (
          <button
            type="button"
            key={row.value}
            className={`${styles.row} ${isActive("status", row.value) ? styles.active : ""}`}
            aria-pressed={isActive("status", row.value)}
            onClick={() =>
              choose("status", row.value === "all" ? null : row.value)
            }
          >
            <span className={styles.label}>{row.label}</span>
            <span className={styles.count}>
              {counts.status[row.value] ?? 0}
            </span>
          </button>
        ))}

        {/* Error taxonomy buckets (D19) — breakdown of the "error" status. */}
        {counts.errors.length > 0 && (
          <div className={styles.group}>Error kind</div>
        )}
        {counts.errors.map((e) => (
          <button
            type="button"
            key={e.value}
            className={`${styles.row} ${isActive("errorKind", e.value) ? styles.active : ""}`}
            aria-pressed={isActive("errorKind", e.value)}
            onClick={() => choose("errorKind", e.value)}
            title={`${ERROR_LABEL[e.value] ?? e.value}: ${e.count} torrent(s)`}
          >
            <span className={styles.label}>
              {ERROR_LABEL[e.value] ?? e.value}
            </span>
            <span className={styles.count}>{e.count}</span>
          </button>
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
            <button
              type="button"
              key={l.value}
              className={`${styles.row} ${isActive("label", l.value) ? styles.active : ""}`}
              aria-pressed={isActive("label", l.value)}
              onClick={() => choose("label", l.value)}
              onContextMenu={(e) => {
                e.preventDefault();
                setLabelMenu({ x: e.clientX, y: e.clientY, value: l.value });
              }}
            >
              <span
                className={styles.square}
                data-label={l.value}
                aria-hidden="true"
              />
              <span className={styles.label}>{l.value}</span>
              <span className={styles.count}>{l.count}</span>
            </button>
          ),
        )}

        {counts.tags.length > 0 && <div className={styles.group}>Tags</div>}
        {counts.tags.map((t) => (
          <button
            type="button"
            key={t.value}
            className={`${styles.row} ${isActive("tags", t.value) ? styles.active : ""}`}
            aria-pressed={isActive("tags", t.value)}
            onClick={() => choose("tags", t.value)}
            title={`${t.value}: ${t.count} torrent(s)`}
          >
            <span
              className={styles.square}
              style={{ background: tagColour(t.value) }}
              aria-hidden="true"
            />
            <span className={styles.label}>{t.value}</span>
            <span className={styles.count}>{t.count}</span>
          </button>
        ))}

        {counts.trackers.length > 0 && (
          <div className={styles.group}>Trackers</div>
        )}
        {counts.trackers.map((t) => (
          <button
            type="button"
            key={t.value}
            className={`${styles.row} ${isActive("tracker", t.value) ? styles.active : ""}`}
            aria-pressed={isActive("tracker", t.value)}
            onClick={() => choose("tracker", t.value)}
            title={t.value}
          >
            <span className={styles.label}>{t.value}</span>
            <span className={styles.count}>{t.count}</span>
          </button>
        ))}

        {/* Native rtorrent views (D12): the daemon's own membership groups. */}
        {counts.views.length > 0 && <div className={styles.group}>Views</div>}
        {counts.views.map((v) => (
          <button
            type="button"
            key={v.value}
            className={`${styles.row} ${isActive("view", v.value) ? styles.active : ""}`}
            aria-pressed={isActive("view", v.value)}
            onClick={() => choose("view", v.value)}
            title={`rtorrent view: ${v.value}`}
          >
            <span className={styles.label}>{v.value}</span>
            <span className={styles.count}>{v.count}</span>
          </button>
        ))}

        <div className={`${styles.group} ${styles.groupWithAction}`}>
          <span>Smart</span>
          <button
            className={styles.groupAction}
            disabled={!canSave || naming}
            title={
              facets.smart
                ? "already viewing a saved filter"
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
            className={`${styles.row} ${styles.rowWithAction} ${isActive("smart", f.id) ? styles.active : ""}`}
          >
            <button
              type="button"
              className={styles.rowAction}
              aria-pressed={isActive("smart", f.id)}
              onClick={() => choose("smart", f.id)}
              title={describeCriteria(f)}
            >
              <span className={styles.label}>{f.name}</span>
              <span className={styles.count}>{smartCounts[f.id] ?? 0}</span>
            </button>
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

        {counts.status.unlabeled > 0 && (
          <button
            type="button"
            className={`${styles.row} ${isActive("label", UNLABELED) ? styles.active : ""}`}
            aria-pressed={isActive("label", UNLABELED)}
            onClick={() => choose("label", UNLABELED)}
            title="Torrents with no label"
          >
            <span
              className={`${styles.square} ${styles.squareNeutral}`}
              aria-hidden="true"
            />
            <span className={styles.label}>unlabeled</span>
            <span className={styles.count}>{counts.status.unlabeled}</span>
          </button>
        )}

        {smartFilters.length === 0 && !naming && (
          <div className={styles.smartHint}>
            filter or search, then + to save
          </div>
        )}
      </div>

      {footer && <div className={styles.footer}>{footer}</div>}

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
    // An empty label is the unlabeled criterion, not an absent one.
    f.label !== undefined && `label: ${f.label || "unlabeled"}`,
    f.tracker && `tracker: ${f.tracker}`,
    f.text && `text: "${f.text}"`,
  ].filter(Boolean);
  return parts.length ? parts.join(" · ") : "matches everything";
}
