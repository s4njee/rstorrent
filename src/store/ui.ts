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
import type { DetailTab } from "../ipc/types";
import {
  defaultColumnState,
  deserializeColumnState,
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
  | "ratio";

export type SortDir = "asc" | "desc";

export type ActiveFilter =
  | { type: "status"; value: string }
  | { type: "label"; value: string }
  | { type: "tracker"; value: string }
  | null;

export type DialogKind =
  null | "add-file" | "add-magnet" | "prefs" | "stats" | "remove";

export interface ExternalAddRequest {
  id: number;
  source: AddSource;
}

let nextExternalRequestId = 1;

interface UiState {
  selection: Set<string>;
  anchor: string | null;
  filter: ActiveFilter;
  search: string;
  sortColumn: SortColumn;
  sortDir: SortDir;
  columns: ColumnState;
  activeTab: DetailTab;
  dialog: DialogKind;
  externalAddRequest: ExternalAddRequest | null;
  externalAddComplete: (() => void) | null;
  /** Cursor position for the context menu, or null when closed. */
  contextMenu: { x: number; y: number } | null;
  /** Cursor position for the header column menu, or null when closed. */
  columnMenu: { x: number; y: number } | null;

  // --- selection ---
  select: (hash: string) => void;
  toggle: (hash: string) => void;
  selectRange: (hash: string, ordered: string[]) => void;
  selectAll: (hashes: string[]) => void;
  clearSelection: () => void;
  /** Drop hashes that no longer exist (after a snapshot/removal). */
  pruneSelection: (existing: Set<string>) => void;

  // --- view ---
  setFilter: (f: ActiveFilter) => void;
  setSearch: (s: string) => void;
  setSort: (col: SortColumn) => void;
  resizeColumn: (id: ColumnId, width: number) => void;
  toggleColumn: (id: ColumnId) => void;
  resetColumns: () => void;
  setActiveTab: (t: DetailTab) => void;

  // --- dialogs / menu ---
  openDialog: (d: DialogKind) => void;
  openExternalAdd: (source: AddSource, onComplete: () => void) => void;
  closeDialog: () => void;
  openContextMenu: (x: number, y: number) => void;
  closeContextMenu: () => void;
  openColumnMenu: (x: number, y: number) => void;
  closeColumnMenu: () => void;
}

// --- localStorage persistence for view prefs ---
const LS_KEY = "rstorrent.view";
interface PersistedView {
  sortColumn: SortColumn;
  sortDir: SortDir;
  filter: ActiveFilter;
  activeTab: DetailTab;
  columns: string;
}

interface LoadedView extends Omit<PersistedView, "columns"> {
  columns: ColumnState;
}

function loadView(): LoadedView {
  const fallback: LoadedView = {
    sortColumn: "name",
    sortDir: "asc",
    filter: null,
    activeTab: "general",
    columns: defaultColumnState(),
  };

  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) {
      const parsed = JSON.parse(raw) as Partial<PersistedView>;
      return {
        sortColumn: parsed.sortColumn ?? fallback.sortColumn,
        sortDir: parsed.sortDir ?? fallback.sortDir,
        filter: parsed.filter ?? fallback.filter,
        activeTab: parsed.activeTab ?? fallback.activeTab,
        columns: deserializeColumnState(parsed.columns),
      };
    }
  } catch {
    // ignore malformed storage
  }
  return fallback;
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
      filter: s.filter,
      activeTab: s.activeTab,
      columns: serializeColumnState(s.columns),
    });
  };

  return {
    selection: new Set(),
    anchor: null,
    filter: initial.filter,
    search: "",
    sortColumn: initial.sortColumn,
    sortDir: initial.sortDir,
    columns: initial.columns,
    activeTab: initial.activeTab,
    dialog: null,
    externalAddRequest: null,
    externalAddComplete: null,
    contextMenu: null,
    columnMenu: null,

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

    setFilter: (f) => {
      set({ filter: f });
      persist();
    },
    setSearch: (search) => set({ search }),
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
        dialog: source.kind === "file" ? "add-file" : "add-magnet",
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
      });
      complete?.();
    },
    openContextMenu: (x, y) => set({ contextMenu: { x, y }, columnMenu: null }),
    closeContextMenu: () => set({ contextMenu: null }),
    openColumnMenu: (x, y) => set({ columnMenu: { x, y }, contextMenu: null }),
    closeColumnMenu: () => set({ columnMenu: null }),
  };
});
