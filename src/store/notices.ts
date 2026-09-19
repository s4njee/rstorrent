/**
 * Notices: the transient messages the operator should see and be able to
 * dismiss — a command that failed, the connection dropping and coming back.
 *
 * Deliberately separate from the *log* (the append-only event ring buffer the
 * Log tab reads, which is the record): a notice is something worth interrupting
 * for, and it disappears. Anything raised here is raised exactly once, so no
 * caller needs to dedupe.
 */

import { create } from "zustand";

export type NoticeLevel = "info" | "warn" | "error";

export interface Notice {
  id: number;
  level: NoticeLevel;
  message: string;
  /** Optional detail line, e.g. the daemon's error text. */
  detail?: string;
  /** Dismiss automatically after this many ms; omitted means it stays. */
  timeoutMs?: number;
}

/** How many notices are kept before the oldest is dropped. */
export const MAX_NOTICES = 4;
/** Default lifetime for an informational toast. */
export const DEFAULT_TIMEOUT_MS = 6_000;

let nextId = 1;

interface NoticesState {
  notices: Notice[];
  /** Raise a notice and return its id. */
  push: (
    level: NoticeLevel,
    message: string,
    options?: { detail?: string; timeoutMs?: number; key?: string },
  ) => number;
  dismiss: (id: number) => void;
  dismissAll: () => void;
}

/**
 * Keys of notices raised since the app started. A `key` makes a notice
 * idempotent for the session — the connection banner would otherwise re-raise
 * the same message on every failed poll.
 */
const raised = new Set<string>();

export const useNotices = create<NoticesState>((set) => ({
  notices: [],
  push: (level, message, options = {}) => {
    const { key } = options;
    if (key) {
      if (raised.has(key)) return 0;
      raised.add(key);
    }
    const id = nextId++;
    const notice: Notice = {
      id,
      level,
      message,
      detail: options.detail,
      timeoutMs:
        options.timeoutMs ??
        (level === "error" ? undefined : DEFAULT_TIMEOUT_MS),
    };
    set((state) => ({
      notices: [...state.notices, notice].slice(-MAX_NOTICES),
    }));
    if (notice.timeoutMs !== undefined) {
      // Auto-dismiss. Kept out of the store so a test does not need fake timers
      // to observe the notice itself.
      const timer = setTimeout(() => {
        useNotices.getState().dismiss(id);
      }, notice.timeoutMs);
      // Do not hold a Node test process open on a pending toast.
      (timer as { unref?: () => void }).unref?.();
    }
    return id;
  },
  dismiss: (id) =>
    set((state) => ({
      notices: state.notices.filter((notice) => notice.id !== id),
    })),
  dismissAll: () => set({ notices: [] }),
}));

/** Test seam: forget which keyed notices have been raised. */
export function resetNoticeKeys(): void {
  raised.clear();
}

/**
 * Report a failed daemon action. Errors are never silent: the caller may also
 * have its own recovery, but the operator always sees what went wrong.
 */
export function reportFailure(message: string, error: unknown): void {
  const detail = error instanceof Error ? error.message : String(error ?? "");
  useNotices.getState().push("error", message, { detail: detail || undefined });
}
