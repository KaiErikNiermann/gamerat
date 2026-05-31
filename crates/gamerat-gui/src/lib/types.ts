/**
 * Wire types that mirror the Rust structs in `gamerat-proto::types`.
 *
 * Field names must match the serde-serialised JSON the Tauri commands
 * return. All structs use the default serde naming (snake_case), so the
 * TypeScript side matches.
 */

/** A focus rule: glob → profile id. */
export interface Rule {
    readonly app_id_glob: string;
    readonly profile_id: string;
    readonly created_unix: number;
}

/** Software profile (id, DPI stages, category, button bindings, etc.). */
export interface GameratProfile {
    readonly id: string;
    readonly name: string;
    readonly description: string;
    readonly category: string; // "agnostic" | "specific"
    readonly inherits_from: string;
    readonly dpi: readonly number[];
    readonly active_dpi_stage: number;
    readonly created_unix: number;
    /** Per-button bindings the profile declares. Self-contained:
     *  when the daemon materialises the profile, every entry here
     *  gets written to the matching hardware button. */
    readonly buttons: readonly ProfileButton[];
    /** Per-LED state the profile declares (color / mode / brightness).
     *  Same self-contained convention as `buttons`. */
    readonly leds: readonly ProfileLed[];
    /** Software-side button augmentations (currently: sticky toggles).
     *  Daemon rewrites the matching `buttons[i].action` to a
     *  trampoline `KEY` at apply time and runs the toggle state
     *  machine through `/dev/uinput`. */
    readonly soft_macros: readonly SoftMacro[];
}

/** Software-side augmentation for one button inside a
 *  {@link GameratProfile}. Mirrors `gamerat_proto::SoftMacro`. */
export interface SoftMacro {
    readonly button_index: number;
    /** One of {@link SOFT_MACRO_KIND} — `DISABLED` means inert. */
    readonly kind: SoftMacroKind;
    /** Linux keycode the firmware fires (`KEY_MACRO1..30`).
     *  Daemon-allocated; clients leave it `0` on creation and let the
     *  daemon assign on first apply. */
    readonly trampoline_keycode: number;
    /** Linux keycodes the toggle emits. For `STICKY_TOGGLE`, all of
     *  these go down together on odd presses, up on even presses. */
    readonly keys: readonly number[];
}

/** Wire-stable {@link SoftMacro} kinds. Mirrors
 *  `gamerat_proto::soft_macro_kind`. */
export const SOFT_MACRO_KIND = {
    DISABLED: 0,
    STICKY_TOGGLE: 1,
} as const;

export type SoftMacroKind = typeof SOFT_MACRO_KIND[keyof typeof SOFT_MACRO_KIND];

/** Wire-stable soft-input subsystem state. Mirrors
 *  `gamerat_daemon::soft_macros::soft_input_state`. */
export type SoftInputState = 'disabled' | 'active' | 'unavailable';

/** One per-button binding inside a {@link GameratProfile}. */
export interface ProfileButton {
    readonly index: number;
    readonly action: ButtonAction;
}

/** One per-LED state inside a {@link GameratProfile}. Mirrors
 *  `gamerat_proto::ProfileLed`. `color` is an RGB triple, each
 *  channel `0..=255`; `brightness` is `0..=255`. */
export interface ProfileLed {
    readonly index: number;
    readonly mode: LedMode;
    readonly color: readonly [number, number, number];
    readonly brightness: number;
}

/** One hardware LED + its current state. Mirrors `RatbagLed`. */
export interface RatbagLed {
    readonly index: number;
    readonly mode: LedMode;
    readonly color: readonly [number, number, number];
    readonly brightness: number;
    readonly supported_modes: readonly number[];
    readonly color_depth: LedColorDepth;
}

/** Wire-stable LED mode values. Mirrors `gamerat_proto::led_mode`. */
export const LED_MODE = {
    OFF: 0,
    ON: 1,
    CYCLE: 2,
    BREATHING: 3,
} as const;

export type LedMode = typeof LED_MODE[keyof typeof LED_MODE];

/** Wire-stable LED color-depth values. Mirrors
 *  `gamerat_proto::led_color_depth`. */
