/**
 * Pure derivations over the torrent list: the visible (filtered + searched +
 * sorted) rows, and the sidebar group counts. Kept free of React/store so they
 * can be unit-tested directly and memoized in components.
 */

import type { TorrentDto } from "../ipc/types";
import type {
  Facets,
  SmartFilter,
  SmartFilterCriteria,
  SortColumn,
  SortDir,
} from "./ui";

/** Does a torrent match a status key? "completed" is a superset. */
function matchesStatus(t: TorrentDto, status: string): boolean {
  if (status === "completed") return t.percent >= 100;
  // Error buckets (D19): `error` is the aggregate, sub-keys like `tracker_timeout`
  // or `missing_files` match the classified `errorKind`.
  if (status === "error") return t.status === "error";
  if (status.startsWith("error:")) {
    return t.errorKind === status.slice(6);
  }
  // Allow direct errorKind values as status filters (sidebar buckets)
  if (
    [
      "unregistered",
      "tracker_timeout",
      "tracker_error",
      "missing_files",
      "no_space",
      "permission",
      "disk_error",
      "other",
    ].includes(status)
  ) {
    return t.errorKind === status;
  }
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
  if (criteria.errorKind && t.errorKind !== criteria.errorKind) return false;
  if (criteria.label && t.label !== criteria.label) return false;
  if (criteria.tags && !(t.tags ?? []).includes(criteria.tags)) return false;
  if (criteria.tracker && t.trackerHost !== criteria.tracker) return false;
  if (criteria.view && !t.views.includes(criteria.view)) return false;
  if (criteria.text && !matchesSearch(t, criteria.text)) return false;
  return true;
}

/**
 * Does a torrent satisfy the live facets? Every constrained dimension must
 * match (AND), and the search box ANDs on top.
 */
export function matchesFacets(
  t: TorrentDto,
  facets: Facets,
  smartFilters: SmartFilter[] = [],
): boolean {
  if (facets.status && !matchesStatus(t, facets.status)) return false;
  if (facets.errorKind && t.errorKind !== facets.errorKind) return false;
  // `label: ""` is the unlabeled facet, so test for the key, not for truth.
  if (facets.label !== undefined && t.label !== facets.label) return false;
  if (facets.tags !== undefined && !(t.tags ?? []).includes(facets.tags)) {
    return false;
  }
  if (facets.tracker && t.trackerHost !== facets.tracker) return false;
  if (facets.view && !t.views.includes(facets.view)) return false;
  if (facets.smart) {
    const saved = smartFilters.find((f) => f.id === facets.smart);
    // A dangling id shows everything rather than an unexplained empty table
    // (the store also drops dangling references on load and on delete).
    if (saved && !matchesCriteria(t, saved)) return false;
  }
  return true;
}

/** Case-insensitive substring match across name, hash, label, tags, tracker and save path. */
function matchesSearch(t: TorrentDto, search: string): boolean {
  if (!search) return true;
  const q = search.toLowerCase();
  return (
    t.name.toLowerCase().includes(q) ||
    t.hash.toLowerCase().includes(q) ||
    t.label.toLowerCase().includes(q) ||
    (t.tags ?? []).some((tag) => tag.toLowerCase().includes(q)) ||
    t.trackerHost.toLowerCase().includes(q) ||
    (t.savePath ?? "").toLowerCase().includes(q)
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
    case "addedAt":
      // rtorrent reports 0 when it does not know; those sort oldest.
      return t.addedAt ?? 0;
    case "startedAt":
      return t.startedAt;
    case "finishedAt":
      return t.finishedAt;
  }
}

/**
 * Filter + search + sort. Returns a new array; input is not mutated.
 *
 * `smartFilters` is only needed to resolve a saved filter by id; it defaults to
 * empty so callers with no saved filters are unaffected. The search box always
 * ANDs on top, including over a saved filter's own text.
 */
