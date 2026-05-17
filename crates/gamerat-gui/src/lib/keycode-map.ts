/**
 * Bidirectional mapping between `KeyboardEvent.code` (the browser's
 * layout-independent physical-key id) and Linux input-event codes
 * (`KEY_*` from `linux/input-event-codes.h`).
 *
 * The browser's `code` is the right thing to pivot on here: it's
 * stable across keyboard layouts ("KeyA" is the physical A-position
 * key on US, Dvorak, AZERTY, all of them), which matches the way
 * ratbagd / libratbag thinks about keys. Using `event.key` would
 * give us the *character* the user gets — useful for text fields,
 * misleading for hardware bindings.
 *
 * The table covers ~110 keys: letters, digits, F1-F24, arrows,
 * modifiers, numpad, common punctuation, navigation, and a handful
 * of media keys. Anything outside the table falls back to the raw
 * numeric keycode in the UI — the same fallback Piper uses for
 * exotic keys.
 */

/** One mapping entry. `name` is the short label we show in the UI. */
interface KeyEntry {
    readonly code: string;
    readonly keycode: number;
    readonly name: string;
}

// Source of truth — the two lookup tables below derive from this.
// Cross-reference: `linux/input-event-codes.h`.
const KEY_ENTRIES: readonly KeyEntry[] = [
    // ─── Letters ─────────────────────────────────────────────────
    { code: 'KeyA', keycode: 30, name: 'A' },
    { code: 'KeyB', keycode: 48, name: 'B' },
    { code: 'KeyC', keycode: 46, name: 'C' },
    { code: 'KeyD', keycode: 32, name: 'D' },
    { code: 'KeyE', keycode: 18, name: 'E' },
    { code: 'KeyF', keycode: 33, name: 'F' },
    { code: 'KeyG', keycode: 34, name: 'G' },
    { code: 'KeyH', keycode: 35, name: 'H' },
    { code: 'KeyI', keycode: 23, name: 'I' },
    { code: 'KeyJ', keycode: 36, name: 'J' },
    { code: 'KeyK', keycode: 37, name: 'K' },
    { code: 'KeyL', keycode: 38, name: 'L' },
    { code: 'KeyM', keycode: 50, name: 'M' },
    { code: 'KeyN', keycode: 49, name: 'N' },
    { code: 'KeyO', keycode: 24, name: 'O' },
    { code: 'KeyP', keycode: 25, name: 'P' },
    { code: 'KeyQ', keycode: 16, name: 'Q' },
    { code: 'KeyR', keycode: 19, name: 'R' },
    { code: 'KeyS', keycode: 31, name: 'S' },
    { code: 'KeyT', keycode: 20, name: 'T' },
    { code: 'KeyU', keycode: 22, name: 'U' },
    { code: 'KeyV', keycode: 47, name: 'V' },
    { code: 'KeyW', keycode: 17, name: 'W' },
    { code: 'KeyX', keycode: 45, name: 'X' },
    { code: 'KeyY', keycode: 21, name: 'Y' },
    { code: 'KeyZ', keycode: 44, name: 'Z' },

    // ─── Top-row digits ──────────────────────────────────────────
    { code: 'Digit1', keycode: 2, name: '1' },
    { code: 'Digit2', keycode: 3, name: '2' },
    { code: 'Digit3', keycode: 4, name: '3' },
    { code: 'Digit4', keycode: 5, name: '4' },
    { code: 'Digit5', keycode: 6, name: '5' },
    { code: 'Digit6', keycode: 7, name: '6' },
    { code: 'Digit7', keycode: 8, name: '7' },
    { code: 'Digit8', keycode: 9, name: '8' },
    { code: 'Digit9', keycode: 10, name: '9' },
    { code: 'Digit0', keycode: 11, name: '0' },

    // ─── F-keys ─────────────────────────────────────────────────
    { code: 'F1', keycode: 59, name: 'F1' },
    { code: 'F2', keycode: 60, name: 'F2' },
    { code: 'F3', keycode: 61, name: 'F3' },
    { code: 'F4', keycode: 62, name: 'F4' },
    { code: 'F5', keycode: 63, name: 'F5' },
    { code: 'F6', keycode: 64, name: 'F6' },
    { code: 'F7', keycode: 65, name: 'F7' },
    { code: 'F8', keycode: 66, name: 'F8' },
    { code: 'F9', keycode: 67, name: 'F9' },
    { code: 'F10', keycode: 68, name: 'F10' },
    { code: 'F11', keycode: 87, name: 'F11' },
    { code: 'F12', keycode: 88, name: 'F12' },
    { code: 'F13', keycode: 183, name: 'F13' },
    { code: 'F14', keycode: 184, name: 'F14' },
    { code: 'F15', keycode: 185, name: 'F15' },
    { code: 'F16', keycode: 186, name: 'F16' },
    { code: 'F17', keycode: 187, name: 'F17' },
    { code: 'F18', keycode: 188, name: 'F18' },
    { code: 'F19', keycode: 189, name: 'F19' },
    { code: 'F20', keycode: 190, name: 'F20' },
    { code: 'F21', keycode: 191, name: 'F21' },
    { code: 'F22', keycode: 192, name: 'F22' },
    { code: 'F23', keycode: 193, name: 'F23' },
    { code: 'F24', keycode: 194, name: 'F24' },

    // ─── Arrow keys ─────────────────────────────────────────────
    // Up / Down use filled triangles instead of line-arrows — at the
    // 0.72rem label font, ↑/↓ have too little ink and are hard to
    // distinguish from each other. ▲▼ are solid glyphs that read
    // clearly at that size. Left / Right keep the line-arrows
    // because the horizontal versions are wider and unambiguous.
    { code: 'ArrowUp', keycode: 103, name: '▲' },
    { code: 'ArrowLeft', keycode: 105, name: '←' },
    { code: 'ArrowRight', keycode: 106, name: '→' },
    { code: 'ArrowDown', keycode: 108, name: '▼' },

    // ─── Modifiers ──────────────────────────────────────────────
    { code: 'ShiftLeft', keycode: 42, name: 'L Shift' },
    { code: 'ShiftRight', keycode: 54, name: 'R Shift' },
    { code: 'ControlLeft', keycode: 29, name: 'L Ctrl' },
    { code: 'ControlRight', keycode: 97, name: 'R Ctrl' },
    { code: 'AltLeft', keycode: 56, name: 'L Alt' },
    { code: 'AltRight', keycode: 100, name: 'R Alt' },
    { code: 'MetaLeft', keycode: 125, name: 'L Meta' },
    { code: 'MetaRight', keycode: 126, name: 'R Meta' },
    { code: 'CapsLock', keycode: 58, name: 'Caps Lock' },

    // ─── Common typing keys ─────────────────────────────────────
    { code: 'Escape', keycode: 1, name: 'Esc' },
    { code: 'Tab', keycode: 15, name: 'Tab' },
    { code: 'Enter', keycode: 28, name: 'Enter' },
    { code: 'Backspace', keycode: 14, name: 'Backspace' },
    { code: 'Space', keycode: 57, name: 'Space' },
    { code: 'ContextMenu', keycode: 127, name: 'Menu' },

    // ─── Navigation cluster ─────────────────────────────────────
    { code: 'Insert', keycode: 110, name: 'Insert' },
    { code: 'Delete', keycode: 111, name: 'Delete' },
    { code: 'Home', keycode: 102, name: 'Home' },
    { code: 'End', keycode: 107, name: 'End' },
    { code: 'PageUp', keycode: 104, name: 'Page Up' },
    { code: 'PageDown', keycode: 109, name: 'Page Down' },
    { code: 'PrintScreen', keycode: 99, name: 'Print' },
    { code: 'ScrollLock', keycode: 70, name: 'Scroll Lock' },
    { code: 'Pause', keycode: 119, name: 'Pause' },

    // ─── Punctuation ────────────────────────────────────────────
    { code: 'Minus', keycode: 12, name: '-' },
    { code: 'Equal', keycode: 13, name: '=' },
    { code: 'BracketLeft', keycode: 26, name: '[' },
    { code: 'BracketRight', keycode: 27, name: ']' },
    { code: 'Backslash', keycode: 43, name: '\\' },
    { code: 'Semicolon', keycode: 39, name: ';' },
    { code: 'Quote', keycode: 40, name: "'" },
    { code: 'Backquote', keycode: 41, name: '`' },
    { code: 'Comma', keycode: 51, name: ',' },
    { code: 'Period', keycode: 52, name: '.' },
    { code: 'Slash', keycode: 53, name: '/' },
    { code: 'IntlBackslash', keycode: 86, name: 'Intl \\' },

    // ─── Numpad ─────────────────────────────────────────────────
    { code: 'NumLock', keycode: 69, name: 'Num Lock' },
    { code: 'NumpadDivide', keycode: 98, name: 'Num /' },
    { code: 'NumpadMultiply', keycode: 55, name: 'Num *' },
    { code: 'NumpadSubtract', keycode: 74, name: 'Num -' },
    { code: 'NumpadAdd', keycode: 78, name: 'Num +' },
    { code: 'NumpadEnter', keycode: 96, name: 'Num Enter' },
    { code: 'NumpadDecimal', keycode: 83, name: 'Num .' },
    { code: 'Numpad0', keycode: 82, name: 'Num 0' },
    { code: 'Numpad1', keycode: 79, name: 'Num 1' },
    { code: 'Numpad2', keycode: 80, name: 'Num 2' },
    { code: 'Numpad3', keycode: 81, name: 'Num 3' },
    { code: 'Numpad4', keycode: 75, name: 'Num 4' },
    { code: 'Numpad5', keycode: 76, name: 'Num 5' },
    { code: 'Numpad6', keycode: 77, name: 'Num 6' },
    { code: 'Numpad7', keycode: 71, name: 'Num 7' },
    { code: 'Numpad8', keycode: 72, name: 'Num 8' },
    { code: 'Numpad9', keycode: 73, name: 'Num 9' },

    // ─── Media / browser ────────────────────────────────────────
    { code: 'AudioVolumeUp', keycode: 115, name: 'Vol +' },
    { code: 'AudioVolumeDown', keycode: 114, name: 'Vol -' },
    { code: 'AudioVolumeMute', keycode: 113, name: 'Mute' },
    { code: 'MediaPlayPause', keycode: 164, name: 'Play/Pause' },
    { code: 'MediaStop', keycode: 166, name: 'Media Stop' },
    { code: 'MediaTrackNext', keycode: 163, name: 'Next' },
    { code: 'MediaTrackPrevious', keycode: 165, name: 'Previous' },
];

