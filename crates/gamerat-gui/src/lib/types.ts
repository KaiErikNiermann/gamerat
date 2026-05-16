/**
 * Wire types that mirror the Rust structs in `gamerat-proto::types`.
 *
 * Field names must match the serde-serialised JSON the Tauri commands
 * return. All structs use the default serde naming (snake_case), so the
 * TypeScript side matches.
 */

/** A focus rule: glob → profile index. */
export interface Rule {
    readonly app_id_glob: string;
    readonly profile_index: number;
    readonly created_unix: number;
}

/** Snapshot of a ratbagd-managed device. */
export interface DeviceInfo {
    readonly object_path: string;
    readonly name: string;
    readonly model: string;
    readonly active_profile: number;
    readonly profile_count: number;
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

/** A single entry in the signal-stream log. */
export type LogEntry =
    | { kind: 'focus'; ts: number; payload: FocusChangedPayload }
    | { kind: 'switch'; ts: number; payload: ProfileSwitchedPayload };
