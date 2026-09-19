/**
 * The single seam between the UI and the server.
 *
 * Every frontend → server call goes through `invoke` and every server →
 * frontend push through `listen`. In production that is always the web backend
 * (`ipc/web.ts`: `fetch`/polling against `rstorrent-web`); tests and the
 * browser demo swap in an in-memory double with {@link setBackend}.
 *
 * `commands.ts` and `events.ts` keep their typed wrappers and delegate here, so
 * no component or store talks to `fetch` for the IPC surface directly.
 */

import { webBackend } from "./web";

/** Teardown handle returned by {@link Backend.listen}. */
export type UnlistenFn = () => void;

/** The command + event channels a backend must provide. */
export interface Backend {
  /** Invoke a command by name; resolves with its result or rejects. */
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /** Subscribe to an event; the handler receives the payload directly. */
  listen<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn>;
}

let active: Backend = webBackend;

/** Replace the backend — for tests and the browser demo only. */
export function setBackend(backend: Backend): void {
  active = backend;
}

/** The active backend (the web backend unless a test/demo replaced it). */
export function backend(): Backend {
  return active;
}
