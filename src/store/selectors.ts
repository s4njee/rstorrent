/**
 * Pure derivations over the torrent list: the visible (filtered + searched +
 * sorted) rows, and the sidebar group counts. Kept free of React/store so they
 * can be unit-tested directly and memoized in components.
 */

import type { TorrentDto } from "../ipc/types";
import type {
  ActiveFilter,
  SmartFilter,
  SmartFilterCriteria,
  SortColumn,
  SortDir,
} from "./ui";

/** Does a torrent match a status key? "completed" is a superset. */
function matchesStatus(t: TorrentDto, status: string): boolean {
  if (status === "completed") return t.percent >= 100;
  return t.status === status;
}

/**
 * Does a torrent satisfy every present criterion (AND)? Absent fields are
 * unconstrained. This is what makes a smart filter multi-dimension, unlike the
 * single-dimension sidebar filters it's built from.
 */
export function matchesCriteria(
  t: TorrentDto,
  criteria: SmartFilterCriteria,
): boolean {
  if (criteria.status && !matchesStatus(t, criteria.status)) return false;
  if (criteria.label && t.label !== criteria.label) return false;
  if (criteria.tracker && t.trackerHost !== criteria.tracker) return false;
  if (criteria.text && !matchesSearch(t, criteria.text)) return false;
  return true;
}

/** Does a torrent match the active sidebar filter? */
function matchesFilter(
  t: TorrentDto,
  filter: ActiveFilter,
  smartFilters: SmartFilter[],
): boolean {
  if (!filter) return true;
  switch (filter.type) {
    case "status":
      return matchesStatus(t, filter.value);
    case "label":
      return t.label === filter.value;
    case "tracker":
      return t.trackerHost === filter.value;
    case "smart": {
      const saved = smartFilters.find((f) => f.id === filter.value);
      // A dangling id shows everything rather than an unexplained empty table
      // (the store also drops dangling references on load and on delete).
      return saved ? matchesCriteria(t, saved) : true;
    }
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
    case "startedAt":
      return t.startedAt;
    case "finishedAt":
      return t.finishedAt;
  }
}

/**
 * Filter + search + sort. Returns a new array; input is not mutated.
 *
 * `smartFilters` is only needed to resolve a `{type:'smart'}` filter by id; it
 * defaults to empty so callers with no smart filters are unaffected. The search
 * box always ANDs on top, including over a smart filter's own text.
 */
export function selectVisible(
  torrents: TorrentDto[],
  filter: ActiveFilter,
  search: string,
  sortColumn: SortColumn,
  sortDir: SortDir,
  smartFilters: SmartFilter[] = [],
): TorrentDto[] {
  const rows = torrents.filter(
    (t) => matchesFilter(t, filter, smartFilters) && matchesSearch(t, search),
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

/**
 * Row counts per saved smart filter, keyed by id, for the sidebar. Computed
 * over the unfiltered list like the other sidebar counts, so a group's numbers
 * don't change as you click around.
 */
export function smartFilterCounts(
  torrents: TorrentDto[],
  smartFilters: SmartFilter[],
): Record<string, number> {
  const counts: Record<string, number> = {};
  for (const filter of smartFilters) {
    counts[filter.id] = torrents.reduce(
      (n, t) => (matchesCriteria(t, filter) ? n + 1 : n),
      0,
    );
  }
  return counts;
}

/** Aggregate figures for the multi-selection summary bar (C3). */
export interface SelectionSummary {
  count: number;
  size: number;
  downRate: number;
  upRate: number;
  /** How many of the selected torrents are stopped — drives Resume/Pause. */
  paused: number;
}

/**
 * Summarize the selected torrents. Selection can name hashes that are gone (a
 * removal between snapshot and render), so this counts what actually resolves
 * rather than trusting `selection.size`.
 */
export function selectionSummary(
  torrents: TorrentDto[],
  selection: Set<string>,
): SelectionSummary {
  const summary: SelectionSummary = {
    count: 0,
    size: 0,
    downRate: 0,
    upRate: 0,
    paused: 0,
  };
  for (const t of torrents) {
    if (!selection.has(t.hash)) continue;
    summary.count += 1;
    summary.size += t.size;
    summary.downRate += t.downRate;
    summary.upRate += t.upRate;
    if (t.status === "paused") summary.paused += 1;
  }
  return summary;
}
