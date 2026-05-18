import { beforeEach, describe, expect, it, vi } from 'vitest';

import {
    DEFAULT_ACTION,
    addDpiStage,
    bindingForButton,
    cloneProfile,
    debounce,
    removeDpiStage,
    resetProfileToDefaults,
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

describe('resetProfileToDefaults', () => {
    // Unknown model → generic fallback (mouse 1–5, rest disabled).
    const UNKNOWN = 'usb:0000:0000:0';
    const G502 = 'usb:046d:c08b:0';

    it('falls back to generic mouse defaults for unknown devices', () => {
        const p = profile({ dpi: [400, 800, 1600], active_dpi_stage: 2, buttons: [] });
        const reset = resetProfileToDefaults(p, [0, 1, 2, 3, 4], UNKNOWN);
        expect(reset.buttons).toEqual([
            { index: 0, action: { kind: BUTTON_ACTION_KIND.MOUSE, value: 1, macro_steps: [] } },
            { index: 1, action: { kind: BUTTON_ACTION_KIND.MOUSE, value: 2, macro_steps: [] } },
            { index: 2, action: { kind: BUTTON_ACTION_KIND.MOUSE, value: 3, macro_steps: [] } },
            { index: 3, action: { kind: BUTTON_ACTION_KIND.MOUSE, value: 4, macro_steps: [] } },
            { index: 4, action: { kind: BUTTON_ACTION_KIND.MOUSE, value: 5, macro_steps: [] } },
        ]);
    });

    it('disables buttons beyond the first five on unknown devices', () => {
        const reset = resetProfileToDefaults(profile(), [0, 5, 6, 7], UNKNOWN);
        expect(reset.buttons.find((b) => b.index === 5)?.action).toEqual(DEFAULT_ACTION);
        expect(reset.buttons.find((b) => b.index === 7)?.action).toEqual(DEFAULT_ACTION);
    });

    it('uses the per-device table for known devices (G502 HERO)', () => {
        // G502 HERO ships buttons 6–10 as resolution / profile-cycle /
        // wheel-tilt specials, not Disabled. Verify a few index→action
        // pairs to pin the table.
        const reset = resetProfileToDefaults(
            profile(),
            [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            G502,
        );
        const byIndex = new Map(reset.buttons.map((b) => [b.index, b.action]));
        // 0–4 are still the standard mouse 1–5.
        expect(byIndex.get(0)?.value).toBe(1);
        expect(byIndex.get(4)?.value).toBe(5);
        // 6 = resolution-down (special).
        expect(byIndex.get(6)?.kind).toBe(BUTTON_ACTION_KIND.SPECIAL);
        // 8 = profile-cycle-up (special) — gamerat-default, not Disabled.
        expect(byIndex.get(8)?.kind).toBe(BUTTON_ACTION_KIND.SPECIAL);
        // 10 = wheel-left (special).
        expect(byIndex.get(10)?.kind).toBe(BUTTON_ACTION_KIND.SPECIAL);
    });

    it('collapses DPI to a single 800 stage with stage 0 active', () => {
        const reset = resetProfileToDefaults(
            profile({ dpi: [400, 1200, 2400], active_dpi_stage: 2 }),
            [0],
            UNKNOWN,
        );
        expect(reset.dpi).toEqual([800]);
        expect(reset.active_dpi_stage).toBe(0);
    });

    it('preserves metadata (id, name, category, ...)', () => {
        const original = profile({ id: 'fps', name: 'FPS', category: 'specific' });
        const reset = resetProfileToDefaults(original, [0], UNKNOWN);
        expect(reset.id).toBe('fps');
        expect(reset.name).toBe('FPS');
        expect(reset.category).toBe('specific');
    });

    it('returns sorted-by-index buttons regardless of input order', () => {
        const reset = resetProfileToDefaults(profile(), [3, 0, 7, 1], UNKNOWN);
        expect(reset.buttons.map((b) => b.index)).toEqual([0, 1, 3, 7]);
    });
});

describe('cloneProfile', () => {
    it('returns a deep copy decoupled from the source', () => {
        const original = profile({
            buttons: [{ index: 4, action: action(BUTTON_ACTION_KIND.KEY, 30) }],
        });
        const clone = cloneProfile(original);
        expect(clone).toEqual(original);
        expect(clone).not.toBe(original);
        expect(clone.buttons).not.toBe(original.buttons);
        expect(clone.buttons[0]).not.toBe(original.buttons[0]);
        expect(clone.dpi).not.toBe(original.dpi);
    });

    it('mutations on the clone do not leak back to the source', () => {
        const original = profile({ dpi: [400, 800] });
        // Bypass readonly to verify deep-copy isolation — the
        // GameratProfile type marks fields readonly, but the JS
        // shape allows mutation; that's exactly what we're checking.
        const clone = cloneProfile(original) as {
            -readonly [K in keyof GameratProfile]: GameratProfile[K];
        } & { dpi: number[] };
        clone.dpi.push(1600);
        clone.name = 'mutated';
        expect(original.dpi).toEqual([400, 800]);
        expect(original.name).toBe('FPS');
    });
});
