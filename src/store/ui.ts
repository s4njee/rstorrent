/**
 * UI store — everything about *how* the torrent list is viewed and acted on,
 * kept separate from the torrent data itself.
 *
 * Holds selection, the active sidebar filter, search text, sort column/dir, the
 * active detail tab, and which modal (if any) is open. View preferences (sort,
 * filter, tab) are persisted to localStorage so they survive relaunch.
 */

import { create } from "zustand";
import type { AddSource } from "../ipc/commands";
import { DEFAULT_PANE, parsePane, type DetailPane } from "../utils/panes";
import {
  defaultColumnState,
  deserializeColumnState,
  moveColumn as moveColumnState,
  resizeColumn as resizeColumnState,
  serializeColumnState,
  toggleColumn as toggleColumnState,
  type ColumnId,
  type ColumnState,
} from "../components/table/columns";

/** Columns the table can sort by (subset of visible columns that make sense). */
export type SortColumn =
  | "name"
  | "size"
  | "percent"
  | "status"
  | "downRate"
  | "upRate"
  | "etaSeconds"
  | "ratio"
  | "addedAt"
  | "startedAt"
  | "finishedAt";

export type SortDir = "asc" | "desc";

/**
 * The dimensions a smart filter can constrain. Every present field must match
 * (AND); absent fields are unconstrained. `text` matches like the search box.
 */
export interface SmartFilterCriteria {
  status?: string;
  errorKind?: string;
  label?: string;
  /** A single tag to match (V3-10). */
  tags?: string;
  tracker?: string;
  view?: string;
  text?: string;
}

/** A saved, named multi-dimension query (C4). */
export interface SmartFilter extends SmartFilterCriteria {
  id: string;
  name: string;
}

/**
 * The live filter: one value per dimension, all ANDed (the design's
 * "status AND label AND tracker AND text match").
 *
 * A dimension with no value is unconstrained; clicking the active row in the
 * sidebar clears that dimension rather than replacing the whole filter, so
 * "downloading" and "iso" can be asked at once.
 */
export interface Facets {
  status?: string;
  errorKind?: string;
  label?: string;
  /** A tag every shown torrent must carry (V3-10). */
  tags?: string;
  tracker?: string;
  /** A native rtorrent view name (D12). */
  view?: string;
  /** A saved filter, resolved against `smartFilters` by id. */
  smart?: string;
}

/** Which facet a sidebar row sets. */
export type FacetKind =
  "status" | "errorKind" | "label" | "tags" | "tracker" | "view" | "smart";

/** True when any dimension is constrained. */
export function hasFacets(facets: Facets): boolean {
  return Object.values(facets).some((value) => Boolean(value));
}

/** Ids only need to be unique within this app's storage, not globally. */
function newSmartFilterId(): string {
  return `sf_${Date.now().toString(36)}_${Math.random().toString(36).slice(2, 8)}`;
}

/**
 * Can the current view be saved as a smart filter?
 *
 * There must be something to save, and the active filter must not already be a
 * smart filter. That second rule keeps the feature honest: criteria hold a
 * single `text`, so "smart filter X, further narrowed by a search" has no
 * faithful representation — saving it would have to silently drop or overwrite
 * X's own text. Clear the filter and rebuild instead.
 */
export function canSaveSmartFilter(facets: Facets, search: string): boolean {
  // Saving while a saved filter is active would nest one inside the other; the
  // criteria hold a single set of dimensions, so there is nowhere to put it.
  if (facets.smart) return false;
  return hasFacets(facets) || search.trim().length > 0;
}

export type DialogKind =
  | null
  | "add-file"
  | "add-magnet"
  | "create-torrent"
  | "prefs"
  | "stats"
  | "remove"
  | "recheck"
  | "rate-limit"
  | "tune-network"
  | "shutdown"
  | "set-location"
  | "set-label"
  | "set-tags"
  | "moves"
  | "session";

