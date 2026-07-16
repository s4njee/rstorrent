/**
 * Log store — mirrors the Rust app event log for the Log detail tab.
 *
 * Hydrated once via `get_log` and kept current by the `log://append` event.
 * Capped on the frontend too so a long session doesn't grow the array
 * unbounded (the Rust buffer is already capped; this guards re-render cost).
 */

import { create } from "zustand";
import type { LogEntry } from "../ipc/types";

const CAP = 1000;

interface LogState {
  entries: LogEntry[];
  hydrate: (entries: LogEntry[]) => void;
  append: (entry: LogEntry) => void;
}

export const useLog = create<LogState>((set) => ({
  entries: [],
  hydrate: (entries) => set({ entries: entries.slice(-CAP) }),
  append: (entry) =>
    set((s) => ({ entries: [...s.entries, entry].slice(-CAP) })),
}));