export const LED_COLOR_DEPTH = {
    MONOCHROME: 0,
    RGB_888: 1,
    RGB_111: 2,
} as const;

export type LedColorDepth = typeof LED_COLOR_DEPTH[keyof typeof LED_COLOR_DEPTH];

/** One row of the hardware slot map for a device — which gamerat
 *  profile (if any) currently occupies each slot. Returned by
 *  GetSlotMap. */
export interface SlotInfo {
    readonly index: number;
    readonly profile_id: string;
    readonly profile_name: string;
    readonly is_active: boolean;
    readonly is_desktop: boolean;
}

/** Snapshot of a ratbagd-managed device. */
export interface DeviceInfo {
    readonly object_path: string;
    readonly name: string;
    readonly model: string;
    readonly active_profile: number;
    readonly profile_count: number;
    /** DPI/resolution slot count per profile. Same for every profile
     *  on the device. Caps the GUI's "+ add stage" affordance. */
    readonly max_dpi_stages: number;
}

/** Wire-stable launcher tags from gamerat_proto::game_launcher. */
export type Launcher = 'steam' | 'lutris' | 'heroic' | 'other' | 'manual';

/** A game discovered by one of the launcher scanners. */
export interface GameEntry {
    readonly id: string;
    readonly name: string;
    readonly launcher: string; // one of Launcher at runtime, kept open at type level for forward-compat
    readonly install_dir: string;
    readonly executable: string;
    readonly app_id_hint: string;
}

/** One-shot status snapshot returned by the `status` command. */
export interface StatusInfo {
    readonly focused_app_id: string;
    readonly last_switch_reason: string;
    readonly rules_loaded: number;
}

/** Payload of the `focus-changed` Tauri event. */
export interface FocusChangedPayload {
    readonly app_id: string;
    readonly title: string;
    readonly source: string;
}

/** Payload of the `profile-switched` Tauri event. */
export interface ProfileSwitchedPayload {
    readonly device: string;
    readonly from_profile: number;
    readonly to_profile: number;
    readonly reason: string;
}

/** Payload of the `profile-switching` Tauri event — fires before the
 *  daemon writes to the device, so the GUI can flash a "switching…"
 *  indicator over the firmware-jitter window. */
export interface ProfileSwitchingPayload {
    readonly device: string;
    readonly to_profile: number;
    readonly reason: string;
}

/** Payload of the `active-dpi-stage-changed` Tauri event — fires
 *  when the daemon's DPI tracker observes a live cycle change on
 *  the device (DPI-up / DPI-down / DPI-cycle button press, or any
 *  explicit SetActive write). Requires the libratbag patch in
 *  patches/libratbag/. */
export interface ActiveDpiStageChangedPayload {
    readonly device: string;
    readonly stage: number;
}

/** A single entry in the signal-stream log. */
export type LogEntry =
    | { kind: 'focus'; ts: number; payload: FocusChangedPayload }
    | { kind: 'switch'; ts: number; payload: ProfileSwitchedPayload };

/**
 * Classification of ratbagd's `Manager.APIVersion` against the version
 * gamerat was tested against. Mirrors `RatbagdCompatInfo` in
 * `src-tauri/src/commands.rs`.
 */
export type RatbagdCompatKind =
    | 'exact'
    | 'known_compat'
    | 'below_min'
    | 'above_known'
    | 'unreachable';

export interface RatbagdCompatInfo {
    readonly kind: RatbagdCompatKind;
    readonly api_version: number | null;
    readonly expected: number;
    readonly warning: string | null;
}

/**
 * Health of the KDE focus bridge — the `gamerat-focus` KWin script the
 * daemon needs to observe window focus on Plasma. Mirrors the wire
 * strings in `gamerat_proto::focus_bridge`.
 *
 *  - `active`         — KDE session, script loaded; focus flows.
 *  - `not-loaded`     — KDE session, script not loaded; auto-switch is
 *                       inert. Surfaced as an actionable error.
 *  - `not-applicable` — non-KDE session (wlr / X11 / synthetic); hidden.
 *  - `unknown`        — couldn't probe KWin; shown muted.
 */
export type FocusBridgeState = 'active' | 'not-loaded' | 'not-applicable' | 'unknown';

