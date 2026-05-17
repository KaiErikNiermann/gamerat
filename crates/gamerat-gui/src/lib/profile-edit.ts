/**
 * Pure helpers for the profile-edit flow in MouseView. Extracted so
 * the tricky bits — partial binding lookup, in-place button-update,
 * debounce semantics — can be unit-tested in isolation rather than
 * via a full component render.
 */

import { BUTTON_ACTION_KIND } from './types.js';
import type { ButtonAction, GameratProfile, ProfileButton } from './types.js';

/**
 * Deep-copy a `GameratProfile` into a plain object detached from any
 * Svelte 5 `$state` proxy. Raw `structuredClone(profile)` on a state
 * proxy throws `DataCloneError: The object can not be cloned` — the
 * proxy's internals aren't cloneable. `$state.snapshot` would work
 * but it's a rune (only available in `.svelte` / `.svelte.ts` files).
 *
 * JSON-roundtrip is safe here because GameratProfile is plain data —
 * strings, numbers, arrays of ProfileButton/MacroStep. Profiles are
 * small (a handful of buttons), so the perf cost is irrelevant.
 */
export function cloneProfile(profile: GameratProfile): GameratProfile {
    // eslint-disable-next-line unicorn/prefer-structured-clone -- see above; structuredClone throws DataCloneError on $state proxies
    return JSON.parse(JSON.stringify(profile)) as GameratProfile;
}

/**
 * Canonical-defaults binding for a given button index: the
 * "factory-style" mapping most desktop users expect (left, right,
 * middle, back, forward). Higher indices fall back to Disabled — we
 * don't know what the device-specific extras are without per-mouse
 * data, and Disabled is the safe choice (the user can re-bind from
 * the editor).
 */
function defaultActionForButton(index: number): ButtonAction {
    if (index >= 0 && index <= 4) {
        // libratbag's button-number convention is 1-indexed.
        return {
            kind: BUTTON_ACTION_KIND.MOUSE,
            value: index + 1,
            macro_steps: [],
        };
    }
    return DEFAULT_ACTION;
}

/**
 * Reset a profile's customisations to canonical defaults. Used by the
 * MouseView "Reset to defaults" affordance.
 *
 * The defaults are intentionally generic: buttons 0–4 get the
 * standard mouse mappings (Left/Right/Middle/Back/Forward), all
 * other buttons are cleared to Disabled. DPI collapses to a single
 * 800-DPI stage with that stage active. This won't be a perfect
 * match for every device's firmware defaults, but it covers the
 * five buttons everyone has and gives the user a known-good
 * starting point.
 *
 * `buttonIndices` is the set of physical button indices the device
 * exposes (from `liveButtons.map(b => b.index)`). Without it, we
 * couldn't enumerate the profile's full button list — `profile.buttons`
 * only contains the user's explicit overrides.
 */
export function resetProfileToDefaults(
    profile: GameratProfile,
    buttonIndices: readonly number[],
): GameratProfile {
    const buttons: ProfileButton[] = [...buttonIndices]
        .sort((a, b) => a - b)
        .map((index) => ({ index, action: defaultActionForButton(index) }));
    return {
        ...profile,
        dpi: [800],
        active_dpi_stage: 0,
        buttons,
    };
}

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
