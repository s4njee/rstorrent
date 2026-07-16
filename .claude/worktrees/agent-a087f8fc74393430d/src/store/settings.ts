/**
 * Settings store — the frontend mirror of the Rust-persisted app settings.
 *
 * Loaded once on startup and refreshed after Preferences applies changes.
 * Currently consumed by the Remove dialog (localhost gating) and, later, by the
 * Preferences UI (E11). Kept minimal here; E11 expands its usage.
 */

import { create } from "zustand";
import type { Settings, Transport } from "../ipc/types";
import { getSettings } from "../ipc/commands";

interface SettingsState {
  settings: Settings | null;
  load: () => Promise<void>;
  set: (s: Settings) => void;
}

export const useSettings = create<SettingsState>((set) => ({
  settings: null,
  load: async () => set({ settings: await getSettings() }),
  set: (settings) => set({ settings }),
}));

/** True when the transport points at the local machine (mirrors settings.rs). */
export function isLocalhost(transport: Transport | undefined): boolean {
  if (!transport) return true;
  if (transport.kind === "unixSocket") return true;
  return ["127.0.0.1", "::1", "localhost"].includes(transport.host);
}
