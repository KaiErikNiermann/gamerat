/**
 * Pure helpers for the profile-edit flow in MouseView. Extracted so
 * the tricky bits — partial binding lookup, in-place button-update,
 * debounce semantics — can be unit-tested in isolation rather than
 * via a full component render.
 */

import { defaultBindingsFor } from './device-defaults.js';
import { BUTTON_ACTION_KIND, SOFT_MACRO_KIND } from './types.js';
import type {
    ButtonAction,
    GameratProfile,
    ProfileButton,
    ProfileLed,
    SoftMacro,
} from './types.js';

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
/**
 * Reset a profile's bindings to the device's factory defaults. Used
 * by the MouseView "Reset to defaults" affordance.
 *
 * Bindings come from `device-defaults.ts`'s per-device table when we
 * have one (G502 HERO + whatever else has been seeded), with a
 * generic 5-button fallback (`mouse 1`..`mouse 5`, rest Disabled)
 * for unrecognised models. DPI collapses to a single 800-stage with
 * that stage active.
 *
 * Why per-device: libratbag and ratbagd deliberately don't expose
 * factory bindings (issue #1302), and HID++ has no documented
 * "load factory defaults" call. The hidpp20-reset tool zeros every
 * onboard sector, which is a wipe — not a restore. The only honest
 * way to put a known-good set of bindings back is to know what the
 * factory shipped and write it ourselves.
 *
 * `buttonIndices` is the set of physical button indices the device
 * exposes (`liveButtons.map(b => b.index)`); needed because
 * `profile.buttons` only carries the user's explicit overrides and
 * we want the materialised profile to be self-contained.
 */
export function resetProfileToDefaults(
    profile: GameratProfile,
    buttonIndices: readonly number[],
    model: string,
): GameratProfile {
    const buttons = defaultBindingsFor(model, buttonIndices);
    return {
        ...profile,
        dpi: [800],
        active_dpi_stage: 0,
        buttons: [...buttons],
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
 * Add or replace the {@link SoftMacro} entry for `buttonIndex` inside
 * `profile.soft_macros`. Identical immutable-update pattern to
 * {@link setBinding}; the returned profile carries a fresh object so
 * Svelte's reactivity picks up the change.
 *
 * When `softMacro.kind === SOFT_MACRO_KIND.DISABLED` the entry is
 * dropped instead of added — that's the wire-stable "no soft-macro
 * here" representation, and keeping it inert in storage is cleaner
 * than persisting an obviously-dormant record.
 */
export function setSoftMacro(
    profile: GameratProfile,
    buttonIndex: number,
    softMacro: SoftMacro,
): GameratProfile {
    const withoutTarget = profile.soft_macros.filter((m) => m.button_index !== buttonIndex);
    const next = softMacro.kind === SOFT_MACRO_KIND.DISABLED
        ? [...withoutTarget]
        : [...withoutTarget, { ...softMacro, button_index: buttonIndex }];
    next.sort((a, b) => a.button_index - b.button_index);
    return { ...profile, soft_macros: next };
}

/**
 * Mirror of {@link setBinding} for LEDs. Replaces any existing entry
 * for `ledIndex` in place; missing index gets appended and the list
 * is re-sorted by index. Returns a fresh `GameratProfile` so callers
 * can pipe the result back into Svelte's reactive draft without
 * mutating the input.
 */
export function setLed(
    profile: GameratProfile,
    ledIndex: number,
    state: Omit<ProfileLed, 'index'>,
): GameratProfile {
    const entry: ProfileLed = { index: ledIndex, ...state };
    const next: ProfileLed[] = profile.leds.map((l) =>
        l.index === ledIndex ? entry : l,
    );
    if (!next.some((l) => l.index === ledIndex)) {
        next.push(entry);
    }
    next.sort((a, b) => a.index - b.index);
    return { ...profile, leds: next };
}

/**
 * Return the LED state a profile declares for `ledIndex`, or `null`
 * when the profile doesn't declare anything for that LED. Callers use
 * the null case to fall back to live hardware state (in Base mode) or
 * to render a neutral "not configured" affordance (in profile mode).
 */
export function ledForIndex(
    profile: GameratProfile,
    ledIndex: number,
): ProfileLed | null {
    return profile.leds.find((l) => l.index === ledIndex) ?? null;
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
 * a reasonable default). No-op when already at `maxStages`, since
 * the device has no slot to hold the new stage — the editor's
 * "+ add stage" button is hidden in that state too, but we guard
 * here as well so CLI / programmatic callers can't blow past the
 * limit.
 *
 * `maxStages` is the hardware's resolution-slot count (from
 * DeviceInfo.max_dpi_stages); pass `Infinity` if you don't have it
 * (e.g. tests without a device context).
 */
export function addDpiStage(
    profile: GameratProfile,
    maxStages: number = Number.POSITIVE_INFINITY,
): GameratProfile {
    if (profile.dpi.length >= maxStages) return profile;
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
