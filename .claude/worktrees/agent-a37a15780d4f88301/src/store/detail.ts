/**
 * Detail store — the most recent `state://detail` payload for the selected
 * torrent's active tab. Kept tiny and separate so a 2s detail update doesn't
 * re-render the main table.
 */

import { create } from "zustand";
import type { DetailPayload } from "../ipc/types";

interface DetailState {
  data: DetailPayload | null;
  setDetail: (d: DetailPayload) => void;
  clear: () => void;
}

export const useDetail = create<DetailState>((set) => ({
  data: null,
  setDetail: (data) => set({ data }),
  clear: () => set({ data: null }),
}));
