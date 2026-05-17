<script lang="ts">
    import { listen } from '@tauri-apps/api/event';
    import { onMount } from 'svelte';
    import DevicesPanel from './lib/DevicesPanel.svelte';
    import FocusSimulate from './lib/FocusSimulate.svelte';
    import GamesPanel from './lib/GamesPanel.svelte';
    import MouseView from './lib/MouseView.svelte';
    import ProfilesPanel from './lib/ProfilesPanel.svelte';
    import RulesPanel from './lib/RulesPanel.svelte';
    import SignalStream from './lib/SignalStream.svelte';
    import StatusCard from './lib/StatusCard.svelte';
    import {
        fetchDevices,
        fetchGames,
        fetchProfiles,
        fetchRules,
        fetchStatus,
        fetchVersion,
    } from './lib/ipc.js';
    import type {
        DeviceInfo,
        FocusChangedPayload,
        GameEntry,
        GameratProfile,
        LogEntry,
        ProfileSwitchedPayload,
        Rule,
        StatusInfo,
    } from './lib/types.js';

    const MAX_LOG_ENTRIES = 100;

    // ---------------------------------------------------------------------------
    // Reactive state
    // ---------------------------------------------------------------------------

    let version = $state<string | null>(null);
    let status = $state<StatusInfo | null>(null);
    let statusError = $state<string | null>(null);

    /** Live-updated from FocusChanged signals — overrides status.focused_app_id. */
    let liveFocusedAppId = $state<string | null>(null);

    let rules = $state<Rule[]>([]);
    let devices = $state<DeviceInfo[]>([]);
    let devicesError = $state<string | null>(null);
    let games = $state<GameEntry[]>([]);
    let profiles = $state<GameratProfile[]>([]);

    /** Signal stream log — most recent first, capped at MAX_LOG_ENTRIES. */
    let logEntries = $state<LogEntry[]>([]);

    // ---------------------------------------------------------------------------
    // Data loading helpers
    // ---------------------------------------------------------------------------

    async function loadStatus(): Promise<void> {
        try {
            [version, status] = await Promise.all([fetchVersion(), fetchStatus()]);
            statusError = null;
        } catch (error) {
            statusError = String(error);
        }
    }

    async function loadRules(): Promise<void> {
        try {
            rules = await fetchRules();
        } catch {
            // Rule errors surface inline in the panel.
        }
    }

    async function loadDevices(): Promise<void> {
        try {
            devices = await fetchDevices();
            devicesError = null;
        } catch (error) {
            devicesError = String(error);
        }
    }

    async function loadGames(): Promise<void> {
        try {
            games = await fetchGames();
        } catch {
            // Games errors surface inline in the panel; an empty list
            // is also "no games discovered".
        }
    }

    async function loadProfiles(): Promise<void> {
        try {
            profiles = await fetchProfiles();
        } catch {
            // Same UX as games — empty array conveys "none yet".
        }
    }

    function pushLogEntry(entry: LogEntry): void {
        // Prepend so newest appears first; evict beyond the cap.
        logEntries = [entry, ...logEntries].slice(0, MAX_LOG_ENTRIES);
    }

    // ---------------------------------------------------------------------------
    // Mount: initial load + signal subscriptions
    // ---------------------------------------------------------------------------

    onMount(() => {
        void loadStatus();
        void loadRules();
        void loadDevices();
        void loadGames();
        void loadProfiles();

        // Listen for FocusChanged events forwarded from the Rust signal task.
        const unsubFocus = listen<FocusChangedPayload>('focus-changed', (event) => {
            const payload = event.payload;
            liveFocusedAppId = payload.app_id;
            pushLogEntry({ kind: 'focus', ts: Date.now(), payload });
        });

        // Listen for ProfileSwitched events.
        const unsubSwitch = listen<ProfileSwitchedPayload>('profile-switched', (event) => {
            const payload = event.payload;
            pushLogEntry({ kind: 'switch', ts: Date.now(), payload });
            // A profile switch might change the active profile on a device; reload.
            void loadDevices();
        });

        // Return a cleanup function that unregisters both listeners.
        return () => {
            void unsubFocus.then((fn) => { fn(); });
            void unsubSwitch.then((fn) => { fn(); });
        };
    });
</script>

<div class="app-shell">
    <header class="app-header">
        <span class="app-logo">🐀</span>
        <h1 class="app-title">gamerat</h1>
        <span class="app-subtitle">daemon control panel</span>
    </header>

    <main class="app-grid">
        <StatusCard
            {version}
            {status}
            focusedAppId={liveFocusedAppId}
            error={statusError}
        />

        <MouseView device={devices[0] ?? null} />

        <ProfilesPanel {profiles} onprofileschange={loadProfiles} />

        <RulesPanel {rules} {profiles} onruleschange={loadRules} />

        <GamesPanel {games} {profiles} onruleschange={loadRules} />

        <DevicesPanel {devices} error={devicesError} />

        <SignalStream entries={logEntries} />

        <FocusSimulate />
    </main>
</div>
