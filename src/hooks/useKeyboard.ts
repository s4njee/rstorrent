/**
 * Global keyboard shortcuts for the main window.
 *
 * Mirrors the toolbar/menu actions so the app is drivable without a mouse.
 * Shortcuts that would type into a field are suppressed while an input/textarea
 * is focused (except ⌘-combos, which stay active). Dialog-opening shortcuts
 * (⌘O / ⌘⇧O) set the dialog state; their surfaces render in later epics.
 */

import { useEffect } from "react";
import { useUi } from "../store/ui";
import { useTorrents } from "../store/torrents";
import { selectVisible } from "../store/selectors";
import { typingInField } from "../utils/dom";
import * as actions from "../actions";

/** Current visible-row hashes (respecting filter/search/sort). */
function visibleHashes(): string[] {
  const t = useTorrents.getState();
  const u = useUi.getState();
  return selectVisible(
    t.torrents,
    u.facets,
    u.search,
    u.sortColumn,
    u.sortDir,
    u.smartFilters,
    u.fileMatches,
  ).map((row) => row.hash);
}

export function useKeyboardShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const ui = useUi.getState();
      const mod = e.metaKey || e.ctrlKey;

      // ⌘F — focus the filter box. `/` does the same without a modifier, the
      // design's shortcut, but only when not already typing.
      if (mod && e.key.toLowerCase() === "f") {
        e.preventDefault();
        document.getElementById("filter-input")?.focus();
        return;
      }
      if (e.key === "/" && !mod && !e.altKey && !typingInField()) {
        e.preventDefault();
        document.getElementById("filter-input")?.focus();
        return;
      }
      // ⌘A — select all visible rows (not while typing).
      if (mod && e.key.toLowerCase() === "a" && !typingInField()) {
        e.preventDefault();
        ui.selectAll(visibleHashes());
        return;
      }
      // ⌘N — create torrent.
      if (mod && e.key.toLowerCase() === "n" && !typingInField()) {
        e.preventDefault();
        ui.openDialog("create-torrent");
        return;
      }
      // ⌥⌘R — force a hash recheck on the current selection.
      if (mod && e.altKey && e.key.toLowerCase() === "r" && ui.selection.size) {
        e.preventDefault();
        actions.recheck();
        return;
      }
      // Note: ⌘O / ⌘⇧O (add file/magnet) and ⌘, (settings) are not bound here;
      // the top bar's add and settings buttons are their entry points.

      // The rest are single-key and must not fire while typing.
      if (typingInField()) return;

      // Arrow navigation — moves selection without breaking sticky header or
      // variable columns (FND-01). Virtualized table listens for the scroll
      // event and keeps the new row in view.
      if (
        (e.key === "ArrowDown" || e.key === "ArrowUp") &&
        !e.metaKey &&
        !e.ctrlKey &&
        !e.altKey
      ) {
        const hashes = visibleHashes();
        if (hashes.length === 0) return;
        e.preventDefault();
        const anchor =
          ui.anchor ??
          (ui.selection.size ? [...ui.selection][0] : null) ??
          hashes[0];
        const idx = hashes.indexOf(anchor);
        const baseIdx = idx === -1 ? 0 : idx;
        let nextIdx: number;
        if (e.key === "ArrowDown")
          nextIdx = Math.min(hashes.length - 1, baseIdx + 1);
        else nextIdx = Math.max(0, baseIdx - 1);
        const nextHash = hashes[nextIdx]!;
        if (e.shiftKey) {
          ui.selectRange(nextHash, hashes);
        } else {
          ui.select(nextHash);
        }
        window.dispatchEvent(
          new CustomEvent("scrollToTorrent", { detail: nextHash }),
        );
        return;
      }
      if (e.key === "Home" || e.key === "End") {
        const hashes = visibleHashes();
        if (hashes.length === 0) return;
        e.preventDefault();
        const nextHash =
          e.key === "Home" ? hashes[0]! : hashes[hashes.length - 1]!;
        if (e.shiftKey) ui.selectRange(nextHash, hashes);
        else ui.select(nextHash);
        window.dispatchEvent(
          new CustomEvent("scrollToTorrent", { detail: nextHash }),
        );
        return;
      }

      // Escape — unwind context menu → dialog → selection.
      if (e.key === "Escape") {
        if (ui.sidebarOpen) ui.toggleSidebar();
        else if (ui.columnMenu) ui.closeColumnMenu();
        else if (ui.contextMenu) ui.closeContextMenu();
        else if (ui.dialog) ui.closeDialog();
        else ui.clearSelection();
        return;
      }
      // Space — toggle pause/resume on the selection.
      if (e.key === " " && ui.selection.size > 0) {
        e.preventDefault();
        const torrents = useTorrents.getState().torrents;
        const selected = torrents.filter((t) => ui.selection.has(t.hash));
        const anyActive = selected.some(
          (t) => t.status !== "paused" && t.status !== "error",
        );
        if (anyActive) actions.pause();
        else actions.resume();
        return;
      }
      // Delete removes; ⇧Delete offers to take the data with it. Both open a
      // confirmation — the data variant names the path and the count.
      if (
        (e.key === "Backspace" || e.key === "Delete") &&
        ui.selection.size > 0
      ) {
        e.preventDefault();
        actions.requestRemove({ deleteData: e.shiftKey });
        return;
      }
    };

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
