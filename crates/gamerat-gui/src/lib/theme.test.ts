import { beforeEach, describe, expect, it } from 'vitest';

import { applyTheme, loadTheme, nextTheme, saveTheme } from './theme.js';

// jsdom under vitest 4 doesn't expose every Storage method — calling
// `.clear()` blows up with "is not a function". Targeted removeItem
// for the one key the theme module touches sidesteps it.
const KEY = 'gamerat:theme';

describe('theme', () => {
    beforeEach(() => {
        localStorage.removeItem(KEY);
        // The theme module deliberately uses setAttribute / removeAttribute
        // (rather than dataset) for cross-browser reliability — the test
        // mirrors that pattern, with inline lint disables to keep the
        // unicorn/dom-node-dataset rule from flagging it.
        // eslint-disable-next-line unicorn/dom-node-dataset
        document.documentElement.removeAttribute('data-theme');
    });

    describe('loadTheme', () => {
        it('returns "system" when no preference is saved', () => {
            expect(loadTheme()).toBe('system');
        });

        it('returns the saved preference when valid', () => {
            localStorage.setItem('gamerat:theme', 'light');
            expect(loadTheme()).toBe('light');
            localStorage.setItem('gamerat:theme', 'dark');
            expect(loadTheme()).toBe('dark');
            localStorage.setItem('gamerat:theme', 'system');
            expect(loadTheme()).toBe('system');
        });

        it('falls back to "system" for garbage values', () => {
            localStorage.setItem('gamerat:theme', 'wat');
            expect(loadTheme()).toBe('system');
        });
    });

    describe('saveTheme', () => {
        it('persists the value', () => {
            saveTheme('dark');
            expect(localStorage.getItem('gamerat:theme')).toBe('dark');
        });
    });

    describe('applyTheme', () => {
        it('sets data-theme to "light" / "dark"', () => {
            applyTheme('light');
            /* eslint-disable unicorn/dom-node-dataset */
            expect(document.documentElement.getAttribute('data-theme')).toBe('light');
            applyTheme('dark');
            expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
            /* eslint-enable unicorn/dom-node-dataset */
        });

        it('removes data-theme when set to "system"', () => {
            /* eslint-disable unicorn/dom-node-dataset */
            document.documentElement.setAttribute('data-theme', 'light');
            applyTheme('system');
            expect(document.documentElement.hasAttribute('data-theme')).toBe(false);
            /* eslint-enable unicorn/dom-node-dataset */
        });
    });

    describe('nextTheme', () => {
        it('cycles system → light → dark → system', () => {
            expect(nextTheme('system')).toBe('light');
            expect(nextTheme('light')).toBe('dark');
            expect(nextTheme('dark')).toBe('system');
        });
    });
});
