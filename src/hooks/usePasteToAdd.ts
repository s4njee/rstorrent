/**
 * C2 — paste a magnet/torrent URL onto the main window to add it.
 *
 * This listens for the DOM `paste` event rather than a ⌘V keydown, so it also
 * catches pastes from the browser's Edit menu and context menu.
 *
 * Pastes into a text field (the filter box, a dialog input) are left alone, and
 * clipboard text that isn't a magnet/torrent URL falls through to default
 * handling — pasting prose must not pop an error.
 */

import { useEffect } from "react";
import { enqueueAddSources } from "../addQueue";
import { parsePastedText } from "../externalOpen";
import { typingInField } from "../utils/dom";

export function usePasteToAdd(): void {
  useEffect(() => {
    const onPaste = (e: ClipboardEvent) => {
      if (typingInField()) return;

      const text = e.clipboardData?.getData("text") ?? "";
      const sources = parsePastedText(text);
      if (!sources.length) return;

      e.preventDefault();
      enqueueAddSources(sources);
    };

    window.addEventListener("paste", onPaste);
    return () => window.removeEventListener("paste", onPaste);
  }, []);
}
