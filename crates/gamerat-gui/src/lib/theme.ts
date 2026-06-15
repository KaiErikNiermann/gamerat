/**
 * Theme preference persistence.
 *
 * Three values:
 *   - "system" — follow `prefers-color-scheme` (no `data-theme` attr).
 *   - "light"  — force light tokens via `data-theme="light"`.
 *   - "dark"   — force dark tokens via `data-theme="dark"`.
 *
 * Stored in localStorage under `gamerat:theme`. The CSS in app.css
 * defines the cascade — this module only flips the attribute on
 * `<html>`, never touches actual colour values.
 */

const STORAGE_KEY = 'gamerat:theme';

const THEMES = ['system', 'light', 'dark'] as const;

export type Theme = (typeof THEMES)[number];

function isTheme(value: string | null): value is Theme {
    return value !== null && (THEMES as readonly string[]).includes(value);
}

export function loadTheme(): Theme {
    try {
        const raw = localStorage.getItem(STORAGE_KEY);
        return isTheme(raw) ? raw : 'system';
    } catch {
        // Tauri's webview should always expose localStorage, but if it's
        // unavailable for any reason (private mode, sandboxed contexts)
        // fall back to system rather than crashing.
        return 'system';
    }
}

export function saveTheme(theme: Theme): void {
    try {
        localStorage.setItem(STORAGE_KEY, theme);
    } catch {
        /* see loadTheme — silent fallback. */
    }
}

export function applyTheme(theme: Theme): void {
    // setAttribute / removeAttribute over `dataset` because the
    // dataset proxy doesn't reliably reflect `delete` across all
    // browser builds — the symptom was the toggle visually "doing
    // nothing" on some Chromium/WebKit versions. The plain DOM API
    // is unambiguous.
    const root = document.documentElement;
    if (theme === 'system') {
        // eslint-disable-next-line unicorn/dom-node-dataset -- see comment above
        root.removeAttribute('data-theme');
    } else {
        // eslint-disable-next-line unicorn/dom-node-dataset -- see comment above
        root.setAttribute('data-theme', theme);
    }
}

/** Cycle order for the header toggle button: system → light → dark → … */
export function nextTheme(theme: Theme): Theme {
    if (theme === 'system') return 'light';
    if (theme === 'light') return 'dark';
    return 'system';
}
