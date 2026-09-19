/**
 * Moves store — live move-on-complete statuses (V3-14).
 *
 * Hydrated via `get_moves` and refreshed on the `moves://update` nudge
 * (the web backend polls `GET /api/moves` for it). While a move is
 * active the pill polls `getMoves` itself for smooth byte progress.
 */

import { create } from "zustand";
import type { MoveStatus } from "../ipc/types";
import { getMoves } from "../ipc/commands";

interface MovesState {
  moves: MoveStatus[];
  set: (moves: MoveStatus[]) => void;
  refresh: () => void;
}

export const useMoves = create<MovesState>((set) => ({
  // Coerce: a stubbed or older backend can answer with a non-list.
  moves: [],
  set: (moves) => set({ moves: Array.isArray(moves) ? moves : [] }),
  refresh: () => {
    void getMoves().then(
      (moves) => set({ moves }),
      () => {},
    );
  },
}));

/** Moves with bytes still to copy (or not yet started). */
export function activeMoves(moves: readonly MoveStatus[]): MoveStatus[] {
  return moves.filter(
    (m) => m.state === "pending" || m.state === "in_progress",
  );
}

/** Terminally failed or cancelled moves awaiting retry. */
export function retryableMoves(moves: readonly MoveStatus[]): MoveStatus[] {
  return moves.filter((m) => m.state === "failed" || m.state === "cancelled");
}

/** 0..100, or null when the total is unknown. */
export function movePercent(m: MoveStatus): number | null {
  if (m.totalBytes <= 0) return null;
  return Math.max(
    0,
    Math.min(100, Math.floor((m.doneBytes / m.totalBytes) * 100)),
  );
}

/** Last path segment, for the compact pill. */
export function baseName(path: string): string {
  const clean = path.replace(/\/+$/, "");
  const idx = clean.lastIndexOf("/");
  return idx >= 0 ? clean.slice(idx + 1) : clean;
}
