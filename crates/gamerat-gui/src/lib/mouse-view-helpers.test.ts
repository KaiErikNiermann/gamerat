import { describe, expect, it } from 'vitest';

import { findBindingForLabel, liveLabelText } from './mouse-view-helpers.js';
import {
    BUTTON_ACTION_KIND,
    BUTTON_SPECIAL,
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