export interface ExternalAddRequest {
  id: number;
  source: AddSource;
}

let nextExternalRequestId = 1;

/** The facets minus the saved-filter dimension. */
function withoutSmart(facets: Facets): Facets {
  const { smart: _smart, ...rest } = facets;
  return rest;
}

interface UiState {
  selection: Set<string>;
  anchor: string | null;
  facets: Facets;
  /** Saved multi-dimension queries (C4), shown as their own sidebar group. */
  smartFilters: SmartFilter[];
  search: string;
  /** Hashes whose contained filenames match `search`, from the server's index
   *  (V3-12). Transient: cleared when the search box empties. */
  fileMatches: Set<string>;
  sortColumn: SortColumn;
  sortDir: SortDir;
  columns: ColumnState;
  activeTab: DetailPane;
  dialog: DialogKind;
  externalAddRequest: ExternalAddRequest | null;
  externalAddComplete: (() => void) | null;
  /** Cursor position for the context menu, or null when closed. */
  contextMenu: { x: number; y: number } | null;
  /** Cursor position for the header column menu, or null when closed. */
  columnMenu: { x: number; y: number } | null;
  /** Narrow windows show the sidebar as an overlay panel (design §Responsive);
   *  at full width it is always visible and this is ignored. */
  sidebarOpen: boolean;
  /** Whether the remove confirmation should open with "delete the files"
   *  ticked — what the design's ⇧Delete carries in. Cleared on close. */
  removeWithData: boolean;

  // --- selection ---
  select: (hash: string) => void;
  toggle: (hash: string) => void;
  selectRange: (hash: string, ordered: string[]) => void;
  selectAll: (hashes: string[]) => void;
  clearSelection: () => void;
  /** Drop hashes that no longer exist (after a snapshot/removal). */
  pruneSelection: (existing: Set<string>) => void;

  // --- view ---
  /** Toggle one dimension: setting the value it already holds clears it. */
  setFacet: (kind: FacetKind, value: string | null) => void;
  /** Every dimension cleared, for "Clear filters". */
  clearFacets: () => void;
  /**
   * Save the current view (dimension filter + search text) as a named smart
   * filter, then activate it. Only meaningful for a non-smart view — see
   * `canSaveSmartFilter`.
   */
  saveSmartFilter: (name: string) => void;
  removeSmartFilter: (id: string) => void;
  setSearch: (s: string) => void;
  /** Replace the filename-search results (V3-12). */
  setFileMatches: (hashes: string[]) => void;
  setSort: (col: SortColumn) => void;
  resizeColumn: (id: ColumnId, width: number) => void;
  toggleColumn: (id: ColumnId) => void;
  /** Drag a header onto another to reorder the table. */
  moveColumn: (id: ColumnId, before: ColumnId) => void;
  resetColumns: () => void;
  setActiveTab: (t: DetailPane) => void;

  // --- dialogs / menu ---
  openDialog: (d: DialogKind) => void;
  openExternalAdd: (source: AddSource, onComplete: () => void) => void;
  closeDialog: () => void;
  openContextMenu: (x: number, y: number) => void;
  closeContextMenu: () => void;
  openColumnMenu: (x: number, y: number) => void;
  closeColumnMenu: () => void;
  toggleSidebar: () => void;
  requestRemoveData: () => void;
}

// --- localStorage persistence for view prefs ---
const LS_KEY = "rstorrent.view";
interface PersistedView {
  sortColumn: SortColumn;
  sortDir: SortDir;
  facets: Facets;
  activeTab: DetailPane;
  columns: string;
  smartFilters: SmartFilter[];
}

/** The shape written before the facet set: one dimension, or null. */
interface LegacyFilter {
  type: "status" | "errorKind" | "label" | "tracker" | "view" | "smart";
  value: string;
}

interface LoadedView extends Omit<PersistedView, "columns"> {
  columns: ColumnState;
}

