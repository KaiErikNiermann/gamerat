/**
 * Thin async wrappers around Tauri `invoke` calls.
 *
 * Each function maps 1:1 to a command registered in `src-tauri/src/commands.rs`.
 * Errors from the Rust side arrive as plain strings (the daemon stringifies
 * D-Bus errors at the IPC boundary), so we propagate them as-is.
 */

import { invoke } from '@tauri-apps/api/core';
import { logInvokeError, logInvokeResult, logInvokeStart } from './dev-log.js';
import type {
    ButtonAction,
    DeviceInfo,
    GameEntry,
    GameratProfile,
    RatbagButton,
    RatbagdCompatInfo,
    Rule,
    SlotInfo,
    StatusInfo,
} from './types.js';

/**
 * Wrap `invoke()` with dev-log instrumentation. Records the call
 * start, the result (or error), and how long it took. Pure
 * pass-through otherwise — caller still sees a typed Promise.
 */
async function loggedInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    const argsObj = args ?? {};
    const startedAt = performance.now();
    logInvokeStart(command, argsObj);
    try {
        const result = await invoke<T>(command, argsObj);
        logInvokeResult(command, result, performance.now() - startedAt);
        return result;
    } catch (error) {
        logInvokeError(command, error, performance.now() - startedAt);
        throw error;
    }
}

export async function fetchStatus(): Promise<StatusInfo> {
    return loggedInvoke<StatusInfo>('status');
}

export async function fetchVersion(): Promise<string> {
    return loggedInvoke<string>('version');
}

export async function fetchRules(): Promise<Rule[]> {
    return loggedInvoke<Rule[]>('list_rules');
}

export async function addRule(appIdGlob: string, profileId: string): Promise<void> {
    await loggedInvoke<undefined>('set_rule', { appIdGlob, profileId });
}

export async function removeRule(appIdGlob: string): Promise<void> {
    await loggedInvoke<undefined>('delete_rule', { appIdGlob });
}

export async function fetchDevices(): Promise<DeviceInfo[]> {
    return loggedInvoke<DeviceInfo[]>('list_devices');
}

export async function fetchGames(): Promise<GameEntry[]> {
    return loggedInvoke<GameEntry[]>('list_games');
}

export async function fetchProfiles(): Promise<GameratProfile[]> {
    return loggedInvoke<GameratProfile[]>('list_profiles');
}

export async function upsertProfile(profile: GameratProfile): Promise<void> {
    await loggedInvoke<undefined>('set_profile', { profile });
}

export async function removeProfile(id: string): Promise<void> {
    await loggedInvoke<undefined>('delete_profile', { id });
}

export async function doSimulateFocus(appId: string, title: string): Promise<void> {
    await loggedInvoke<undefined>('simulate_focus', { appId, title });
}

export async function fetchRatbagdCompat(): Promise<RatbagdCompatInfo> {
    return loggedInvoke<RatbagdCompatInfo>('ratbagd_compat');
}

/**
 * `profileIndex === 0xFFFFFFFF` (`-1 >>> 0`) is the well-known
 * "currently active profile" sentinel — matches the daemon-side
 * `u32::MAX` convention.
 */
// eslint-disable-next-line unicorn/numeric-separators-style -- u32::MAX, no natural group split
export const PROFILE_INDEX_ACTIVE = 0xFFFFFFFF;

export async function fetchButtons(
    devicePath: string,
    profileIndex: number = PROFILE_INDEX_ACTIVE,
): Promise<RatbagButton[]> {
    return loggedInvoke<RatbagButton[]>('list_buttons', { devicePath, profileIndex });
}

export async function writeButton(
    devicePath: string,
    profileIndex: number,
    buttonIndex: number,
    action: ButtonAction,
): Promise<void> {
    await loggedInvoke<undefined>('set_button', {
        devicePath,
        profileIndex,
        buttonIndex,
        action,
    });
}

export async function fetchAutoswitch(): Promise<boolean> {
    return loggedInvoke<boolean>('get_autoswitch');
}

/** Force a saved profile onto the device, bypassing focus rules
 *  and the autoswitch flag. Mirrors the daemon's ApplyProfile. */
export async function applyProfile(id: string): Promise<void> {
    await loggedInvoke<undefined>('apply_profile', { id });
}

/** Hardware slot map for a device — which gamerat profile (if
 *  any) is materialised in each slot. */
export async function fetchSlotMap(devicePath: string): Promise<SlotInfo[]> {
    return loggedInvoke<SlotInfo[]>('get_slot_map', { devicePath });
}

/** Active DPI stage index on the device's currently-active hardware
 *  profile. Polled by MouseView so on-mouse DPI cycles propagate to
 *  the UI without requiring a profile re-select. */
export async function fetchActiveDpiStage(devicePath: string): Promise<number> {
    return loggedInvoke<number>('get_active_dpi_stage', { devicePath });
}

/** Force the device back to its reserved Desktop slot. Manual-mode
 *  Apply Base. */
export async function applyBase(): Promise<void> {
    await loggedInvoke<undefined>('apply_base');
}

export async function writeAutoswitch(value: boolean): Promise<boolean> {
    return loggedInvoke<boolean>('set_autoswitch', { value });
}
