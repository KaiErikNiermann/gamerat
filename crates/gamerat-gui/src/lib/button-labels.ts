/**
 * Human-readable labels for ratbagd button actions.
 *
 * The wire types come over D-Bus as raw u32s — kind / value / macro
 * step list. Translating those into something a user wants to read on
 * a label needs a few lookup tables. Kept centralised so the
 * MouseView and the BindingEditor stay in sync.
 *
 * The well-known maps mirror Piper's labelling (`BUTTON_DESCRIPTION`,
 * `SPECIAL_DESCRIPTION`) so a Piper user sees the same vocabulary
 * here. Unknown values fall back to numeric forms so we don't lie
 * about what the firmware is actually reporting.
 */

import { nameForKeycode } from './keycode-map.js';
import { m } from './paraglide/messages.js';
import { BUTTON_ACTION_KIND, BUTTON_SPECIAL, MACRO_EVENT_KIND, SOFT_MACRO_KIND } from './types.js';
import type { ButtonAction, MacroStep, SoftMacro } from './types.js';

// Lookup tables map the wire value → a Paraglide message *function* (called
// lazily so each read resolves in the active locale). Insertion order of
// SPECIAL_NAMES is the source order for the SPECIAL_OPTIONS dropdown.

/** Conventional names for well-known hardware mouse buttons.
 *
 *  libratbag's logical button number is **1-indexed**: `ratbag_button_get_button`
 *  documents "buttons 1, 2 and 3 are mapped into left, middle, right". Values
 *  above 3 are up to the input stack; under the standard X/Wayland pointer
 *  convention 4–7 are scroll and 8/9 are back/forward. Anything unnamed falls
 *  back to "Mouse N" so we never mislabel a value we can't be sure about. */
const MOUSE_BUTTON_NAMES: ReadonlyMap<number, () => string> = new Map([
    [1, m.btn_mouse_left],
    [2, m.btn_mouse_middle],
    [3, m.btn_mouse_right],
    [8, m.btn_mouse_back],
    [9, m.btn_mouse_forward],
]);

/** Piper-equivalent labels for ratbagd's special-action enum. */
const SPECIAL_NAMES: ReadonlyMap<number, () => string> = new Map([
    [BUTTON_SPECIAL.UNKNOWN, m.btn_special_unknown],
    [BUTTON_SPECIAL.DOUBLECLICK, m.btn_special_doubleclick],
    [BUTTON_SPECIAL.WHEEL_LEFT, m.btn_special_wheel_left],
    [BUTTON_SPECIAL.WHEEL_RIGHT, m.btn_special_wheel_right],
    [BUTTON_SPECIAL.WHEEL_UP, m.btn_special_wheel_up],
    [BUTTON_SPECIAL.WHEEL_DOWN, m.btn_special_wheel_down],
    [BUTTON_SPECIAL.RATCHET_MODE_SWITCH, m.btn_special_ratchet],
    [BUTTON_SPECIAL.RESOLUTION_CYCLE_UP, m.btn_special_dpi_cycle_up],
    [BUTTON_SPECIAL.RESOLUTION_CYCLE_DOWN, m.btn_special_dpi_cycle_down],
    [BUTTON_SPECIAL.RESOLUTION_UP, m.btn_special_dpi_up],
    [BUTTON_SPECIAL.RESOLUTION_DOWN, m.btn_special_dpi_down],
    [BUTTON_SPECIAL.RESOLUTION_ALTERNATE, m.btn_special_dpi_alternate],
    [BUTTON_SPECIAL.RESOLUTION_DEFAULT, m.btn_special_dpi_default],
    [BUTTON_SPECIAL.PROFILE_CYCLE_UP, m.btn_special_profile_cycle_up],
    [BUTTON_SPECIAL.PROFILE_CYCLE_DOWN, m.btn_special_profile_cycle_down],
    [BUTTON_SPECIAL.PROFILE_UP, m.btn_special_profile_up],
    [BUTTON_SPECIAL.PROFILE_DOWN, m.btn_special_profile_down],
    [BUTTON_SPECIAL.SECOND_MODE, m.btn_special_second_mode],
    [BUTTON_SPECIAL.BATTERY_LEVEL, m.btn_special_battery],
]);

/** Localized name for a mouse button value, or `undefined` for values
 *  outside the well-known set (caller falls back to "Mouse N"). */
function mouseButtonName(value: number): string | undefined {
    return MOUSE_BUTTON_NAMES.get(value)?.();
}

/** Localized name for a special-action value, or `undefined` outside the
 *  known set (caller falls back to a hex form). */
function specialName(value: number): string | undefined {
    return SPECIAL_NAMES.get(value)?.();
}

// Keycode → friendly name lookup delegates to `keycode-map.ts`, the
// single source of truth that's shared with the KeyCapture /
// MacroRecorder components. Anything missing from the table falls
// back to "Key N" via `nameForKeycode` so the UI never lies about
// what value the firmware will see.

