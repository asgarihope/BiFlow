// vitest runs `setupFiles` in order; this file is listed before setup.ts so
// the shim is installed before any setup import (notably `i18n/config`,
// which reads the saved language at module-load time) runs.
//
// Node 26 ships an experimental `localStorage` global that stays
// `undefined` unless `--localstorage-file` is passed, and jsdom 26 defers
// to it instead of providing its own, so `window.localStorage` is also
// absent. Tests and the i18n bootstrap both need a working Storage, so
// install a minimal in-memory implementation on the shared global.

interface StorageEntry {
  value: string;
}

const store = new Map<string, StorageEntry>();

const storage: Storage = {
  get length(): number {
    return store.size;
  },
  clear(): void {
    store.clear();
  },
  getItem(key: string): string | null {
    return store.has(key) ? (store.get(key) as StorageEntry).value : null;
  },
  key(index: number): string | null {
    return Array.from(store.keys())[index] ?? null;
  },
  removeItem(key: string): void {
    store.delete(key);
  },
  setItem(key: string, value: string): void {
    store.set(key, { value: String(value) });
  },
};

Object.defineProperty(globalThis, "localStorage", {
  value: storage,
  writable: true,
  configurable: true,
});

if (typeof window !== "undefined" && window.localStorage === undefined) {
  Object.defineProperty(window, "localStorage", {
    value: storage,
    writable: true,
    configurable: true,
  });
}