export function selectVisible(
  torrents: TorrentDto[],
  facets: Facets,
  search: string,
  sortColumn: SortColumn,
  sortDir: SortDir,
  smartFilters: SmartFilter[] = [],
  /** Hashes whose contained filenames match `search`, from the server's index
   *  (V3-12). A torrent matches if either its own fields or its files match. */
  fileMatches?: ReadonlySet<string>,
): TorrentDto[] {
  const needle = search.trim();
  const rows = torrents.filter((t) => {
    if (!matchesFacets(t, facets, smartFilters)) return false;
    if (!needle) return true;
    return matchesSearch(t, needle) || Boolean(fileMatches?.has(t.hash));
  });
  const dir = sortDir === "asc" ? 1 : -1;
  return rows.sort((a, b) => {
    const ka = sortKey(a, sortColumn);
    const kb = sortKey(b, sortColumn);
    if (ka < kb) return -1 * dir;
    if (ka > kb) return 1 * dir;
    // Ties break on name, so an equal-valued column is still a stable list.
    return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
  });
}

export interface SidebarCounts {
  status: Record<string, number>;
  errors: Array<{ value: string; count: number }>;
  labels: Array<{ value: string; count: number }>;
  tags: Array<{ value: string; count: number }>;
  trackers: Array<{ value: string; count: number }>;
  views: Array<{ value: string; count: number }>;
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
    // `paused` covers both the console's Stopped (closed) and Queued (open but
    // idle) rows — the sidebar offers one row for the pair.
    paused: 0,
    checking: 0,
    error: 0,
    // Percent-complete superset, and the state between downloading and stalled.
    completed: 0,
    stalled: 0,
    unlabeled: 0,
  };
  const labelMap = new Map<string, number>();
  const tagMap = new Map<string, number>();
  const trackerMap = new Map<string, number>();
  const viewMap = new Map<string, number>();
  const errorMap = new Map<string, number>();

  for (const t of torrents) {
    if (t.status in status) status[t.status] += 1;
    if (t.percent >= 100) status.completed += 1;
    if (!t.label) status.unlabeled += 1;
    if (t.status === "error" && t.errorKind) {
      errorMap.set(t.errorKind, (errorMap.get(t.errorKind) ?? 0) + 1);
    }
    if (t.label) labelMap.set(t.label, (labelMap.get(t.label) ?? 0) + 1);
    // A torrent can carry several tags, so each membership counts (V3-10).
    for (const tag of t.tags ?? []) {
      tagMap.set(tag, (tagMap.get(tag) ?? 0) + 1);
    }
    if (t.trackerHost)
      trackerMap.set(t.trackerHost, (trackerMap.get(t.trackerHost) ?? 0) + 1);
    // A torrent can be in several views, so each membership counts.
    for (const v of t.views) viewMap.set(v, (viewMap.get(v) ?? 0) + 1);
  }

  const toSorted = (m: Map<string, number>) =>
    [...m.entries()]
      .map(([value, count]) => ({ value, count }))
      .sort((a, b) => a.value.localeCompare(b.value));

  return {
    status,
    errors: toSorted(errorMap),
    labels: toSorted(labelMap),
    tags: toSorted(tagMap),
    trackers: toSorted(trackerMap),
    views: toSorted(viewMap),
  };
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

/**
 * Torrents bucketed by the console's status words, for the status bar.
 *
 * Deliberately coarser than the sidebar's groups: `checking` and `stalled` are
 * things a download is doing, so they count as downloading, and `completed` (a
 * percent superset in this codebase) counts as seeding.
 */
export function statusBarCounts(torrents: Array<{ status: string }>): {
  torrents: number;
  downloading: number;
  seeding: number;
  stopped: number;
  errored: number;
} {
  const counts = {
    torrents: torrents.length,
    downloading: 0,
    seeding: 0,
    stopped: 0,
    errored: 0,
  };
  for (const torrent of torrents) {
    switch (torrent.status) {
      case "downloading":
      case "stalled":
      case "checking":
        counts.downloading += 1;
        break;
      case "seeding":
      case "completed":
        counts.seeding += 1;
        break;
      case "paused":
        counts.stopped += 1;
        break;
      case "error":
        counts.errored += 1;
        break;
      default:
        break;
    }
  }
  return counts;
}
