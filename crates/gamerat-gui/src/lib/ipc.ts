/**
 * Thin async wrappers around Tauri `invoke` calls.
 *
 * Each function maps 1:1 to a command registered in `src-tauri/src/commands.rs`.
 * Errors from the Rust side arrive as plain strings (the daemon stringifies
 * D-Bus errors at the IPC boundary), so we propagate them as-is.
 */

import { invoke } from '@tauri-apps/api/core';
import type { DeviceInfo, Rule, StatusInfo } from './types.js';

export async function fetchStatus(): Promise<StatusInfo> {
    return invoke<StatusInfo>('status');
}

export async function fetchVersion(): Promise<string> {
    return invoke<string>('version');
}

export async function fetchRules(): Promise<Rule[]> {
    return invoke<Rule[]>('list_rules');
}

export async function addRule(appIdGlob: string, profileIndex: number): Promise<void> {
    await invoke<undefined>('set_rule', { appIdGlob, profileIndex });
}

export async function removeRule(appIdGlob: string): Promise<void> {
    await invoke<undefined>('delete_rule', { appIdGlob });
}

export async function fetchDevices(): Promise<DeviceInfo[]> {
    return invoke<DeviceInfo[]>('list_devices');
}

export async function doSimulateFocus(appId: string, title: string): Promise<void> {
    await invoke<undefined>('simulate_focus', { appId, title });
}
