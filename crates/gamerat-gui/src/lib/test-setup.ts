/**
 * Vitest setup file.
 *
 * jsdom 29 under Vitest 4 exposes `localStorage` as a global without
 * the Storage methods (`.clear`, `.removeItem`, etc. are all
 * undefined). Replace it with a tiny in-memory implementation so
 * tests that touch the real Storage API work as expected.
 */

class MemoryStorage implements Storage {
    private readonly store = new Map<string, string>();

    get length(): number {
        return this.store.size;
    }

    clear(): void {
        this.store.clear();
    }

    getItem(key: string): string | null {
        return this.store.get(key) ?? null;
    }

    key(index: number): string | null {
        return [...this.store.keys()][index] ?? null;
    }

    removeItem(key: string): void {
        this.store.delete(key);
    }

    setItem(key: string, value: string): void {
        this.store.set(key, value);
    }
}

Object.defineProperty(globalThis, 'localStorage', {
    value: new MemoryStorage(),
    writable: true,
    configurable: true,
});
