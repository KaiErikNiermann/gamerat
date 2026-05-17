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

import { formatAction } from './button-labels.js';
import type { RatbagButton } from './types.js';

/** Minimal subset of `LabelPos` the helpers actually use. Lets tests
 *  build label fixtures without committing to the geometry fields. */
export interface LabelRef {
    readonly buttonIndex: number | null;
    /** Static fallback text (`"B0"` / `"LED 0"`). */
    readonly text: string;
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
