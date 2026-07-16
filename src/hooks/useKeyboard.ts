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
import * as actions from "../actions";

/** True when focus is in a text-entry element. */
function typingInField(): boolean {
  const el = document.activeElement;
  if (!el) return false;
  const tag = el.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    (el as HTMLElement).isContentEditable
  );
}

/** Current visible-row hashes (respecting filter/search/sort). */
function visibleHashes(): string[] {
  const t = useTorrents.getState();
  const u = useUi.getState();
  return selectVisible(
    t.torrents,
    u.filter,
    u.search,
    u.sortColumn,
    u.sortDir,
  ).map((row) => row.hash);
}

export function useKeyboardShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const ui = useUi.getState();
      const mod = e.metaKey || e.ctrlKey;

      // ⌘F — focus the filter box.
      if (mod && e.key.toLowerCase() === "f") {
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
      // ⌘O / ⌘⇧O — add file / magnet.
      if (mod && e.key.toLowerCase() === "o") {
        e.preventDefault();
        ui.openDialog(e.shiftKey ? "add-magnet" : "add-file");
        return;
      }
      // ⌘, — Preferences (macOS convention).
      if (mod && e.key === ",") {
        e.preventDefault();
        ui.openDialog("prefs");
        return;
      }

      // The rest are single-key and must not fire while typing.
      if (typingInField()) return;

      // Escape — unwind context menu → dialog → selection.
      if (e.key === "Escape") {
        if (ui.contextMenu) ui.closeContextMenu();
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
      // Delete / Backspace — remove the selection (opens confirm in E7).
      if (
        (e.key === "Backspace" || e.key === "Delete") &&
        ui.selection.size > 0
      ) {
        e.preventDefault();
        actions.requestRemove();
      }
    };

    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
