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
    FocusBridgeState,
    GameEntry,
    GameratProfile,
    MacroStep,
    PanicHatchResult,
    ProfileButton,
    ProfileLed,
    RatbagButton,
    RatbagLed,
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

/** Probe the KDE focus-bridge health (read-only). */
export async function fetchFocusBridge(): Promise<FocusBridgeState> {
    return loggedInvoke<FocusBridgeState>('check_focus_bridge');
}

/** Install + enable + load the gamerat-focus KWin script (the "Repair"
 *  action). Returns the resulting state. */
export async function repairFocusBridge(): Promise<FocusBridgeState> {
    return loggedInvoke<FocusBridgeState>('ensure_kwin_focus_bridge');
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

/**
 * Ask the daemon which keycodes a macro leaves pressed after its last
 * step. Used by the binding editor's save-time warning. Returns an
 * empty array when the macro is balanced.
 */
export async function checkMacroBalance(steps: readonly MacroStep[]): Promise<readonly number[]> {
    const result = await loggedInvoke<number[]>('check_macro_balance', { steps });
    return result;
}

/**
 * Trigger the panic hatch on `(devicePath, buttonIndex)`. The daemon
 * either binds NONE immediately (no stuck keys) or rebinds to a
 * release-only macro and arms a 5-second auto-disable timer.
 *
 * Listen to the `panic-hatch-settled` Tauri event to know when the
 * timer fires (outcome `timeout_disabled` / `superseded`) or
 * {@link cancelPanicHatch} aborts it (outcome `cancelled`).
 */
export async function panicHatch(
    devicePath: string,
    buttonIndex: number,
): Promise<PanicHatchResult> {
    return loggedInvoke<PanicHatchResult>('panic_hatch', { devicePath, buttonIndex });
}

/** Abort a pending panic-hatch auto-disable timer. Idempotent. */
export async function cancelPanicHatch(devicePath: string, buttonIndex: number): Promise<void> {
    await loggedInvoke<undefined>('cancel_panic_hatch', { devicePath, buttonIndex });
}

/** Snapshot every LED on a device profile. Mirrors `fetchButtons`. */
export async function fetchLeds(
    devicePath: string,
    profileIndex: number = PROFILE_INDEX_ACTIVE,
): Promise<RatbagLed[]> {
    return loggedInvoke<RatbagLed[]>('list_leds', { devicePath, profileIndex });
}

/** Write one LED's mode + color + brightness via the daemon's SetLed.
 *  Implicitly commits to hardware. */
export async function writeLed(
    devicePath: string,
    profileIndex: number,
    ledIndex: number,
    led: ProfileLed,
): Promise<void> {
    await loggedInvoke<undefined>('set_led', {
        devicePath,
        profileIndex,
        ledIndex,
        led,
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

/** DPI stages + active stage index on the device's currently-active
 *  hardware profile. Lets MouseView's Base-mode editor render the
 *  live values without a gamerat profile record. */
export async function fetchActiveProfileDpi(
    devicePath: string,
): Promise<{ dpi: number[]; activeStage: number }> {
    const result = await loggedInvoke<[number[], number]>('get_active_profile_dpi', {
        devicePath,
    });
    return { dpi: result[0], activeStage: result[1] };
}

/** Per-resolution-slot answer to "can this slot be hardware-disabled?".
 *  Length matches the device's DPI slot count; entry `i` is `true` iff
 *  slot `i` declares `RATBAG_RESOLUTION_CAP_DISABLE`.
 *
 *  MouseView's DPI editor consults this: when every slot supports the
 *  cap, shortening the profile's stage array genuinely removes stages
 *  from the firmware cycle. When some slot lacks the cap, the
 *  shorten-cycle affordance is annotated/disabled because the firmware
 *  would keep cycling through the removed slots regardless. */
export async function fetchDpiStageDisableCaps(
    devicePath: string,
): Promise<boolean[]> {
    return loggedInvoke<boolean[]>('get_dpi_stage_disable_caps', { devicePath });
}

/** Write DPI + button bindings + LED state to the device's
 *  currently-active hardware profile in one batched commit. Used by
 *  MouseView's Base-mode editor (DPI stage edits, Reset to defaults,
 *  LED color picker apply). Pass empty arrays to skip the
 *  corresponding section. */
export async function applyToActiveProfile(
    devicePath: string,
    dpi: number[],
    activeStage: number,
    buttons: readonly ProfileButton[],
    leds: readonly ProfileLed[] = [],
): Promise<void> {
    await loggedInvoke<undefined>('apply_to_active_profile', {
        devicePath,
        dpi,
        activeStage,
        buttons,
        leds,
    });
}

export async function writeAutoswitch(value: boolean): Promise<boolean> {
    return loggedInvoke<boolean>('set_autoswitch', { value });
}

export async function fetchDesktopReturnEnabled(): Promise<boolean> {
    return loggedInvoke<boolean>('get_desktop_return_enabled');
}

export async function writeDesktopReturnEnabled(value: boolean): Promise<boolean> {
    return loggedInvoke<boolean>('set_desktop_return_enabled', { value });
}

export async function fetchDesktopReturnDelayMs(): Promise<number> {
    return loggedInvoke<number>('get_desktop_return_delay_ms');
}

export async function writeDesktopReturnDelayMs(value: number): Promise<number> {
    return loggedInvoke<number>('set_desktop_return_delay_ms', { value });
}

export async function fetchNotifyOnProfileSwitch(): Promise<boolean> {
    return loggedInvoke<boolean>('get_notify_on_profile_switch');
}

export async function writeNotifyOnProfileSwitch(value: boolean): Promise<boolean> {
    return loggedInvoke<boolean>('set_notify_on_profile_switch', { value });
}
