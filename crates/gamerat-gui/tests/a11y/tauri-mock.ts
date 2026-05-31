// Tauri runtime mock for the a11y suite.
//
// The GUI is hard-gated behind Tauri: `App.svelte` calls
// `invoke('daemon_alive')` on mount and blocks the entire UI behind
// `DaemonGate` until it resolves truthy, then fans out ~15 more IPC calls
// to populate the panels. In a plain browser none of that exists, so the
// UI would never render.
//
// Tauri 2 routes *every* `invoke()` — including the event plugin's
// `listen()` (`plugin:event|listen`) — through the single chokepoint
// `window.__TAURI_INTERNALS__.invoke`. Mocking that one function with a
// fixture table unblocks the whole app with zero production-code changes.
//
// Fixtures are intentionally *populated* (a profile with buttons + LEDs, a
// rule, a steam game, a manual game, a device with slots) so axe scans the
// maximum amount of rendered surface — empty panels would hide tokens we
// want to check. Shapes mirror `src/lib/types.ts`.

import type { Page } from '@playwright/test';

/** Command → canned return value. Mirrors the wire shapes the Tauri
 *  commands in `src-tauri/src/commands.rs` produce. Commands absent from
 *  this map (and all void setters) resolve to `null`, which every caller
 *  treats as a benign empty result. */
const FIXTURES: Record<string, unknown> = {
    // ── Daemon liveness + status ────────────────────────────────────
    daemon_alive: true,
    version: '0.0.1-a11y',
    status: {
        focused_app_id: 'cs2.exe',
        last_switch_reason: 'focus:cs2.exe',
        rules_loaded: 1,
    },
    ratbagd_compat: {
        kind: 'exact',
        api_version: 1,
        expected: 1,
        warning: null,
    },
    // 'active' exercises the StatusCard "soft input" pill in its lit state.
    fetch_soft_input_state: 'active',
    get_software_macros_enabled: true,
    get_autoswitch: true,
    // 'not-applicable' keeps the KWin-bridge row hidden (non-KDE session),
    // matching the common case; the row's own contrast is covered by the
    // other status pills.
    check_focus_bridge: 'not-applicable',

    // ── Rules / games / profiles ────────────────────────────────────
    list_rules: [
        { app_id_glob: 'cs2.exe', profile_id: 'fps', created_unix: 1_700_000_000 },
    ],
    list_games: [
        {
            id: 'steam:730',
            name: 'Counter-Strike 2',
            launcher: 'steam',
            install_dir: '/home/u/.steam/steamapps/common/cs2',
            executable: 'cs2.exe',
            app_id_hint: 'cs2.exe',
        },
        {
            id: 'manual:my-game',
            name: 'My DRM-free Game',
            launcher: 'manual',
            install_dir: '/mnt/games/MyGame',
            executable: '',
            app_id_hint: 'mygame.exe',
        },
    ],
    list_profiles: [
        {
            id: 'fps',
            name: 'FPS',
            description: 'High DPI, sniper toggle',
            category: 'specific',
            inherits_from: '',
            dpi: [800, 1600, 3200],
            active_dpi_stage: 1,
            created_unix: 1_700_000_000,
            buttons: [
                { index: 0, action: { kind: 1, value: 1, macro_steps: [] } },
                { index: 1, action: { kind: 3, value: 30, macro_steps: [] } },
            ],
            leds: [{ index: 0, mode: 1, color: [245, 158, 11], brightness: 255 }],
            soft_macros: [],
        },
    ],

    // ── Device + per-slot readback ──────────────────────────────────
    list_devices: [
        {
            object_path: '/org/freedesktop/ratbag1/device/0',
            name: 'Logitech G502 HERO',
            model: 'usb:046d:c08b:0',
            active_profile: 0,
            profile_count: 5,
            max_dpi_stages: 5,
        },
    ],
    get_slot_map: [
        { index: 0, profile_id: '', profile_name: 'Desktop', is_active: true, is_desktop: true },
        { index: 1, profile_id: 'fps', profile_name: 'FPS', is_active: false, is_desktop: false },
        { index: 2, profile_id: '', profile_name: '(empty)', is_active: false, is_desktop: false },
    ],
    list_buttons: [
        { index: 0, action: { kind: 1, value: 1, macro_steps: [] }, supported_action_types: [0, 1, 2, 3, 4] },
        { index: 1, action: { kind: 3, value: 30, macro_steps: [] }, supported_action_types: [0, 1, 2, 3, 4] },
        { index: 2, action: { kind: 0, value: 0, macro_steps: [] }, supported_action_types: [0, 1, 2, 3, 4] },
    ],
    list_leds: [
        {
            index: 0,
            mode: 1,
            color: [245, 158, 11],
            brightness: 255,
            supported_modes: [0, 1, 2, 3],
            color_depth: 1,
        },
    ],
    // ipc.ts destructures these as [dpi[], activeStage].
    get_active_profile_dpi: [[800, 1600, 3200], 1],
    get_profile_dpi: [[800, 1600, 3200], 1],
    get_active_dpi_stage: 1,
    get_dpi_stage_disable_caps: [true, true, true, true, true],

    // ── Settings modal getters ──────────────────────────────────────
    get_desktop_return_enabled: true,
    get_desktop_return_delay_ms: 500,
    get_notify_on_profile_switch: true,
};

/**
 * Install the mock on a page so it's present *before* any app script
 * runs. Must be called prior to `page.goto`.
 */
export async function installTauriMock(page: Page): Promise<void> {
    await page.addInitScript((fixtures: Record<string, unknown>) => {
        const internals = {
            // The event plugin registers callbacks through this; we never
            // fire events, so a stable dummy id is enough. (Extra runtime
            // args from the caller are harmlessly ignored.)
            transformCallback(): number {
                return 0;
            },
            invoke(cmd: string): Promise<unknown> {
                // listen()/unlisten() and any other plugin call: resolve a
                // numeric handle so the subscription bookkeeping succeeds.
                if (cmd.startsWith('plugin:')) return Promise.resolve(0);
                if (Object.prototype.hasOwnProperty.call(fixtures, cmd)) {
                    return Promise.resolve(fixtures[cmd]);
                }
                // Unknown commands + void setters: null never hangs an
                // awaiting caller and renders as a benign empty result.
                return Promise.resolve(null);
            },
            metadata: {
                currentWindow: { label: 'main' },
                currentWebview: { label: 'main', windowLabel: 'main' },
            },
        };
        (globalThis as unknown as { __TAURI_INTERNALS__: unknown }).__TAURI_INTERNALS__ =
            internals;
    }, FIXTURES);
}
