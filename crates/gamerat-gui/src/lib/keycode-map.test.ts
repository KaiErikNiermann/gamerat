import { describe, expect, it } from 'vitest';

import {
    ALL_KNOWN_KEYCODES,
    KEY_OPTIONS,
    keycodeFromBrowserCode,
    nameForKeycode,
} from './keycode-map.js';

describe('keycode-map', () => {
    describe('keycodeFromBrowserCode', () => {
        it.each([
            ['KeyA', 30],
            ['KeyB', 48],
            ['KeyZ', 44],
            ['Digit1', 2],
            ['Digit0', 11],
            ['F1', 59],
            ['F12', 88],
            ['Space', 57],
            ['Enter', 28],
            ['Escape', 1],
            ['ArrowUp', 103],
            ['ArrowDown', 108],
            ['ShiftLeft', 42],
            ['ControlRight', 97],
            ['NumpadDivide', 98],
        ])('maps %s → %i', (code, keycode) => {
            expect(keycodeFromBrowserCode(code)).toBe(keycode);
        });

        it('returns null for unmapped codes', () => {
            expect(keycodeFromBrowserCode('Lang5')).toBeNull();
            expect(keycodeFromBrowserCode('')).toBeNull();
            expect(keycodeFromBrowserCode('NotAKey')).toBeNull();
        });
    });

    describe('nameForKeycode', () => {
        it('returns short friendly names for well-known keycodes', () => {
            expect(nameForKeycode(30)).toBe('A');
            expect(nameForKeycode(57)).toBe('Space');
            // Up / Down use filled triangles for legibility at the
            // ~0.72rem label font. Left / Right keep line-arrows.
            expect(nameForKeycode(103)).toBe('▲');
            expect(nameForKeycode(108)).toBe('▼');
            expect(nameForKeycode(105)).toBe('←');
            expect(nameForKeycode(106)).toBe('→');
            expect(nameForKeycode(125)).toBe('L Meta');
        });

        it('falls back to "Key N" for unmapped keycodes', () => {
            expect(nameForKeycode(999)).toBe('Key 999');
            expect(nameForKeycode(0)).toBe('Key 0');
        });
    });

    describe('KEY_OPTIONS', () => {
        it('is sorted by name (case-insensitive)', () => {
            const names = KEY_OPTIONS.map((o) => o.name.toLowerCase());
            const sorted = [...names].sort((a, b) => a.localeCompare(b));
            expect(names).toEqual(sorted);
        });

        it('exposes a code, keycode, and name for every entry', () => {
            for (const opt of KEY_OPTIONS) {
                expect(opt.code).toMatch(/^\S+$/u);
                expect(typeof opt.keycode).toBe('number');
                expect(opt.keycode).toBeGreaterThan(0);
                expect(opt.name.length).toBeGreaterThan(0);
            }
        });
    });

    describe('ALL_KNOWN_KEYCODES', () => {
        it('contains no duplicates and is sorted numerically', () => {
            const sorted = [...ALL_KNOWN_KEYCODES].sort((a, b) => a - b);
            expect(ALL_KNOWN_KEYCODES).toEqual(sorted);
            const unique = new Set(ALL_KNOWN_KEYCODES);
            expect(unique.size).toBe(ALL_KNOWN_KEYCODES.length);
        });

        it('covers all 26 letters', () => {
            // KEY_A is 30; the letters aren't contiguous in the
            // input-event-codes map, so just spot-check that each is present.
            const letterCodes = [
                30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25,
                16, 19, 31, 20, 22, 47, 17, 45, 21, 44,
            ];
            for (const code of letterCodes) {
                expect(ALL_KNOWN_KEYCODES).toContain(code);
            }
        });

        it('covers F1 through F12', () => {
            for (let i = 0; i < 10; i++) {
                expect(ALL_KNOWN_KEYCODES).toContain(59 + i); // F1..F10
            }
            expect(ALL_KNOWN_KEYCODES).toContain(87); // F11
            expect(ALL_KNOWN_KEYCODES).toContain(88); // F12
        });
    });

    it('round-trips browser-code ↔ name via keycode', () => {
        // Every KEY_OPTIONS entry should be self-consistent.
        for (const opt of KEY_OPTIONS) {
            expect(keycodeFromBrowserCode(opt.code)).toBe(opt.keycode);
            expect(nameForKeycode(opt.keycode)).toBe(opt.name);
        }
    });
});
