/**
 * C1 — drag & drop onto the window.
 *
 * Two channels, because they carry different things:
 *
 *  - **Tauri's native drag-drop** (`onDragDropEvent`) delivers real filesystem
 *    paths for dropped files, which is what the add pipeline needs. An HTML5
 *    drop would only give us a `File` with no path.
 *  - **DOM drop events** carry dragged *text* (a magnet link dragged out of a
 *    browser). Tauri's native handler owns file drags, so this is best-effort:
 *    if the platform routes text drags through the native handler too, this
 *    never fires and paste (C2) remains the reliable way to add a magnet.
 *
 * Returns whether a droppable payload is currently hovering, for the overlay.
 */

import { useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { enqueueAddSources } from "../addQueue";
import { parseDroppedPaths, parsePastedText } from "../externalOpen";

export function useDragDrop(): boolean {
  const [over, setOver] = useState(false);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;

    void (async () => {
      const un = await getCurrentWebview().onDragDropEvent((event) => {
        const payload = event.payload;
        if (payload.type === "enter") {
          // `enter` carries the paths, so the overlay only lights up when the
          // drag actually holds a .torrent — not for any stray file.
          setOver(parseDroppedPaths(payload.paths).length > 0);
        } else if (payload.type === "drop") {
          setOver(false);
          enqueueAddSources(parseDroppedPaths(payload.paths));
        } else if (payload.type === "leave") {
          setOver(false);
        }
      });
      if (cancelled) un();
      else unlisten = un;
    })();

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  // Best-effort text drops (magnet dragged from a browser).
  useEffect(() => {
    // A file drag belongs exclusively to Tauri's native handler, which gives us
    // real paths. macOS can also expose a dragged file's path as text/plain, so
    // without this guard a single .torrent drop could be added twice — once
    // from the native event and once from here.
    const isFileDrag = (e: DragEvent) =>
      e.dataTransfer?.types.includes("Files") ?? false;

    const onDragOver = (e: DragEvent) => {
      if (isFileDrag(e)) return;
      if (e.dataTransfer?.types.includes("text/plain")) {
        // Required, or the webview refuses the drop.
        e.preventDefault();
      }
    };
    const onDrop = (e: DragEvent) => {
      if (isFileDrag(e)) return;
      const text =
        e.dataTransfer?.getData("text/plain") ||
        e.dataTransfer?.getData("text/uri-list") ||
        "";
      const sources = parsePastedText(text);
      if (!sources.length) return;
      e.preventDefault();
      setOver(false);
      enqueueAddSources(sources);
    };

    window.addEventListener("dragover", onDragOver);
    window.addEventListener("drop", onDrop);
    return () => {
      window.removeEventListener("dragover", onDragOver);
      window.removeEventListener("drop", onDrop);
    };
  }, []);

  return over;
}
