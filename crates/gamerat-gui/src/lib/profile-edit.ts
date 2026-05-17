/**
 * Pure helpers for the profile-edit flow in MouseView. Extracted so
 * the tricky bits — partial binding lookup, in-place button-update,
 * debounce semantics — can be unit-tested in isolation rather than
 * via a full component render.
 */

import { BUTTON_ACTION_KIND } from './types.js';
import type { ButtonAction, GameratProfile, ProfileButton } from './types.js';

/** Default action for a button the profile doesn't declare. We
 *  treat unspecified buttons as Disabled at render time so the
 *  on-screen label is unambiguous. The daemon's apply path only
 *  writes the buttons present in `profile.buttons`, so this is
 *  purely a UI convention. */
export const DEFAULT_ACTION: ButtonAction = Object.freeze({
    kind: BUTTON_ACTION_KIND.NONE,
    value: 0,
    macro_steps: [],
});

/**
 * Return the action a profile binds to `buttonIndex`. Falls back to
 * `DEFAULT_ACTION` (Disabled) when the profile doesn't list the
 * button — i.e. the user hasn't explicitly set it yet.
 */
export function bindingForButton(
    profile: GameratProfile,
    buttonIndex: number,
): ButtonAction {
    const found = profile.buttons.find((b) => b.index === buttonIndex);
    return found?.action ?? DEFAULT_ACTION;
}

/**
 * Produce a new `GameratProfile` with `buttonIndex` re-bound to
 * `action`. Existing entry is replaced in place; missing index gets
 * appended and the list is re-sorted by index for stable
 * persistence diffs.
 *
 * Returns a fresh object — the input is untouched, which keeps
 * Svelte's reactivity happy and makes tests trivial.
 */
export function setBinding(
    profile: GameratProfile,
    buttonIndex: number,
    action: ButtonAction,
): GameratProfile {
    const next: ProfileButton[] = profile.buttons.map((b) =>
        b.index === buttonIndex ? { index: b.index, action } : b,
    );
    if (!next.some((b) => b.index === buttonIndex)) {
        next.push({ index: buttonIndex, action });
    }
    next.sort((a, b) => a.index - b.index);
    return { ...profile, buttons: next };
}

/**
 * Tiny debounce helper. Returns a function that re-arms a single
 * pending timer on each call; the wrapped action fires once after
 * `delayMs` of quiet. Exposes `cancel()` so the component teardown
 * can drop any pending save.
 *
 * Uses generic arg-spreading so types flow through cleanly.
 */
export interface DebouncedFn<TArgs extends readonly unknown[]> {
    (...args: TArgs): void;
    cancel(): void;
}

export function debounce<TArgs extends readonly unknown[]>(
    fn: (...args: TArgs) => void,
    delayMs: number,
): DebouncedFn<TArgs> {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const debounced = ((...args: TArgs) => {
        if (timer !== undefined) clearTimeout(timer);
        timer = setTimeout(() => {
            timer = undefined;
            fn(...args);
        }, delayMs);
    }) as DebouncedFn<TArgs>;
    debounced.cancel = (): void => {
        if (timer !== undefined) {
            clearTimeout(timer);
            timer = undefined;
        }
    };
    return debounced;
}

/**
 * Update one DPI stage's value, preserving the active-stage index.
 * Useful for the DPI editor row inside MouseView's profile mode.
 */
export function setDpiStage(
    profile: GameratProfile,
    stageIndex: number,
    value: number,
): GameratProfile {
    if (stageIndex < 0 || stageIndex >= profile.dpi.length) return profile;
    const dpi = profile.dpi.map((v, i) => (i === stageIndex ? value : v));
    return { ...profile, dpi };
}

/**
 * Add one DPI stage at the end (cloning the last stage's value as
 * a reasonable default).
 */
export function addDpiStage(profile: GameratProfile): GameratProfile {
    const last = profile.dpi.at(-1) ?? 800;
    return { ...profile, dpi: [...profile.dpi, last] };
}

/**
 * Remove a DPI stage. Refuses to drop the last stage — the daemon
 * requires at least one. Clamps `active_dpi_stage` if the removal
 * leaves it out of bounds.
 */
export function removeDpiStage(
    profile: GameratProfile,
    stageIndex: number,
): GameratProfile {
    if (profile.dpi.length <= 1) return profile;
    if (stageIndex < 0 || stageIndex >= profile.dpi.length) return profile;
    const dpi = profile.dpi.filter((_, i) => i !== stageIndex);
    const active_dpi_stage = Math.min(profile.active_dpi_stage, dpi.length - 1);
    return { ...profile, dpi, active_dpi_stage };
}

/**
 * Set which DPI stage is the default-active.
 */
export function setActiveDpiStage(
    profile: GameratProfile,
    stageIndex: number,
): GameratProfile {
    if (stageIndex < 0 || stageIndex >= profile.dpi.length) return profile;
    return { ...profile, active_dpi_stage: stageIndex };
}