/** Render a single ButtonAction as a short label string. */
export function formatAction(action: ButtonAction): string {
    switch (action.kind) {
        case BUTTON_ACTION_KIND.NONE: {
            return m.btn_action_disabled();
        }
        case BUTTON_ACTION_KIND.MOUSE: {
            return mouseButtonName(action.value) ?? m.btn_action_mouse_n({ n: action.value });
        }
        case BUTTON_ACTION_KIND.SPECIAL: {
            return (
                specialName(action.value) ??
                m.btn_action_special_hex({ hex: action.value.toString(16) })
            );
        }
        case BUTTON_ACTION_KIND.KEY: {
            return nameForKeycode(action.value);
        }
        case BUTTON_ACTION_KIND.MACRO: {
            return action.macro_steps.length === 0
                ? m.btn_action_empty_macro()
                : m.btn_action_macro_steps({ count: action.macro_steps.length });
        }
        default: {
            return m.btn_action_kind_n({ n: action.kind });
        }
    }
}

/** Comma-join a keycode list into friendly names (e.g. `A, B, C`).
 *  Shared by the binding editor's warnings and the soft-macro label
 *  below so both render keycodes identically. */
export function describeKeys(keys: readonly number[]): string {
    return keys.map((k) => nameForKeycode(k)).join(', ');
}

/** Render a soft macro as a short button label. A sticky toggle shows
 *  the keys it emits — `Toggle · A, B` — so the leader label reflects
 *  the *soft* binding instead of the (deliberately `NONE`) firmware
 *  action underneath it. `DISABLED` macros are inert and never reach
 *  the label (they're filtered out at the `soft_macros` layer). */
export function formatSoftMacro(macro: SoftMacro): string {
    if (macro.kind === SOFT_MACRO_KIND.STICKY_TOGGLE) {
        return m.mv_button_toggle({ keys: describeKeys(macro.keys) });
    }
    return m.btn_action_disabled();
}

/** Long-form description for the editor popover header. */
export function describeAction(action: ButtonAction): string {
    switch (action.kind) {
        case BUTTON_ACTION_KIND.NONE: {
            return m.btn_describe_disabled();
        }
        case BUTTON_ACTION_KIND.MOUSE: {
            return m.btn_describe_mouse({ n: action.value });
        }
        case BUTTON_ACTION_KIND.SPECIAL: {
            const name = specialName(action.value) ?? `0x${action.value.toString(16)}`;
            return m.btn_describe_special({ name });
        }
        case BUTTON_ACTION_KIND.KEY: {
            return m.btn_describe_key({ n: action.value });
        }
        case BUTTON_ACTION_KIND.MACRO: {
            return m.btn_describe_macro({ count: action.macro_steps.length });
        }
        default: {
            return m.btn_describe_unknown({ n: action.kind });
        }
    }
}

/**
 * Display a macro step as `▼ A` / `▲ A` / `⏲ 25ms`. Symbolic
 * delimiter-friendly form rather than natural language so the
 * tooltip's sequence-of-steps stays compact and readable next to
 * the `→` joiner: `▼ A → ⏲ 25ms → ▲ A`.
 *
 * The triangles match what `MacroRecorder.svelte`'s live preview
 * shows during recording, so the user reads the same vocabulary
 * everywhere a macro is rendered.
 */
export function formatMacroStep(step: MacroStep): string {
    switch (step.kind) {
        case MACRO_EVENT_KIND.KEY_PRESS: {
            return `▼ ${nameForKeycode(step.value)}`;
        }
        case MACRO_EVENT_KIND.KEY_RELEASE: {
            return `▲ ${nameForKeycode(step.value)}`;
        }
        case MACRO_EVENT_KIND.WAIT: {
            return `⏲ ${String(step.value)}ms`;
        }
        case MACRO_EVENT_KIND.NONE: {
            return `· ${nameForKeycode(step.value)}`;
        }
        default: {
            return `? ${String(step.kind)}:${String(step.value)}`;
        }
    }
}

/** Friendly name for the action-kind enum, for selectors. */
export function kindName(kind: number): string {
    switch (kind) {
        case BUTTON_ACTION_KIND.NONE: {
            return m.btn_action_disabled();
        }
        case BUTTON_ACTION_KIND.MOUSE: {
            return m.btn_kind_mouse();
        }
        case BUTTON_ACTION_KIND.SPECIAL: {
            return m.btn_kind_special();
        }
        case BUTTON_ACTION_KIND.KEY: {
            return m.btn_kind_key();
        }
        case BUTTON_ACTION_KIND.MACRO: {
            return m.btn_kind_macro();
        }
        default: {
            return m.btn_action_kind_n({ n: kind });
        }
    }
}

/** All known specials, sorted by (localized) name, for the editor dropdown.
 *  Evaluated at module load in the active locale; a locale switch reloads
 *  the app, so this re-sorts under the new language. */
export const SPECIAL_OPTIONS: readonly { readonly value: number; readonly label: string }[] =
    [...SPECIAL_NAMES]
        .map(([value, label]) => ({ value, label: label() }))
        .toSorted((a, b) => a.label.localeCompare(b.label));
