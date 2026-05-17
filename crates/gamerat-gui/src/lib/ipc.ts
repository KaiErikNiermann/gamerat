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
    DeviceInfo,
    GameEntry,
    GameratProfile,
    RatbagdCompatInfo,
    Rule,
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
