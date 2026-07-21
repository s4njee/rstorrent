/**
 * The desktop host backend: real Tauri IPC.
 *
 * `invoke` forwards to Tauri's command bridge; `listen` forwards to Tauri's
 * event bus and unwraps the payload. This is also the backend the browser demo
 * registers — there the underlying `@tauri-apps` calls are intercepted by
 * `mockIPC`, so the same adapter drives fixtures with no daemon.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Backend, Capabilities, UnlistenFn } from "./backend";

/** The desktop shell can do everything; runtime remote-gating happens elsewhere. */
const capabilities: Capabilities = {
  localFs: true,
  nativeDialogs: true,
  keychain: true,
  menus: true,
  deepLinks: true,
  clipboardRead: true,
};

export const tauriBackend: Backend = {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    return invoke<T>(command, args);
  },
  listen<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
    return listen<T>(event, (e) => handler(e.payload));
  },
  capabilities,
};