// Build O(1) lookup maps from the source-of-truth array.
const KEYCODE_BY_CODE: ReadonlyMap<string, number> = new Map(
    KEY_ENTRIES.map(({ code, keycode }) => [code, keycode]),
);

const NAME_BY_KEYCODE: ReadonlyMap<number, string> = new Map(
    KEY_ENTRIES.map(({ keycode, name }) => [keycode, name]),
);

/**
 * Translate a `KeyboardEvent.code` value to its Linux input-event
 * keycode, or `null` if we don't have a mapping. Callers should fall
 * back to a manual entry field when this returns null.
 */
export function keycodeFromBrowserCode(code: string): number | null {
    return KEYCODE_BY_CODE.get(code) ?? null;
}

/**
 * Friendly short name for a Linux keycode. Falls back to a numeric
 * `Key N` form so we never lie about what value the firmware will
 * see.
 */
export function nameForKeycode(keycode: number): string {
    return NAME_BY_KEYCODE.get(keycode) ?? `Key ${String(keycode)}`;
}

/**
 * The full set of known entries, sorted by name, for search /
 * dropdown UI. Read-only so callers can't mutate the source.
 */
export const KEY_OPTIONS: readonly KeyEntry[] = [...KEY_ENTRIES].sort((a, b) =>
    a.name.localeCompare(b.name),
);

/**
 * Convenience: every known Linux keycode in numeric order. Useful
 * for sanity checks / tests that want to verify the table is dense
 * in the ranges it covers.
 */
export const ALL_KNOWN_KEYCODES: readonly number[] = [
    ...new Set(KEY_ENTRIES.map((e) => e.keycode)),
].sort((a, b) => a - b);
