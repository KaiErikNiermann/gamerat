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
}

/** One per-button binding inside a {@link GameratProfile}. */
export interface ProfileButton {
    readonly index: number;
    readonly action: ButtonAction;
}

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
export type Launcher = 'steam' | 'lutris' | 'heroic' | 'other';

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
