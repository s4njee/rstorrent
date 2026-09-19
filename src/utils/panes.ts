/**
 * The detail panel's panes.
 *
 * A pane is what the console shows; a `DetailTab` is what the daemon can be
 * asked for. They are not the same set — the design has five panes plus the log,
 * and two of them read the daemon's `general` payload — so the mapping lives here
 * rather than being implied by a union of both vocabularies.
 *
 * This module is deliberately free of React: the UI store persists the active
 * pane and the panel renders it, and neither should own the other's vocabulary.
 */

import type { DetailTab } from "../ipc/types";

export type DetailPane =
  "files" | "peers" | "trackers" | "transfer" | "pieces" | "log";

export interface PaneDef {
  id: DetailPane;
  /** The tab's label, per the design. */
  label: string;
  /** The daemon tab whose payload this pane renders. */
  daemon: DetailTab;
  /** Whether the facts rail shares the body with this pane (design frames 1a/1d). */
  rail: boolean;
}

/**
 * The panes in tab-strip order: the design's five, with the log last (it is ours,
 * not the design's — WC5-S7 keeps it, in the design's language).
 */
export const PANES: readonly PaneDef[] = [
  { id: "files", label: "Files", daemon: "content", rail: true },
  { id: "peers", label: "Peers", daemon: "peers", rail: false },
  { id: "trackers", label: "Trackers", daemon: "trackers", rail: false },
  { id: "transfer", label: "Transfer", daemon: "general", rail: true },
  { id: "pieces", label: "Pieces", daemon: "general", rail: false },
  { id: "log", label: "Log", daemon: "log", rail: false },
];

export const PANE_IDS: readonly DetailPane[] = PANES.map((pane) => pane.id);

/** The pane the panel opens on — the design's active tab. */
export const DEFAULT_PANE: DetailPane = "files";

export function paneDef(pane: DetailPane): PaneDef {
  return PANES.find((entry) => entry.id === pane) ?? PANES[0];
}

/** The daemon tab to poll for a pane. */
export function daemonTabFor(pane: DetailPane): DetailTab {
  return paneDef(pane).daemon;
}

export function paneHasRail(pane: DetailPane): boolean {
  return paneDef(pane).rail;
}

/**
 * The rtorrent tab names this app used before the console's vocabulary, so a
 * persisted view from an older build opens on the equivalent pane instead of
 * falling back to the default. `speed` was the per-torrent chart, which the
 * design moves into Transfer.
 */
const LEGACY: Record<string, DetailPane> = {
  general: "transfer",
  content: "files",
  speed: "transfer",
};

/** Parses a persisted pane, accepting the pre-console names. */
export function parsePane(value: unknown): DetailPane | null {
  if (typeof value !== "string") return null;
  if ((PANE_IDS as readonly string[]).includes(value))
    return value as DetailPane;
  return LEGACY[value] ?? null;
}
