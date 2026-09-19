/**
 * Filename search (V3-12).
 *
 * The browser matches name/hash/label/tags/tracker/save-path locally; the
 * torrent's *contained filenames* live behind the server's lazily built index,
 * so this asks it (debounced) whenever the search box has text and stores the
 * resulting hashes. A failed lookup clears them rather than blocking the local
 * matches — the cheap fields still work offline.
 */

import { useEffect } from "react";
import { useUi } from "../store/ui";
import { searchFiles } from "../ipc/webSettings";

export function useFileSearch(): void {
  const search = useUi((s) => s.search);
  const setFileMatches = useUi((s) => s.setFileMatches);

  useEffect(() => {
    const query = search.trim();
    if (!query) {
      setFileMatches([]);
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      void searchFiles(query)
        .then((results) => {
          if (!cancelled) setFileMatches(results.hashes ?? []);
        })
        .catch(() => {
          if (!cancelled) setFileMatches([]);
        });
    }, 150);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [search, setFileMatches]);
}