function loadView(): LoadedView {
  const fallback: LoadedView = {
    // The design's default: newest first, ties broken by name (see selectors).
    sortColumn: "addedAt",
    sortDir: "desc",
    facets: {},
    activeTab: DEFAULT_PANE,
    columns: defaultColumnState(),
    smartFilters: [],
  };

  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<PersistedView> & {
        filter?: LegacyFilter | null;
      };
      const smartFilters = parsed.smartFilters ?? fallback.smartFilters;
      const facets = parsed.facets ?? migrateFilter(parsed.filter);
      // Don't restore a filter pointing at a smart filter that no longer
      // exists — that would silently show an unexplained empty table.
      if (facets.smart && !smartFilters.some((f) => f.id === facets.smart)) {
        delete facets.smart;
      }
      return {
        sortColumn: parsed.sortColumn ?? fallback.sortColumn,
        sortDir: parsed.sortDir ?? fallback.sortDir,
        facets,
        activeTab: parsePane(parsed.activeTab) ?? fallback.activeTab,
        columns: deserializeColumnState(parsed.columns),
        smartFilters,
      };
    }
  } catch {
    // ignore malformed storage
  }
  return fallback;
}

/** A pre-facet view's single filter, as a facet set. */
function migrateFilter(filter: LegacyFilter | null | undefined): Facets {
  if (!filter || typeof filter.value !== "string" || !filter.value) return {};
  return { [filter.type]: filter.value };
}

function saveView(v: PersistedView) {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(v));
  } catch {
    // storage may be unavailable; view prefs are non-critical
  }
}

