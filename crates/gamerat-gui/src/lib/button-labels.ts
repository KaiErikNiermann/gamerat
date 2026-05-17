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

import { BUTTON_ACTION_KIND, BUTTON_SPECIAL, MACRO_EVENT_KIND } from './types.js';
import type { ButtonAction, MacroStep } from './types.js';

/** Conventional names for the first five hardware mouse buttons. */
const MOUSE_BUTTON_NAMES: ReadonlyMap<number, string> = new Map([
    [0, 'Left'],
    [1, 'Middle'],
    [2, 'Right'],
    [3, 'Back'],
    [4, 'Forward'],
]);

/** Piper-equivalent labels for ratbagd's special-action enum. */
const SPECIAL_NAMES: ReadonlyMap<number, string> = new Map([
    [BUTTON_SPECIAL.UNKNOWN, 'Unknown'],
    [BUTTON_SPECIAL.DOUBLECLICK, 'Double click'],
    [BUTTON_SPECIAL.WHEEL_LEFT, 'Wheel left'],
    [BUTTON_SPECIAL.WHEEL_RIGHT, 'Wheel right'],
    [BUTTON_SPECIAL.WHEEL_UP, 'Wheel up'],
    [BUTTON_SPECIAL.WHEEL_DOWN, 'Wheel down'],
    [BUTTON_SPECIAL.RATCHET_MODE_SWITCH, 'Ratchet mode'],
    [BUTTON_SPECIAL.RESOLUTION_CYCLE_UP, 'DPI cycle up'],
    [BUTTON_SPECIAL.RESOLUTION_CYCLE_DOWN, 'DPI cycle down'],
    [BUTTON_SPECIAL.RESOLUTION_UP, 'DPI up'],
    [BUTTON_SPECIAL.RESOLUTION_DOWN, 'DPI down'],
    [BUTTON_SPECIAL.RESOLUTION_ALTERNATE, 'DPI alternate'],
    [BUTTON_SPECIAL.RESOLUTION_DEFAULT, 'DPI default'],
    [BUTTON_SPECIAL.PROFILE_CYCLE_UP, 'Profile cycle up'],
    [BUTTON_SPECIAL.PROFILE_CYCLE_DOWN, 'Profile cycle down'],
    [BUTTON_SPECIAL.PROFILE_UP, 'Profile up'],
    [BUTTON_SPECIAL.PROFILE_DOWN, 'Profile down'],
    [BUTTON_SPECIAL.SECOND_MODE, 'Second mode'],
    [BUTTON_SPECIAL.BATTERY_LEVEL, 'Battery level'],
]);

/** Linux keycodes we want to render with their canonical name. The
 *  table is intentionally small — Piper itself doesn't pretty-print
 *  every keycode either, and gamerat's editor will accept the raw
 *  number as fallback. */
const KEYCODE_NAMES: ReadonlyMap<number, string> = new Map([
    [1, 'Esc'],
    [14, 'Backspace'],
    [15, 'Tab'],
    [28, 'Enter'],
    [29, 'L Ctrl'],
    [42, 'L Shift'],
    [56, 'L Alt'],
    [57, 'Space'],
    [97, 'R Ctrl'],
    [100, 'R Alt'],
    [125, 'L Meta'],
    [103, '↑'],
    [108, '↓'],
    [105, '←'],
    [106, '→'],
]);

/** Render a single ButtonAction as a short label string. */
export function formatAction(action: ButtonAction): string {
    switch (action.kind) {
        case BUTTON_ACTION_KIND.NONE: {
            return 'Disabled';
        }
        case BUTTON_ACTION_KIND.MOUSE: {
            return MOUSE_BUTTON_NAMES.get(action.value) ?? `Mouse ${String(action.value)}`;
        }
        case BUTTON_ACTION_KIND.SPECIAL: {
            return (
                SPECIAL_NAMES.get(action.value) ??
                `Special ${action.value.toString(16)}`
            );
        }
        case BUTTON_ACTION_KIND.KEY: {
            const name = KEYCODE_NAMES.get(action.value);
            return name ?? `Key ${String(action.value)}`;
        }
        case BUTTON_ACTION_KIND.MACRO: {
            return action.macro_steps.length === 0
                ? 'Empty macro'
                : `Macro (${String(action.macro_steps.length)} steps)`;
        }
        default: {
            return `Kind ${String(action.kind)}`;
        }
    }
}

/** Long-form description for the editor popover header. */
export function describeAction(action: ButtonAction): string {
    switch (action.kind) {
        case BUTTON_ACTION_KIND.NONE: {
            return 'Disabled — pressing this button has no effect.';
        }
        case BUTTON_ACTION_KIND.MOUSE: {
            return `Mapped to hardware mouse button ${String(action.value)}.`;
        }
        case BUTTON_ACTION_KIND.SPECIAL: {
            const name = SPECIAL_NAMES.get(action.value) ?? `0x${action.value.toString(16)}`;
            return `Special: ${name}`;
        }
        case BUTTON_ACTION_KIND.KEY: {
            return `Keycode ${String(action.value)} — sends a single keypress.`;
        }
        case BUTTON_ACTION_KIND.MACRO: {
            return `Macro with ${String(action.macro_steps.length)} step(s).`;
        }
        default: {
            return `Unknown kind ${String(action.kind)}.`;
        }
    }
}

function macroStepKindLabel(kind: number): string {
    switch (kind) {
        case MACRO_EVENT_KIND.NONE: {
            return 'none';
        }
        case MACRO_EVENT_KIND.KEY_PRESS: {
            return 'press';
        }
        case MACRO_EVENT_KIND.KEY_RELEASE: {
            return 'release';
        }
        case MACRO_EVENT_KIND.WAIT: {
            return 'wait';
        }
        default: {
            return `k${String(kind)}`;
        }
    }
}

/** Display a macro step as "press: KEY_A" / "wait: 25ms" / etc. */
export function formatMacroStep(step: MacroStep): string {
    const kindLabel = macroStepKindLabel(step.kind);
    if (step.kind === MACRO_EVENT_KIND.WAIT) {
        return `${kindLabel}: ${String(step.value)}ms`;
    }
    const keyName = KEYCODE_NAMES.get(step.value);
    const keyText = keyName ?? `key ${String(step.value)}`;
    return `${kindLabel}: ${keyText}`;
}

/** Friendly name for the action-kind enum, for selectors. */
export function kindName(kind: number): string {
    switch (kind) {
        case BUTTON_ACTION_KIND.NONE: {
            return 'Disabled';
        }
        case BUTTON_ACTION_KIND.MOUSE: {
            return 'Mouse button';
        }
        case BUTTON_ACTION_KIND.SPECIAL: {
            return 'Special action';
        }
        case BUTTON_ACTION_KIND.KEY: {
            return 'Keyboard key';
        }
        case BUTTON_ACTION_KIND.MACRO: {
            return 'Macro';
        }
        default: {
            return `Kind ${String(kind)}`;
        }
    }
}

/** All known specials, sorted by name, for the editor dropdown. */
export const SPECIAL_OPTIONS: readonly { readonly value: number; readonly label: string }[] =
    [...SPECIAL_NAMES]
        .map(([value, label]) => ({ value, label }))
        .sort((a, b) => a.label.localeCompare(b.label));
