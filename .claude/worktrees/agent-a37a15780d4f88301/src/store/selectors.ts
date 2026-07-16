/**
 * Pure derivations over the torrent list: the visible (filtered + searched +
 * sorted) rows, and the sidebar group counts. Kept free of React/store so they
 * can be unit-tested directly and memoized in components.
 */

import type { TorrentDto } from "../ipc/types";
import type { ActiveFilter, SortColumn, SortDir } from "./ui";

/** Does a torrent match the active sidebar filter? */
function matchesFilter(t: TorrentDto, filter: ActiveFilter): boolean {
  if (!filter) return true;
  switch (filter.type) {
    case "status":
      // "completed" is a superset (anything 100% done), else exact status.
      if (filter.value === "completed") return t.percent >= 100;
      return t.status === filter.value;
    case "label":
      return t.label === filter.value;
    case "tracker":
      return t.trackerHost === filter.value;
  }
}

/** Case-insensitive substring match across name, label, and tracker. */
function matchesSearch(t: TorrentDto, search: string): boolean {
  if (!search) return true;
  const q = search.toLowerCase();
  return (
    t.name.toLowerCase().includes(q) ||
    t.label.toLowerCase().includes(q) ||
    t.trackerHost.toLowerCase().includes(q)
  );
}

/** Comparable value for a sort column (numbers sort numerically). */
function sortKey(t: TorrentDto, col: SortColumn): number | string {
  switch (col) {
    case "name":
      return t.name.toLowerCase();
    case "size":
      return t.size;
    case "percent":
      return t.percent;
    case "status":
      return t.status;
    case "downRate":
      return t.downRate;
    case "upRate":
      return t.upRate;
    case "etaSeconds":
      // Null ETA (∞/—) sorts last in ascending order.
      return t.etaSeconds ?? Number.MAX_SAFE_INTEGER;
    case "ratio":
      return t.ratio;
  }
}

/** Filter + search + sort. Returns a new array; input is not mutated. */
export function selectVisible(
  torrents: TorrentDto[],
  filter: ActiveFilter,
  search: string,
  sortColumn: SortColumn,
  sortDir: SortDir,
): TorrentDto[] {
  const rows = torrents.filter(
    (t) => matchesFilter(t, filter) && matchesSearch(t, search),
  );
  const dir = sortDir === "asc" ? 1 : -1;
  return rows.sort((a, b) => {
    const ka = sortKey(a, sortColumn);
    const kb = sortKey(b, sortColumn);
    if (ka < kb) return -1 * dir;
    if (ka > kb) return 1 * dir;
    return 0;
  });
}

export interface SidebarCounts {
  status: Record<string, number>;
  labels: Array<{ value: string; count: number }>;
  trackers: Array<{ value: string; count: number }>;
}

/**
 * Sidebar counts over the *unfiltered* list (per the design: counts stay global
 * regardless of the active filter). Status predicates overlap — `all` and
 * `completed` are supersets — matching the mockup.
 */
export function sidebarCounts(torrents: TorrentDto[]): SidebarCounts {
  const status: Record<string, number> = {
    all: torrents.length,
    downloading: 0,
    seeding: 0,
    completed: 0,
    paused: 0,
    stalled: 0,
    error: 0,
  };
  const labelMap = new Map<string, number>();
  const trackerMap = new Map<string, number>();

  for (const t of torrents) {
    if (t.status in status) status[t.status] += 1;
    if (t.percent >= 100) status.completed += 1;
    if (t.label) labelMap.set(t.label, (labelMap.get(t.label) ?? 0) + 1);
    if (t.trackerHost)
      trackerMap.set(t.trackerHost, (trackerMap.get(t.trackerHost) ?? 0) + 1);
  }

  const toSorted = (m: Map<string, number>) =>
    [...m.entries()]
      .map(([value, count]) => ({ value, count }))
      .sort((a, b) => a.value.localeCompare(b.value));

  return { status, labels: toSorted(labelMap), trackers: toSorted(trackerMap) };
}
