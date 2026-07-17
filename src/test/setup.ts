/**
 * Vitest setup.
 *
 * jsdom under Vitest exposes `window` and `document` but no `localStorage`, so
 * every code path that persists view preferences silently fell into its
 * `catch` during tests and was never actually exercised. Provide a minimal
 * in-memory Storage so those paths run the way they do in the real webview.
 *
 * Registered via `setupFiles`, which runs before test modules are imported —
 * important, because the UI store reads persisted prefs at module load.
 */

class MemoryStorage implements Storage {
  private data = new Map<string, string>();

  get length(): number {
    return this.data.size;
  }

  clear(): void {
    this.data.clear();
  }

  getItem(key: string): string | null {
    return this.data.has(key) ? (this.data.get(key) as string) : null;
  }

  key(index: number): string | null {
    return [...this.data.keys()][index] ?? null;
  }

  removeItem(key: string): void {
    this.data.delete(key);
  }

  setItem(key: string, value: string): void {
    // Real Storage stringifies both key and value.
    this.data.set(String(key), String(value));
  }
}

if (typeof globalThis.localStorage === "undefined") {
  Object.defineProperty(globalThis, "localStorage", {
    value: new MemoryStorage(),
    configurable: true,
    writable: true,
  });
}
