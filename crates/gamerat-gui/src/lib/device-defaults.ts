/**
 * Per-device factory binding tables.
 *
 * Why this exists: ratbagd / libratbag deliberately don't expose
 * factory defaults (see libratbag issue #1302). HID++ has no
 * documented "load factory mappings" call either — the only
 * approximation is `hidpp20-reset.c`'s "zero every onboard sector
 * and hope the firmware re-initialises", which isn't a real
 * factory-restore.
 *
 * So we encode the actual factory bindings ourselves, sourced from
 * vendor setup PDFs + libratbag's per-device data files. Keyed by
 * the `bustype:vid:pid:version` model string ratbagd reports
 * (e.g. `"usb:046d:c08b:0"` for the G502 HERO).
 *
 * Unknown mice fall back to `genericMouseDefaults` — buttons 0-4 =
 * standard mouse buttons, the rest disabled. Not what most devices
 * actually ship with, but covers the universal subset.
 */

import { BUTTON_ACTION_KIND, BUTTON_SPECIAL } from './types.js';
import type { ButtonAction, ProfileButton } from './types.js';

/** Helper builders so the table reads as declarative data. */
const mouse = (value: number): ButtonAction => ({
    kind: BUTTON_ACTION_KIND.MOUSE,
    value,
    macro_steps: [],
});
const special = (value: number): ButtonAction => ({
    kind: BUTTON_ACTION_KIND.SPECIAL,
    value,
    macro_steps: [],
});
const disabled = (): ButtonAction => ({
    kind: BUTTON_ACTION_KIND.NONE,
    value: 0,
    macro_steps: [],
});

/**
 * One device's factory bindings. Maps button index → action. Any
 * index that's missing falls back to Disabled when materialised.
 */
type DeviceDefaults = readonly { index: number; action: ButtonAction }[];

/**
 * Logitech G502 HERO (`usb:046d:c08b:0`). 11 buttons.
 *
 * Sources:
 *   - Logitech's G502 HERO setup guide
 *   - libratbag/data/devices/logitech-g502-hero.device
 *   - libratbag issue threads on G502 button enumeration
 *
 * Indices map to physical buttons as follows (Logitech's "G1..G11"
 * naming):
 *
 *   0  G1   left click
 *   1  G2   right click
 *   2  G3   wheel click
 *   3  G4   thumb back
 *   4  G5   thumb forward
 *   5  G6   sniper (hold for DPI shift)
 *   6  G7   behind-wheel-up    → DPI down
 *   7  G8   behind-wheel-down  → DPI up
 *   8  G9   top near LEDs       → profile cycle
 *   9  G10  wheel-tilt-right    → wheel-right
 *  10  G11  wheel-tilt-left     → wheel-left
 */
const LOGITECH_G502_HERO: DeviceDefaults = [
    { index: 0, action: mouse(1) },
    { index: 1, action: mouse(2) },
    { index: 2, action: mouse(3) },
    { index: 3, action: mouse(4) },
    { index: 4, action: mouse(5) },
    { index: 5, action: special(BUTTON_SPECIAL.RESOLUTION_ALTERNATE) },
    { index: 6, action: special(BUTTON_SPECIAL.RESOLUTION_DOWN) },
    { index: 7, action: special(BUTTON_SPECIAL.RESOLUTION_UP) },
    { index: 8, action: special(BUTTON_SPECIAL.PROFILE_CYCLE_UP) },
    { index: 9, action: special(BUTTON_SPECIAL.WHEEL_RIGHT) },
    { index: 10, action: special(BUTTON_SPECIAL.WHEEL_LEFT) },
];

/**
 * Table keyed by ratbagd's model string (`bustype:vid:pid:version`).
 * The version segment is part of the key because firmware revisions
 * sometimes ship different defaults; we accept whatever ratbagd
 * tells us. Most users will only ever have `:0`.
 */
const DEVICE_TABLE: Readonly<Record<string, DeviceDefaults>> = {
    'usb:046d:c08b:0': LOGITECH_G502_HERO,
    // Add more entries as users surface them. Keep alphabetised by
    // VID then PID. Cite the source in a comment when you add one.
};

/**
 * Generic mouse defaults used when we don't have a per-device table:
 * buttons 0-4 = standard mouse 1-5 (left / right / middle / back /
 * forward), everything else Disabled. Imperfect but covers the
 * universal subset every mouse honours.
 */
function genericMouseDefaults(buttonIndices: readonly number[]): DeviceDefaults {
    return [...buttonIndices]
        .sort((a, b) => a - b)
        .map((index) => ({
            index,
            action: index >= 0 && index <= 4 ? mouse(index + 1) : disabled(),
        }));
}

/**
 * Resolve the factory bindings for a device.
 *
 * @param model        ratbagd's `bustype:vid:pid:version` model string
 *                     (from `DeviceInfo.model`).
 * @param buttonIndices  the full set of button indices the hardware
 *                       exposes — used to size the result. Any index
 *                       not covered by the per-device table gets
 *                       Disabled so we still emit a self-contained
 *                       profile.
 */
export function defaultBindingsFor(
    model: string,
    buttonIndices: readonly number[],
): readonly ProfileButton[] {
    // ratbagd-reported model string is a `bustype:vid:pid:version`
    // identifier, not user-typed input. The Record lookup returns
    // `undefined` for unknown keys (handled below) — no prototype
    // pollution surface.
    // eslint-disable-next-line security/detect-object-injection
    const entries = DEVICE_TABLE[model];
    if (entries === undefined) {
        return genericMouseDefaults(buttonIndices);
    }
    const byIndex = new Map<number, ButtonAction>();
    for (const e of entries) byIndex.set(e.index, e.action);
    return [...buttonIndices]
        .sort((a, b) => a - b)
        .map((index) => ({
            index,
            action: byIndex.get(index) ?? disabled(),
        }));
}

/** True if we have a per-device table for this model. Surfaces the
 *  distinction in the UI ("Reset to <device> defaults" vs the
 *  generic "Reset bindings"). */
export function hasDeviceDefaults(model: string): boolean {
    return Object.hasOwn(DEVICE_TABLE, model);
}