/**
 * Result of the daemon's `PanicHatch` IPC. Mirrors the
 * `PanicHatchResult` struct in `src-tauri/src/commands.rs`.
 *
 *  - `released_keys` — Linux keycodes the daemon identified as stuck
 *    (`KEY_PRESS` without matching `KEY_RELEASE`). Format for display
 *    via {@link nameForKeycode} in `keycode-map.ts`.
 *  - `awaiting_press` — `true` iff the daemon armed a 5s auto-disable
 *    timer and the user should press the affected button once to fire
 *    the release-only macro. `false` means the daemon went straight
 *    to `NONE` (no stuck keys to release).
 */
export interface PanicHatchResult {
    readonly released_keys: readonly number[];
    readonly awaiting_press: boolean;
}

/**
 * Payload of the `panic-hatch-settled` Tauri event — the daemon's
 * auto-disable timer fired, was cancelled, or was superseded by an
 * unrelated rebind in the meantime.
 */
export interface PanicHatchSettledPayload {
    readonly device: string;
    readonly button: number;
    readonly outcome: 'timeout_disabled' | 'cancelled' | 'superseded';
}

// ─────────────────────────────────────────────────────────────────────
// Button bindings
// ─────────────────────────────────────────────────────────────────────

/**
 * Wire-stable action kinds. Mirrors `gamerat_proto::button_action_kind`
 * and libratbag's `RATBAG_BUTTON_ACTION_TYPE_*`.
 */
export const BUTTON_ACTION_KIND = {
    NONE: 0,
    MOUSE: 1,
    SPECIAL: 2,
    KEY: 3,
    MACRO: 4,
} as const;

export type ButtonActionKind = typeof BUTTON_ACTION_KIND[keyof typeof BUTTON_ACTION_KIND];

/**
 * Special action enum. All values are `(1 << 30) + N`. Mirrors
 * Piper's `RatbagdButton.ActionSpecial`. Append-only.
 */
export const BUTTON_SPECIAL = {
    BASE: 1 << 30,
    UNKNOWN: 1 << 30,
    DOUBLECLICK: (1 << 30) + 1,
    WHEEL_LEFT: (1 << 30) + 2,
    WHEEL_RIGHT: (1 << 30) + 3,
    WHEEL_UP: (1 << 30) + 4,
    WHEEL_DOWN: (1 << 30) + 5,
    RATCHET_MODE_SWITCH: (1 << 30) + 6,
    RESOLUTION_CYCLE_UP: (1 << 30) + 7,
    RESOLUTION_CYCLE_DOWN: (1 << 30) + 8,
    RESOLUTION_UP: (1 << 30) + 9,
    RESOLUTION_DOWN: (1 << 30) + 10,
    RESOLUTION_ALTERNATE: (1 << 30) + 11,
    RESOLUTION_DEFAULT: (1 << 30) + 12,
    PROFILE_CYCLE_UP: (1 << 30) + 13,
    PROFILE_CYCLE_DOWN: (1 << 30) + 14,
    PROFILE_UP: (1 << 30) + 15,
    PROFILE_DOWN: (1 << 30) + 16,
    SECOND_MODE: (1 << 30) + 17,
    BATTERY_LEVEL: (1 << 30) + 18,
} as const;

export const MACRO_EVENT_KIND = {
    NONE: 0,
    KEY_PRESS: 1,
    KEY_RELEASE: 2,
    WAIT: 3,
} as const;

/** One step in a recorded macro. Mirrors `MacroStep` over D-Bus. */
export interface MacroStep {
    readonly kind: number;   // one of MACRO_EVENT_KIND
    readonly value: number;  // Linux keycode for press/release; ms for wait
}

/** Flat button action shape mirroring `gamerat_proto::ButtonAction`. */
export interface ButtonAction {
    readonly kind: ButtonActionKind;
    readonly value: number;
    readonly macro_steps: readonly MacroStep[];
}

/** One hardware button + its current binding. Mirrors `RatbagButton`. */
export interface RatbagButton {
    readonly index: number;
    readonly action: ButtonAction;
    readonly supported_action_types: readonly number[];
}