export const useUi = create<UiState>((set, get) => {
  const initial = loadView();

  /** Persist the current view-pref subset after a change. */
  const persist = () => {
    const s = get();
    saveView({
      sortColumn: s.sortColumn,
      sortDir: s.sortDir,
      facets: s.facets,
      activeTab: s.activeTab,
      columns: serializeColumnState(s.columns),
      smartFilters: s.smartFilters,
    });
  };

  return {
    selection: new Set(),
    anchor: null,
    facets: initial.facets,
    smartFilters: initial.smartFilters,
    search: "",
    fileMatches: new Set(),
    sortColumn: initial.sortColumn,
    sortDir: initial.sortDir,
    columns: initial.columns,
    activeTab: initial.activeTab,
    dialog: null,
    externalAddRequest: null,
    externalAddComplete: null,
    contextMenu: null,
    columnMenu: null,
    sidebarOpen: false,
    removeWithData: false,

    select: (hash) => set({ selection: new Set([hash]), anchor: hash }),

    toggle: (hash) =>
      set((s) => {
        const next = new Set(s.selection);
        if (next.has(hash)) next.delete(hash);
        else next.add(hash);
        return { selection: next, anchor: hash };
      }),

    selectRange: (hash, ordered) =>
      set((s) => {
        const anchor = s.anchor ?? hash;
        const a = ordered.indexOf(anchor);
        const b = ordered.indexOf(hash);
        if (a === -1 || b === -1)
          return { selection: new Set([hash]), anchor: hash };
        const [lo, hi] = a < b ? [a, b] : [b, a];
        return { selection: new Set(ordered.slice(lo, hi + 1)) };
      }),

    selectAll: (hashes) => set({ selection: new Set(hashes) }),

    clearSelection: () => set({ selection: new Set(), anchor: null }),

    pruneSelection: (existing) =>
      set((s) => {
        let changed = false;
        const next = new Set<string>();
        for (const h of s.selection) {
          if (existing.has(h)) next.add(h);
          else changed = true;
        }
        return changed ? { selection: next } : {};
      }),

    setFacet: (kind, value) => {
      set((s) => {
        const facets = { ...s.facets };
        // Only null clears. An empty string is a real value: `label: ""` is the
        // "unlabeled" facet, which is how the sidebar offers that row.
        if (value === null || facets[kind] === value) delete facets[kind];
        else facets[kind] = value;
        return { facets };
      });
      persist();
    },

    clearFacets: () => {
      set({ facets: {} });
      persist();
    },

    saveSmartFilter: (name) => {
      const s = get();
      const trimmed = name.trim();
      if (!trimmed || !canSaveSmartFilter(s.facets, s.search)) return;

      const { status, label, tags, tracker, errorKind, view } = s.facets;
      const saved: SmartFilter = {
        id: newSmartFilterId(),
        name: trimmed,
        ...(status ? { status } : {}),
        ...(label ? { label } : {}),
        ...(tags ? { tags } : {}),
        ...(tracker ? { tracker } : {}),
        ...(errorKind ? { errorKind } : {}),
        ...(view ? { view } : {}),
        ...(s.search.trim() ? { text: s.search.trim() } : {}),
      };
      set({
        smartFilters: [...s.smartFilters, saved],
        // The saved filter becomes the active one, and its criteria move out of
        // the live facets so they are not applied twice.
        facets: { smart: saved.id },
        // The text now lives in the filter, so clear the box: the visible rows
        // are unchanged, but the criterion has exactly one home.
        search: "",
      });
      persist();
    },

    removeSmartFilter: (id) => {
      const s = get();
      const wasActive = s.facets.smart === id;
      set({
        smartFilters: s.smartFilters.filter((f) => f.id !== id),
        facets: wasActive ? withoutSmart(s.facets) : s.facets,
      });
      persist();
    },

    setSearch: (search) =>
      // An emptied box has no filename matches left.
      set(search.trim() ? { search } : { search, fileMatches: new Set() }),
    setFileMatches: (hashes) => set({ fileMatches: new Set(hashes) }),
    setSort: (col) => {
      set((s) => {
        // Same column → flip direction; new column → default ascending.
        if (s.sortColumn === col) {
          return { sortDir: s.sortDir === "asc" ? "desc" : "asc" };
        }
        return { sortColumn: col, sortDir: "asc" };
      });
      persist();
    },
    resizeColumn: (id, width) => {
      set((s) => ({ columns: resizeColumnState(s.columns, id, width) }));
      persist();
    },
    toggleColumn: (id) => {
      set((s) => ({ columns: toggleColumnState(s.columns, id) }));
      persist();
    },
    moveColumn: (id, before) => {
      set((s) => ({ columns: moveColumnState(s.columns, id, before) }));
      persist();
    },
    resetColumns: () => {
      set({ columns: defaultColumnState() });
      persist();
    },
    setActiveTab: (t) => {
      set({ activeTab: t });
      persist();
    },

    openDialog: (dialog) => {
      // A queued Finder/deep-link dialog owns the modal until it is completed
      // or cancelled; menu actions must not strand its queue promise.
      if (get().externalAddRequest) return;
      set({ dialog, contextMenu: null, columnMenu: null });
    },
    openExternalAdd: (source, onComplete) =>
      set({
        // file + upload both open the torrent dialog; magnet opens the other.
        dialog: source.kind === "magnet" ? "add-magnet" : "add-file",
        externalAddRequest: { id: nextExternalRequestId++, source },
        externalAddComplete: onComplete,
        contextMenu: null,
        columnMenu: null,
      }),
    closeDialog: () => {
      const complete = get().externalAddComplete;
      set({
        dialog: null,
        externalAddRequest: null,
        externalAddComplete: null,
        removeWithData: false,
      });
      complete?.();
    },
    openContextMenu: (x, y) => set({ contextMenu: { x, y }, columnMenu: null }),
    closeContextMenu: () => set({ contextMenu: null }),
    openColumnMenu: (x, y) => set({ columnMenu: { x, y }, contextMenu: null }),
    closeColumnMenu: () => set({ columnMenu: null }),
    toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
    requestRemoveData: () => set({ removeWithData: true }),
  };
});
