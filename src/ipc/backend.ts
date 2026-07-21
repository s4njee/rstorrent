/**
 * The single seam between the UI and its host.
 *
 * Every frontend → host call goes through `invoke` and every host → frontend
 * push through `listen`, mirroring Tauri's own API. Swapping the registered
 * backend is what lets the identical component tree, stores, and dialogs run
 * over three hosts:
 *   - **tauri** (`ipc/tauri.ts`)   — the desktop app, real Tauri IPC.
 *   - **web** (`ipc/web.ts`)       — the browser, `fetch`/polling against the server.
 *   - **demo** (Tauri `mockIPC`)   — fixtures, for the browser demo and tests.
 *
 * `commands.ts` and `events.ts` keep their typed wrappers and delegate here, so
 * no component or store imports a host API directly.
 */

/** Teardown handle returned by {@link Backend.listen} (matches Tauri's shape). */
export type UnlistenFn = () => void;

/**
 * What the host shell can do. Shared components branch on capability instead of
 * sniffing the platform, so the same component behaves correctly on desktop and
 * in the browser.
 */
export interface Capabilities {
  /** Reveal/open files on the daemon host, and delete-data to the trash. */
  localFs: boolean;
  /** Native file/folder pickers (vs a browser `<input type="file">`). */
  nativeDialogs: boolean;
  /** OS keychain for remote-daemon passwords. */
  keychain: boolean;
  /** Native menubar events. */
  menus: boolean;
  /** File/magnet deep-link open requests. */
  deepLinks: boolean;
  /** Reading the clipboard (magnet prefill). */
  clipboardRead: boolean;
}

/** The command + event channels a host must provide. */
export interface Backend {
  /** Invoke a host command by name; resolves with its result or rejects. */
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  /** Subscribe to a host event; the handler receives the payload directly. */
  listen<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn>;
  /** What this host can do (see {@link Capabilities}). */
  readonly capabilities: Capabilities;
}

let active: Backend | null = null;

/** Register the host backend. Call once, before rendering `<App />`. */
export function setBackend(backend: Backend): void {
  active = backend;
}

/** The registered backend. Throws if none was registered (a wiring bug). */
export function backend(): Backend {
  if (!active) {
    throw new Error(
      "no IPC backend registered — call setBackend() before rendering App",
    );
  }
  return active;
}

/** Convenience accessor for the active host's capabilities. */
export function capabilities(): Capabilities {
  return backend().capabilities;
}
