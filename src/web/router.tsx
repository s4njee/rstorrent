/**
 * A tiny pushState router (WC7-S1).
 *
 * Three routes — `/` (console), `/settings`, `/stats` — are all the web console
 * needs, so this is a `useSyncExternalStore` over `history` rather than a
 * dependency. The server's SPA fallback serves `web.html` for every path, so a
 * deep link or a reload lands on the right route without a round-trip.
 */

import { useSyncExternalStore } from "react";

/** The known routes. Anything else falls back to the console. */
export type Route = "/" | "/settings" | "/stats";

/** Our own navigation event, so a push notifies every subscriber at once. */
const NAVIGATION = "rstorrent:navigate";

/** Map any pathname onto a route, defaulting to the console. */
export function normalize(pathname: string): Route {
  if (pathname.startsWith("/settings")) return "/settings";
  if (pathname.startsWith("/stats")) return "/stats";
  return "/";
}

function subscribe(listener: () => void): () => void {
  window.addEventListener("popstate", listener);
  window.addEventListener(NAVIGATION, listener);
  return () => {
    window.removeEventListener("popstate", listener);
    window.removeEventListener(NAVIGATION, listener);
  };
}

function snapshot(): Route {
  return normalize(window.location.pathname);
}

/** The current route; re-renders on navigation and on back/forward. */
export function useRoute(): Route {
  return useSyncExternalStore(subscribe, snapshot, () => "/");
}

/** Push a route and notify every subscriber. */
export function navigate(to: Route): void {
  if (window.location.pathname === to) return;
  window.history.pushState({}, "", to);
  window.dispatchEvent(new Event(NAVIGATION));
}
