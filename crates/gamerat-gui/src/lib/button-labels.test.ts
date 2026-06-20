import { describe, expect, it } from 'vitest';

import {
    SPECIAL_OPTIONS,
    describeAction,
    describeKeys,
    formatAction,
    formatMacroStep,
    formatSoftMacro,
    kindName,
} from './button-labels.js';
import { BUTTON_ACTION_KIND, BUTTON_SPECIAL, MACRO_EVENT_KIND, SOFT_MACRO_KIND } from './types.js';
import type { ButtonAction } from './types.js';

function action(kind: number, value = 0, macro_steps: ButtonAction['macro_steps'] = []): ButtonAction {
    return { kind: kind as ButtonAction['kind'], value, macro_steps };
}

describe('button-labels', () => {
    describe('formatAction', () => {
        it('returns "Disabled" for NONE', () => {
            expect(formatAction(action(BUTTON_ACTION_KIND.NONE))).toBe('Disabled');
        });

        it('names well-known mouse buttons (0–4)', () => {
            expect(formatAction(action(BUTTON_ACTION_KIND.MOUSE, 0))).toBe('Left');
            expect(formatAction(action(BUTTON_ACTION_KIND.MOUSE, 1))).toBe('Middle');
            expect(formatAction(action(BUTTON_ACTION_KIND.MOUSE, 2))).toBe('Right');
            expect(formatAction(action(BUTTON_ACTION_KIND.MOUSE, 3))).toBe('Back');
            expect(formatAction(action(BUTTON_ACTION_KIND.MOUSE, 4))).toBe('Forward');
        });

        it('falls back to "Mouse N" for unnamed mouse buttons', () => {
            expect(formatAction(action(BUTTON_ACTION_KIND.MOUSE, 7))).toBe('Mouse 7');
        });

        it('names well-known specials', () => {
            expect(
                formatAction(action(BUTTON_ACTION_KIND.SPECIAL, BUTTON_SPECIAL.WHEEL_DOWN)),
            ).toBe('Wheel down');
            expect(
                formatAction(action(BUTTON_ACTION_KIND.SPECIAL, BUTTON_SPECIAL.RESOLUTION_CYCLE_UP)),
            ).toBe('DPI cycle up');
        });

        it('falls back to hex for unknown specials', () => {
            expect(
                formatAction(action(BUTTON_ACTION_KIND.SPECIAL, BUTTON_SPECIAL.BASE + 99)),
            ).toMatch(/^Special [0-9a-f]+$/u);
        });

        it('formats keycodes via the canonical name table', () => {
            // 30 = KEY_A, 57 = KEY_SPACE — both well-known.
            expect(formatAction(action(BUTTON_ACTION_KIND.KEY, 30))).toBe('A');
            expect(formatAction(action(BUTTON_ACTION_KIND.KEY, 57))).toBe('Space');
            // Unmapped keycode → numeric fallback.
            expect(formatAction(action(BUTTON_ACTION_KIND.KEY, 999))).toBe('Key 999');
        });

        it('summarises macros by step count', () => {
            expect(
                formatAction(
                    action(BUTTON_ACTION_KIND.MACRO, 0, [
                        { kind: MACRO_EVENT_KIND.KEY_PRESS, value: 30 },
                        { kind: MACRO_EVENT_KIND.KEY_RELEASE, value: 30 },
                    ]),
                ),
            ).toBe('Macro (2 steps)');
        });

        it('handles empty macros explicitly', () => {
            expect(formatAction(action(BUTTON_ACTION_KIND.MACRO, 0, []))).toBe('Empty macro');
        });
    });

    describe('describeAction', () => {
        it('produces sentence-form copy for each kind', () => {
            expect(describeAction(action(BUTTON_ACTION_KIND.NONE))).toMatch(/^Disabled/u);
            expect(describeAction(action(BUTTON_ACTION_KIND.MOUSE, 1))).toContain('mouse button 1');
            expect(describeAction(action(BUTTON_ACTION_KIND.KEY, 30))).toContain('Keycode 30');
            expect(
                describeAction(action(BUTTON_ACTION_KIND.MACRO, 0, [
                    { kind: MACRO_EVENT_KIND.KEY_PRESS, value: 30 },
                ])),
            ).toContain('1 step');
        });
    });

    describe('formatMacroStep', () => {
        it('renders press / release with the resolved key name and the triangle glyphs', () => {
            expect(formatMacroStep({ kind: MACRO_EVENT_KIND.KEY_PRESS, value: 30 })).toBe('▼ A');
            expect(formatMacroStep({ kind: MACRO_EVENT_KIND.KEY_RELEASE, value: 57 })).toBe(
                '▲ Space',
            );
        });

        it('renders wait in milliseconds with the timer glyph', () => {
            expect(formatMacroStep({ kind: MACRO_EVENT_KIND.WAIT, value: 25 })).toBe('⏲ 25ms');
        });

        it('falls back to "Key N" form for unmapped codes', () => {
            expect(formatMacroStep({ kind: MACRO_EVENT_KIND.KEY_PRESS, value: 999 })).toBe(
                '▼ Key 999',
            );
        });
    });

    describe('kindName', () => {
        it.each([
            [BUTTON_ACTION_KIND.NONE, 'Disabled'],
            [BUTTON_ACTION_KIND.MOUSE, 'Mouse button'],
            [BUTTON_ACTION_KIND.SPECIAL, 'Special action'],
            [BUTTON_ACTION_KIND.KEY, 'Keyboard key'],
            [BUTTON_ACTION_KIND.MACRO, 'Macro'],
        ])('maps kind %i → %s', (kind, expected) => {
            expect(kindName(kind)).toBe(expected);
        });

        it('falls back for unknown kinds', () => {
            expect(kindName(99)).toBe('Kind 99');
        });
    });

    describe('SPECIAL_OPTIONS', () => {
        it('is alphabetically sorted by label', () => {
            const labels = SPECIAL_OPTIONS.map((o) => o.label);
            const sorted = [...labels].toSorted((a, b) => a.localeCompare(b));
            expect(labels).toEqual(sorted);
        });

        it('covers the full Piper-equivalent special-action set', () => {
            // 19 entries: UNKNOWN + DOUBLECLICK + wheel x4 + ratchet + dpi x6 + profile x4 + 2 others.
            expect(SPECIAL_OPTIONS).toHaveLength(19);
        });
    });

    describe('formatSoftMacro', () => {
        it('renders a sticky toggle as "Toggle · <keys>"', () => {
            expect(
                formatSoftMacro({
                    button_index: 4,
                    kind: SOFT_MACRO_KIND.STICKY_TOGGLE,
                    trampoline_keycode: 0,
                    keys: [30, 31],
                }),
            ).toBe(`Toggle · ${describeKeys([30, 31])}`);
        });

        it('falls back to Disabled for an inert (DISABLED) macro', () => {
            expect(
                formatSoftMacro({
                    button_index: 4,
                    kind: SOFT_MACRO_KIND.DISABLED,
                    trampoline_keycode: 0,
                    keys: [],
                }),
            ).toBe('Disabled');
        });
    });
});
