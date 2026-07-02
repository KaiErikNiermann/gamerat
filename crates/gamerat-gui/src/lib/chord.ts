/**
 * Keyboard-shortcut ("chord") helpers: the bridge between a legible
 * "modifier(s) + one key" shortcut and the macro wire form libratbag
 * actually stores.
 *
 * Why this exists: hidpp20 devices (most Logitech mice) can't store a
 * true multi-step macro — libratbag collapses any macro to a single
 * key + modifier flags. So the only "macro" that works there is a
 * modifier+key chord, which is exactly what a keyboard shortcut is.
 * We present those as `Ctrl + A` and store them as a canonical macro
 * (`{@link chordToSteps}`) that survives the collapse; genuine
 * multi-key sequences are left as granular macros.
 *
 * The modifier keycode set mirrors `MODIFIER_KEYCODES` in
 * `gamerat-ratbag/src/button.rs` and libratbag's own switch.
 */

import { nameForKeycode } from './keycode-map.js';
import { MACRO_EVENT_KIND } from './types.js';
import type { MacroStep } from './types.js';

/** evdev modifier keycodes offered in the shortcut builder, in display
 *  order (Ctrl, Shift, Alt, Super — left-hand variants). */
export const MODIFIER_KEYCODES: readonly number[] = [
    29, // KEY_LEFTCTRL
    42, // KEY_LEFTSHIFT
    56, // KEY_LEFTALT
    125, // KEY_LEFTMETA
];

/** Every keycode libratbag classifies as a modifier (left + right). */
const ALL_MODIFIERS: ReadonlySet<number> = new Set([29, 42, 56, 125, 97, 54, 100, 126]);

export function isModifierKeycode(keycode: number): boolean {
    return ALL_MODIFIERS.has(keycode);
}

/** Order-independent fingerprint of a keycode list, for multiset compare. */
function sortedKeyList(list: readonly number[]): string {
    return list.toSorted((a, b) => a - b).join(',');
}

/** A keyboard shortcut: one regular key held with zero or more
 *  modifiers. */
export interface Chord {
    readonly key: number;
    /** Modifier keycodes in press order. */
    readonly modifiers: readonly number[];
}

/**
 * Parse a macro into a modifier chord, or `null` when it isn't one.
 *
 * A chord is: exactly one non-modifier key press, **at least one
 * modifier** press, and balanced presses/releases (waits ignored,
 * release order irrelevant). We require a modifier so a bare
 * single-key macro — which may encode a deliberate hold duration —
 * stays a granular macro rather than being flattened to a plain key.
 */
export function stepsToChord(steps: readonly MacroStep[]): Chord | null {
    let key: number | null = null;
    const modifiers: number[] = [];
    const pressed: number[] = [];
    const released: number[] = [];
    for (const step of steps) {
        if (step.kind === MACRO_EVENT_KIND.KEY_PRESS) {
            pressed.push(step.value);
            if (isModifierKeycode(step.value)) {
                modifiers.push(step.value);
            } else if (key === null) {
                key = step.value;
            } else {
                return null; // >1 regular key → genuine sequence
            }
        } else if (step.kind === MACRO_EVENT_KIND.KEY_RELEASE) {
            released.push(step.value);
        }
    }
    if (key === null || modifiers.length === 0) return null;
    // Balanced: identical multiset of pressed vs released keycodes.
    if (sortedKeyList(pressed) !== sortedKeyList(released)) return null;
    return { key, modifiers };
}

/**
 * Canonical macro for a chord: press modifiers, press the key, release
 * the key, release modifiers (reverse). Releasing the regular key
 * *before* its modifiers is what lets libratbag's macro→key collapse
 * keep the modifiers (see `normalize_chord_release_order` in
 * `gamerat-ratbag`); building it this way means it's already in that
 * form on the wire.
 */
export function chordToSteps(chord: Chord): MacroStep[] {
    return [
        ...chord.modifiers.map((value) => ({ kind: MACRO_EVENT_KIND.KEY_PRESS, value })),
        { kind: MACRO_EVENT_KIND.KEY_PRESS, value: chord.key },
        { kind: MACRO_EVENT_KIND.KEY_RELEASE, value: chord.key },
        ...chord.modifiers
            .toReversed()
            .map((value) => ({ kind: MACRO_EVENT_KIND.KEY_RELEASE, value })),
    ];
}

/** `L Alt + A` style label: modifiers first (press order), key last. */
export function formatChord(chord: Chord): string {
    return [...chord.modifiers, chord.key].map((k) => nameForKeycode(k)).join(' + ');
}

/** Count non-modifier key presses — used to warn when a macro has more
 *  than one and so can't survive the hidpp20 collapse. */
export function regularKeyPressCount(steps: readonly MacroStep[]): number {
    return steps.filter(
        (s) => s.kind === MACRO_EVENT_KIND.KEY_PRESS && !isModifierKeycode(s.value),
    ).length;
}

/**
 * Append a `KEY_RELEASE` for every key still held at the end of
 * `steps`, in reverse press order (LIFO).
 *
 * The live recorder depends on the webview delivering a `keyup` for
 * every key. On WebKitGTK a release is sometimes dropped — especially
 * the *second* of two keys released with a gap between them — leaving a
 * modifier "stuck down" in the recording (`press Shift, press A,
 * release A` with no `release Shift`). Balancing when recording stops
 * makes the captured macro well-formed regardless: a key that's still
 * down when the user clicks Stop is released either way (you can't hold
 * a key *into* a saved macro), so this is always the correct result.
 */
export function balanceMacroReleases(steps: readonly MacroStep[]): MacroStep[] {
    const held: number[] = [];
    for (const step of steps) {
        if (step.kind === MACRO_EVENT_KIND.KEY_PRESS) {
            if (!held.includes(step.value)) held.push(step.value);
        } else if (step.kind === MACRO_EVENT_KIND.KEY_RELEASE) {
            const at = held.indexOf(step.value);
            if (at !== -1) held.splice(at, 1);
        }
    }
    if (held.length === 0) return [...steps];
    return [
        ...steps,
        ...held.toReversed().map((value) => ({ kind: MACRO_EVENT_KIND.KEY_RELEASE, value })),
    ];
}
