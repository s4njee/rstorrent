/**
 * WE4-S3 — drag & drop onto the *browser* window.
 *
 * Desktop uses Tauri's native drag-drop (real filesystem paths). The web shell
 * has only the DOM File API, so this hook:
 *  - accepts `.torrent` file drops → upload sources
 *  - accepts dropped magnet/URL text → magnet sources
 *  - ignores unrelated payloads
 *
 * Returns whether a droppable payload is currently hovering, for an overlay.
 */

import { useEffect, useState } from "react";
import { enqueueAddSources } from "../addQueue";
import { parseDroppedFiles, parsePastedText } from "../externalOpen";

export function useWebDragDrop(): boolean {
  const [over, setOver] = useState(false);

  useEffect(() => {
    const onDragOver = (e: DragEvent) => {
      const dt = e.dataTransfer;
      if (!dt) return;
      const files = dt.types.includes("Files");
      const text =
        dt.types.includes("text/plain") || dt.types.includes("text/uri-list");
      if (!files && !text) return;
      // Required, or the browser refuses the drop.
      e.preventDefault();
      dt.dropEffect = "copy";
      // File names aren't available until drop; light the overlay for any
      // file drag and filter to `.torrent` in onDrop.
      setOver(true);
    };

    const onDragLeave = (e: DragEvent) => {
      // Only clear when leaving the window (relatedTarget null-ish).
      if (e.relatedTarget == null) setOver(false);
    };

    const onDrop = (e: DragEvent) => {
      setOver(false);
      const dt = e.dataTransfer;
      if (!dt) return;

      const fileSources = parseDroppedFiles(dt.files);
      if (fileSources.length) {
        e.preventDefault();
        enqueueAddSources(fileSources);
        return;
      }

      const text =
        dt.getData("text/plain") || dt.getData("text/uri-list") || "";
      const textSources = parsePastedText(text);
      if (textSources.length) {
        e.preventDefault();
        enqueueAddSources(textSources);
      }
    };

    window.addEventListener("dragover", onDragOver);
    window.addEventListener("dragleave", onDragLeave);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("dragover", onDragOver);
      window.removeEventListener("dragleave", onDragLeave);
      window.removeEventListener("drop", onDrop);
    };
  }, []);

  return over;
}
