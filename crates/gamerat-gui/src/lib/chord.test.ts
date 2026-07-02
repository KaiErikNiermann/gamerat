import { describe, expect, it } from 'vitest';

import { chordToSteps, formatChord, regularKeyPressCount, stepsToChord } from './chord.js';
import { MACRO_EVENT_KIND, type MacroStep } from './types.js';

const P = MACRO_EVENT_KIND.KEY_PRESS;
const R = MACRO_EVENT_KIND.KEY_RELEASE;
const W = MACRO_EVENT_KIND.WAIT;

// Evdev codes: 56 = L Alt, 30 = A, 29 = L Ctrl, 42 = L Shift, 32 = D.
const step = (kind: number, value: number): MacroStep => ({ kind, value });

describe('stepsToChord', () => {
    it('parses a modifier+key chord regardless of release order or waits', () => {
        // The exact Wolfenstein shape: Alt released before A, with a wait.
        const chord = stepsToChord([
            step(P, 56),
            step(W, 517),
            step(P, 30),
            step(R, 56),
            step(R, 30),
        ]);
        expect(chord).toEqual({ key: 30, modifiers: [56] });
    });

    it('keeps multiple modifiers in press order', () => {
        const chord = stepsToChord([
            step(P, 29),
            step(P, 42),
            step(P, 30),
            step(R, 30),
            step(R, 42),
            step(R, 29),
        ]);
        expect(chord).toEqual({ key: 30, modifiers: [29, 42] });
    });

    it('rejects a bare single key with no modifier (may encode a hold)', () => {
        expect(stepsToChord([step(P, 30), step(W, 25), step(R, 30)])).toBeNull();
    });

    it('rejects genuine multi-key sequences', () => {
        expect(
            stepsToChord([step(P, 30), step(R, 30), step(P, 32), step(R, 32)]),
        ).toBeNull();
    });

    it('rejects unbalanced press/release', () => {
        expect(stepsToChord([step(P, 56), step(P, 30), step(R, 30)])).toBeNull();
    });
});

describe('chordToSteps', () => {
    it('emits presses then key-release-first, modifiers released in reverse', () => {
        expect(chordToSteps({ key: 30, modifiers: [29, 56] })).toEqual([
            step(P, 29),
            step(P, 56),
            step(P, 30),
            step(R, 30),
            step(R, 56),
            step(R, 29),
        ]);
    });

    it('round-trips through stepsToChord', () => {
        const chord = { key: 30, modifiers: [56] };
        expect(stepsToChord(chordToSteps(chord))).toEqual(chord);
    });
});

describe('formatChord', () => {
    it('renders modifiers first, key last', () => {
        expect(formatChord({ key: 30, modifiers: [56] })).toBe('L Alt + A');
        expect(formatChord({ key: 30, modifiers: [29, 42] })).toBe('L Ctrl + L Shift + A');
    });
});

describe('regularKeyPressCount', () => {
    it('counts non-modifier presses only', () => {
        expect(
            regularKeyPressCount([step(P, 56), step(P, 30), step(P, 32), step(R, 30)]),
        ).toBe(2);
    });
});
