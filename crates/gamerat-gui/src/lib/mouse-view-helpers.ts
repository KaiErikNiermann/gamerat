/**
 * Pure helpers extracted from `MouseView.svelte` so the
 * label-rendering and click-decision logic can be unit-tested
 * without spinning up the full component / Tauri runtime.
 *
 * The label rendering on the mouse diagram has three branches that
 * routinely trip people up:
 *
 *   - The label refers to a non-button SVG element (an LED, the
 *     chassis, etc.). It can't be rebound; show the static label
 *     text and disable the click target.
 *   - The label refers to a button but the daemon hasn't returned
 *     `list_buttons` yet. Fall back to the static text rather than
 *     opening an editor against a synthesised default.
 *   - The label refers to a button we have a binding for. Show the
 *     human-readable action and let clicks open the editor.
 *
 * Keeping the decision in pure functions lets `mouse-view-helpers.test.ts`
 * catch regressions in any of these branches.
 */

import { formatAction, formatMacroStep } from './button-labels.js';
import { BUTTON_ACTION_KIND } from './types.js';
import type { RatbagButton } from './types.js';

/** Minimal subset of `LabelPos` the helpers actually use. Lets tests
 *  build label fixtures without committing to the geometry fields. */
export interface LabelRef {
    readonly buttonIndex: number | null;
    /** Static fallback text (`"B0"` / `"LED 0"`). */
    readonly text: string;
    /** Optional SVG-id of the leader element (e.g. `"led1"`). Used
     *  for the tooltip on non-button labels. */
    readonly id?: string;
}

/**
 * Resolve the text shown on a leader label. Falls back to the static
 * text when no live binding is available (no buttonIndex, or the
 * daemon hasn't returned `list_buttons` yet).
 */
export function liveLabelText(label: LabelRef, buttons: readonly RatbagButton[]): string {
    if (label.buttonIndex === null) return label.text;
    const binding = buttons.find((b) => b.index === label.buttonIndex);
    if (binding === undefined) return label.text;
    return formatAction(binding.action);
}

/**
 * Resolve the live binding the editor should be opened against when
 * the user clicks a label. Returns `null` for non-button labels and
 * for buttons we don't have a binding for yet — both cases the
 * caller should treat as "no-op click", NOT "open editor against an
 * empty action".
 */
export function findBindingForLabel(
    label: LabelRef,
    buttons: readonly RatbagButton[],
): RatbagButton | null {
    if (label.buttonIndex === null) return null;
    return buttons.find((b) => b.index === label.buttonIndex) ?? null;
}

/**
 * Hover-tooltip copy for a leader label. Mirrors the on-screen
 * label by default but expands macros into their full step sequence
 * (`press: A → wait: 25ms → release: A`) so the user can see what
 * the macro actually does without opening the editor. The edit
 * hint comes on a second line so both pieces of information are
 * always visible.
 */
export function labelTooltip(label: LabelRef, buttons: readonly RatbagButton[]): string {
    if (label.buttonIndex === null) {
        return label.id ?? label.text;
    }
    const binding = buttons.find((b) => b.index === label.buttonIndex);
    const hint = `Click to edit binding for button ${String(label.buttonIndex)}`;
    if (binding === undefined) return hint;
    if (binding.action.kind === BUTTON_ACTION_KIND.MACRO) {
        const steps = binding.action.macro_steps;
        if (steps.length === 0) {
            return `Empty macro\n${hint}`;
        }
        const sequence = steps.map((step) => formatMacroStep(step)).join(' → ');
        return `${sequence}\n${hint}`;
    }
    return hint;
}
