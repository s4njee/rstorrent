/**
 * Global transfer history collected from the live snapshot stream.
 *
 * This is intentionally frontend-owned: the daemon exposes current global
 * rates, but not a useful time series. Keeping a bounded ring buffer gives the
 * Statistics dialog a live graph without adding an RPC or a persistence
 * format. The buffer is scoped to the current app session.
 */

import { create } from "zustand";
import type { GlobalStats } from "../ipc/types";

/** Thirty minutes at the normal one-second snapshot cadence. */
export const MAX_TRANSFER_POINTS = 1_800;

export interface TransferPoint {
  time: number;
  down: number;
  up: number;
}

interface TransferHistoryState {
  points: TransferPoint[];
  record: (globals: GlobalStats, time?: number) => void;
  clear: () => void;
}

function safeRate(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}

export const useTransferHistory = create<TransferHistoryState>((set) => ({
  points: [],
  record: (globals, time = Date.now()) =>
    set((state) => ({
      points: [
        ...state.points,
        {
          time,
          down: safeRate(globals.downRate),
          up: safeRate(globals.upRate),
        },
      ].slice(-MAX_TRANSFER_POINTS),
    })),
  clear: () => set({ points: [] }),
}));
