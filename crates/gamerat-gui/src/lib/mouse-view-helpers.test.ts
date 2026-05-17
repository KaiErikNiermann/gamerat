import { describe, expect, it } from 'vitest';

import { findBindingForLabel, labelTooltip, liveLabelText } from './mouse-view-helpers.js';
import {
    BUTTON_ACTION_KIND,
    BUTTON_SPECIAL,
    MACRO_EVENT_KIND,
    type ButtonAction,
    type RatbagButton,
} from './types.js';

function button(index: number, action: ButtonAction): RatbagButton {
    return { index, action, supported_action_types: [] };
}

function action(kind: ButtonAction['kind'], value = 0): ButtonAction {
    return { kind, value, macro_steps: [] };
}

describe('liveLabelText', () => {
    it('returns the static fallback for non-button labels (LEDs, chassis…)', () => {
        expect(liveLabelText({ buttonIndex: null, text: 'LED 0' }, [])).toBe('LED 0');
        expect(liveLabelText({ buttonIndex: null, text: 'chassis' }, [])).toBe('chassis');
    });

    it('returns the static fallback when bindings haven\'t loaded yet', () => {
        expect(liveLabelText({ buttonIndex: 3, text: 'B3' }, [])).toBe('B3');
    });

    it('returns the static fallback when the button index is missing from the binding list', () => {
        // bindings present for button 1, label asks about button 3
        const buttons = [button(1, action(BUTTON_ACTION_KIND.NONE))];
        expect(liveLabelText({ buttonIndex: 3, text: 'B3' }, buttons)).toBe('B3');
    });

    it('renders MOUSE-mapped buttons with the well-known name', () => {
        const buttons = [button(0, action(BUTTON_ACTION_KIND.MOUSE, 0))];
        expect(liveLabelText({ buttonIndex: 0, text: 'B0' }, buttons)).toBe('Left');
    });

    it('renders SPECIAL bindings with their Piper-style label', () => {
        const buttons = [
            button(5, action(BUTTON_ACTION_KIND.SPECIAL, BUTTON_SPECIAL.WHEEL_DOWN)),
        ];
        expect(liveLabelText({ buttonIndex: 5, text: 'B5' }, buttons)).toBe('Wheel down');
    });

    it('renders KEY bindings with the keycode-map name', () => {
        // 30 = KEY_A
        const buttons = [button(2, action(BUTTON_ACTION_KIND.KEY, 30))];
        expect(liveLabelText({ buttonIndex: 2, text: 'B2' }, buttons)).toBe('A');
    });

    it('summarises macros by step count', () => {
        const buttons: RatbagButton[] = [
            {
                index: 4,
                action: {
                    kind: BUTTON_ACTION_KIND.MACRO,
                    value: 0,
                    macro_steps: [
                        { kind: 1, value: 30 },
                        { kind: 3, value: 25 },
                        { kind: 2, value: 30 },
                    ],
                },
                supported_action_types: [],
            },
        ];
        expect(liveLabelText({ buttonIndex: 4, text: 'B4' }, buttons)).toBe(
            'Macro (3 steps)',
        );
    });
});

describe('findBindingForLabel', () => {
    it('returns null for non-button labels — caller must not open the editor', () => {
        expect(findBindingForLabel({ buttonIndex: null, text: 'LED 0' }, [])).toBeNull();
    });

    it('returns null when bindings haven\'t loaded — caller must not open the editor against an empty default', () => {
        expect(findBindingForLabel({ buttonIndex: 3, text: 'B3' }, [])).toBeNull();
    });

    it('returns null when the button index isn\'t in the binding list', () => {
        const buttons = [button(1, action(BUTTON_ACTION_KIND.NONE))];
        expect(findBindingForLabel({ buttonIndex: 3, text: 'B3' }, buttons)).toBeNull();
    });

    it('returns the matching binding when present', () => {
        const target = button(3, action(BUTTON_ACTION_KIND.MOUSE, 3));
        const buttons = [button(0, action(BUTTON_ACTION_KIND.NONE)), target];
        expect(findBindingForLabel({ buttonIndex: 3, text: 'B3' }, buttons)).toBe(target);
    });
});

describe('labelTooltip', () => {
    it('falls back to the SVG id for non-button labels', () => {
        expect(labelTooltip({ buttonIndex: null, text: 'LED 0', id: 'led0' }, [])).toBe('led0');
    });

    it('falls back to the static text when no id is set', () => {
        expect(labelTooltip({ buttonIndex: null, text: 'chassis' }, [])).toBe('chassis');
    });

    it('returns only the edit hint when no binding is known yet', () => {
        expect(labelTooltip({ buttonIndex: 3, text: 'B3' }, [])).toBe(
            'Click to edit binding for button 3',
        );
    });

    it('returns only the edit hint for non-macro bindings', () => {
        // We deliberately don't echo the on-screen label in the
        // tooltip for simple bindings — the label itself already
        // says e.g. "Wheel down" so the tooltip would just repeat
        // the visible text. The hint is the value-add.
        const buttons = [
            button(5, action(BUTTON_ACTION_KIND.SPECIAL, BUTTON_SPECIAL.WHEEL_DOWN)),
        ];
        expect(labelTooltip({ buttonIndex: 5, text: 'B5' }, buttons)).toBe(
            'Click to edit binding for button 5',
        );
    });

    it('expands macro bindings into the full step sequence', () => {
        const buttons: RatbagButton[] = [
            {
                index: 4,
                action: {
                    kind: BUTTON_ACTION_KIND.MACRO,
                    value: 0,
                    macro_steps: [
                        { kind: MACRO_EVENT_KIND.KEY_PRESS, value: 30 },
                        { kind: MACRO_EVENT_KIND.WAIT, value: 25 },
                        { kind: MACRO_EVENT_KIND.KEY_RELEASE, value: 30 },
                    ],
                },
                supported_action_types: [],
            },
        ];
        const tooltip = labelTooltip({ buttonIndex: 4, text: 'B4' }, buttons);
        // The sequence rendered above the hint, separated by a
        // newline so browser tooltips show both lines. Symbols
        // match MacroRecorder's live preview vocabulary.
        expect(tooltip).toBe('▼ A → ⏲ 25ms → ▲ A\nClick to edit binding for button 4');
    });

    it('labels empty macros explicitly', () => {
        const buttons: RatbagButton[] = [
            {
                index: 4,
                action: {
                    kind: BUTTON_ACTION_KIND.MACRO,
                    value: 0,
                    macro_steps: [],
                },
                supported_action_types: [],
            },
        ];
        expect(labelTooltip({ buttonIndex: 4, text: 'B4' }, buttons)).toBe(
            'Empty macro\nClick to edit binding for button 4',
        );
    });
});
