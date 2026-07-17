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
  if (transport.kind === "http") return httpHostIsLocal(transport.url);
  return LOCAL_HOSTS.includes(transport.host);
}

const LOCAL_HOSTS = ["127.0.0.1", "::1", "localhost", "0.0.0.0"];

/**
 * Host of an http(s) URL, or "" if it can't be read.
 *
 * Mirrors `host_is_local` in the Rust `rtorrent::http` module. The duplication
 * is deliberate: Rust gates the actual privileged operations, while this drives
 * instant UI feedback without an IPC round-trip per keystroke. Keep them in
 * step — both must treat userinfo as *not* the host.
 */
function urlHost(url: string): string {
  try {
    return new URL(url.trim()).hostname.replace(/^\[|\]$/g, "").toLowerCase();
  } catch {
    return "";
  }
}

export function httpHostIsLocal(url: string): boolean {
  return LOCAL_HOSTS.includes(urlHost(url));
}

/**
 * Would this endpoint put credentials on the wire in the clear? Basic auth is
 * base64, not encryption, so plain HTTP to a remote host exposes the password.
 * Localhost is exempt — it never leaves the machine.
 */
export function isInsecureCredentialed(
  transport: Transport | undefined,
): boolean {
  if (transport?.kind !== "http") return false;
  if (!transport.username) return false;
  if (!/^http:\/\//i.test(transport.url.trim())) return false;
  return !httpHostIsLocal(transport.url);
}
