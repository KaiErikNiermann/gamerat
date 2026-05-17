import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
    DEFAULT_ACTION,
    addDpiStage,
    bindingForButton,
    debounce,
    removeDpiStage,
    setActiveDpiStage,
    setBinding,
    setDpiStage,
} from './profile-edit.js';
import { BUTTON_ACTION_KIND, type ButtonAction, type GameratProfile } from './types.js';

function profile(overrides: Partial<GameratProfile> = {}): GameratProfile {
    return {
        id: 'fps-low-dpi',
        name: 'FPS',
        description: '',
        category: 'agnostic',
        inherits_from: '',
        dpi: [400, 800, 1600],
        active_dpi_stage: 1,
        created_unix: 0,
        buttons: [],
        ...overrides,
    };
}

function action(kind: ButtonAction['kind'], value = 0): ButtonAction {
    return { kind, value, macro_steps: [] };
}

describe('bindingForButton', () => {
    it('falls back to DEFAULT_ACTION (Disabled) for unbound buttons', () => {
        expect(bindingForButton(profile(), 3)).toBe(DEFAULT_ACTION);
        expect(bindingForButton(profile(), 3).kind).toBe(BUTTON_ACTION_KIND.NONE);
    });

    it('returns the matching ProfileButton.action when present', () => {
        const p = profile({
            buttons: [
                { index: 0, action: action(BUTTON_ACTION_KIND.MOUSE, 0) },
                { index: 3, action: action(BUTTON_ACTION_KIND.KEY, 30) },
            ],
        });
        expect(bindingForButton(p, 3)).toEqual(action(BUTTON_ACTION_KIND.KEY, 30));
    });
});

describe('setBinding', () => {
    it('appends a new binding when the index is unbound', () => {
        const next = setBinding(profile(), 5, action(BUTTON_ACTION_KIND.KEY, 30));
        expect(next.buttons).toHaveLength(1);
        expect(next.buttons[0]?.index).toBe(5);
        expect(next.buttons[0]?.action.value).toBe(30);
    });

    it('replaces an existing binding in place', () => {
        const p = profile({
            buttons: [
                { index: 0, action: action(BUTTON_ACTION_KIND.MOUSE, 0) },
                { index: 3, action: action(BUTTON_ACTION_KIND.KEY, 30) },
            ],
        });
        const next = setBinding(p, 3, action(BUTTON_ACTION_KIND.KEY, 31));
        expect(next.buttons).toHaveLength(2);
        const updated = next.buttons.find((b) => b.index === 3);
        expect(updated?.action.value).toBe(31);
    });

    it('keeps buttons sorted by index for stable persistence diffs', () => {
        const p = profile({
            buttons: [{ index: 5, action: action(BUTTON_ACTION_KIND.MOUSE, 0) }],
        });
        const next = setBinding(p, 1, action(BUTTON_ACTION_KIND.KEY, 30));
        expect(next.buttons.map((b) => b.index)).toEqual([1, 5]);
    });

    it('returns a new object — input untouched', () => {
        const original = profile();
        const next = setBinding(original, 3, action(BUTTON_ACTION_KIND.KEY, 30));
        expect(next).not.toBe(original);
        expect(original.buttons).toHaveLength(0);
    });
});

describe('DPI mutations', () => {
    it('setDpiStage replaces one value, preserves the rest', () => {
        const next = setDpiStage(profile(), 1, 900);
        expect(next.dpi).toEqual([400, 900, 1600]);
        expect(next.active_dpi_stage).toBe(1);
    });

    it('setDpiStage is a no-op for out-of-range indices', () => {
        const p = profile();
        expect(setDpiStage(p, -1, 999)).toEqual(p);
        expect(setDpiStage(p, 99, 999)).toEqual(p);
    });

    it('addDpiStage appends, defaulting to the previous tail', () => {
        const next = addDpiStage(profile());
        expect(next.dpi).toEqual([400, 800, 1600, 1600]);
    });

    it('removeDpiStage refuses to drop the last stage', () => {
        const p = profile({ dpi: [800], active_dpi_stage: 0 });
        expect(removeDpiStage(p, 0)).toEqual(p);
    });

    it('removeDpiStage clamps active_dpi_stage if it ends up out of bounds', () => {
        // active stage 2, length 3. Remove stage 2 → length 2, active should clamp to 1.
        const p = profile({ dpi: [400, 800, 1600], active_dpi_stage: 2 });
        const next = removeDpiStage(p, 2);
        expect(next.dpi).toEqual([400, 800]);
        expect(next.active_dpi_stage).toBe(1);
    });

    it('setActiveDpiStage updates the index only when in range', () => {
        expect(setActiveDpiStage(profile(), 0).active_dpi_stage).toBe(0);
        expect(setActiveDpiStage(profile(), 99).active_dpi_stage).toBe(1); // no-op
    });
});

describe('debounce', () => {
    beforeEach(() => {
        vi.useFakeTimers();
    });

    it('fires once after the delay, with the latest args', () => {
        const fn = vi.fn();
        const d = debounce(fn, 500);
        d('a');
        d('b');
        d('c');
        expect(fn).not.toHaveBeenCalled();
        vi.advanceTimersByTime(499);
        expect(fn).not.toHaveBeenCalled();
        vi.advanceTimersByTime(1);
        expect(fn).toHaveBeenCalledTimes(1);
        expect(fn).toHaveBeenCalledWith('c');
    });

    it('cancel() prevents a pending call from firing', () => {
        const fn = vi.fn();
        const d = debounce(fn, 500);
        d('a');
        d.cancel();
        vi.advanceTimersByTime(1000);
        expect(fn).not.toHaveBeenCalled();
    });

    it('subsequent calls re-arm a fresh timer', () => {
        const fn = vi.fn();
        const d = debounce(fn, 500);
        d('a');
        vi.advanceTimersByTime(600);
        expect(fn).toHaveBeenCalledTimes(1);
        d('b');
        vi.advanceTimersByTime(600);
        expect(fn).toHaveBeenCalledTimes(2);
        expect(fn).toHaveBeenLastCalledWith('b');
    });
});
