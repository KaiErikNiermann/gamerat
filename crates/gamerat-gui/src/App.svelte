<script lang="ts">
    import { invoke } from '@tauri-apps/api/core';
    import { listen } from '@tauri-apps/api/event';
    import { onMount } from 'svelte';
    import AutoswitchToggle from './lib/AutoswitchToggle.svelte';
    import DaemonGate from './lib/DaemonGate.svelte';
    import DevicesPanel from './lib/DevicesPanel.svelte';
    import DevPanel from './lib/DevPanel.svelte';
    import { logEvent } from './lib/dev-log.js';
    import FocusSimulate from './lib/FocusSimulate.svelte';
    import GamesPanel from './lib/GamesPanel.svelte';
    import MouseView from './lib/MouseView.svelte';
    import ProfilesPanel from './lib/ProfilesPanel.svelte';
    import RulesPanel from './lib/RulesPanel.svelte';
    import SignalStream from './lib/SignalStream.svelte';
    import StatusCard from './lib/StatusCard.svelte';
    import ThemeToggle from './lib/ThemeToggle.svelte';
    import {
        fetchDevices,
        fetchGames,
        fetchProfiles,
        fetchRatbagdCompat,
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
        RatbagdCompatInfo,
        Rule,
        StatusInfo,
    } from './lib/types.js';

    const MAX_LOG_ENTRIES = 100;

    // The dev panel is shown by default in `pnpm dev` builds and
    // toggled off in release. URL `?dev=1` forces it on so we can
    // also debug release builds when needed.
    const showDevPanel = $derived.by(() => {
        if (import.meta.env.DEV) return true;
        try {
            return new URLSearchParams(globalThis.location.search).get('dev') === '1';
        } catch {
            return false;
        }
    });

    // ---------------------------------------------------------------------------
    // Reactive state
    // ---------------------------------------------------------------------------

    let version = $state<string | null>(null);
    let status = $state<StatusInfo | null>(null);
    let statusError = $state<string | null>(null);
    let ratbagdCompat = $state<RatbagdCompatInfo | null>(null);

    // ---------------------------------------------------------------------------
    // Daemon health check
    // ---------------------------------------------------------------------------
    //
    // Poll the daemon's `version` property to detect when the
    // gamerat-daemon process is up. When offline the whole UI is
    // gated behind a modal — there's nothing meaningful to render
    // without the daemon (no rules, no mouse, no events).
    //
    // Poll cadence:
    //   - offline / checking  → every 1.5s, catches a startup quickly
    //   - online              → every 10s, just liveness
    //
    // The ping uses raw `invoke` (not loggedInvoke) so it doesn't
    // spam the dev panel; we explicitly log transitions so dev users
    // still see when the daemon comes up or goes away.

    let daemonState = $state<'checking' | 'online' | 'offline'>('checking');
    let daemonLastError = $state<string | null>(null);
    let daemonLastPingAt = $state<number>(Date.now());
    let secondsSinceLastPing = $state<number>(0);
    let pollTimer: ReturnType<typeof setTimeout> | undefined;
    let pingTickTimer: ReturnType<typeof setInterval> | undefined;

    async function pingDaemon(): Promise<boolean> {
        try {
            await invoke<string>('version');
            daemonLastError = null;
            return true;
        } catch (error) {
            daemonLastError = String(error);
            return false;
        }
    }

    async function pingLoop(): Promise<void> {
        daemonLastPingAt = Date.now();
        const online = await pingDaemon();
        if (online && daemonState !== 'online') {
            daemonState = 'online';
            logEvent('daemon-online', { source: 'health-check' });
            // Fresh connection — load everything from scratch.
            void reloadAll();
        } else if (!online && daemonState !== 'offline') {
            daemonState = 'offline';
            logEvent('daemon-offline', { error: daemonLastError });
        }
        const delay = online ? 10_000 : 1500;
        pollTimer = setTimeout(() => {
            void pingLoop();
        }, delay);
    }

    async function reloadAll(): Promise<void> {
        await Promise.all([
            loadStatus(),
            loadRules(),
            loadDevices(),
            loadGames(),
            loadProfiles(),
            loadRatbagdCompat(),
        ]);
    }

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

    async function loadRatbagdCompat(): Promise<void> {
        try {
            ratbagdCompat = await fetchRatbagdCompat();
        } catch {
            // Probe is best-effort — surface "unreachable" rather than a
            // hard error if the IPC itself fails for some reason.
            ratbagdCompat = {
                kind: 'unreachable',
                api_version: null,
                expected: 0,
                warning: 'Could not probe ratbagd compatibility.',
            };
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
        // Kick off the health check loop — initial loads happen
        // inside `pingLoop` on the first successful ping, so an
        // offline daemon doesn't spam errors on mount.
        void pingLoop();

        // 1-second tick to refresh the "n seconds ago" copy on the
        // gate modal. Cheap; no IPC.
        pingTickTimer = setInterval(() => {
            secondsSinceLastPing = Math.max(
                0,
                Math.floor((Date.now() - daemonLastPingAt) / 1000),
            );
        }, 1000);

        // Listen for FocusChanged events forwarded from the Rust signal task.
        const unsubFocus = listen<FocusChangedPayload>('focus-changed', (event) => {
            const payload = event.payload;
            liveFocusedAppId = payload.app_id;
            pushLogEntry({ kind: 'focus', ts: Date.now(), payload });
            logEvent('focus-changed', payload);
        });

        // Listen for ProfileSwitched events.
        const unsubSwitch = listen<ProfileSwitchedPayload>('profile-switched', (event) => {
            const payload = event.payload;
            pushLogEntry({ kind: 'switch', ts: Date.now(), payload });
            logEvent('profile-switched', payload);
            // A profile switch might change the active profile on a device; reload.
            void loadDevices();
        });

        // Return a cleanup function that unregisters both listeners.
        return () => {
            void unsubFocus.then((fn) => { fn(); });
            void unsubSwitch.then((fn) => { fn(); });
            if (pollTimer !== undefined) clearTimeout(pollTimer);
            if (pingTickTimer !== undefined) clearInterval(pingTickTimer);
        };
    });
</script>

<div class="app-shell">
    <header class="app-header">
        <span class="app-logo" aria-hidden="true">
            <!-- minimalist rodent silhouette — see #icons/rat.svg if we
                 ever want to externalise these. -->
            <svg viewBox="0 0 24 24" width="22" height="22">
                <path
                    fill="currentColor"
                    d="M4 16c0-3 2-5 5-5 1.6 0 2.6.5 3.3 1.3l2.2-2.2a3 3 0 1 1 1.4 1.4l-2.2 2.2c.8.7 1.3 1.7 1.3 3.3 0 3-2 5-5 5s-5-2-5-5Zm5 1.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Zm9-10.5a1.5 1.5 0 1 0 0-3 1.5 1.5 0 0 0 0 3Z"
                />
            </svg>
        </span>
        <h1 class="app-title">gamerat</h1>
        <span class="app-subtitle">daemon control panel</span>
        <div class="app-header-spacer"></div>
        <AutoswitchToggle />
        <ThemeToggle />
    </header>

    <!-- Daemon gate: hides the whole UI until pingLoop confirms the
         daemon is up. We render the layout *behind* the modal so
         transitioning online → offline doesn't unmount everything
         (which would lose any per-component state and re-fetch
         everything from scratch on reconnect). -->
    {#if daemonState !== 'online'}
        <DaemonGate
            state={daemonState}
            lastError={daemonLastError}
            {secondsSinceLastPing}
        />
    {/if}

    <!-- Piper / G-Hub layout: the mouse view is the hero on the left,
         everything else stacks in a sidebar on the right. Below
         ~1024px the two columns collapse into one continuous stack
         (hero first, sidebar contents after) so the layout still
         works on narrow / portrait viewports. -->
    <main class="app-layout" aria-hidden={daemonState !== 'online'}>
        <section class="app-hero">
            <MouseView device={devices[0] ?? null} />
        </section>

        <aside class="app-sidebar">
            <StatusCard
                {version}
                {status}
                focusedAppId={liveFocusedAppId}
                error={statusError}
                {ratbagdCompat}
            />

            <ProfilesPanel {profiles} onprofileschange={loadProfiles} />

            <RulesPanel {rules} {profiles} onruleschange={loadRules} />

            <GamesPanel {games} {profiles} onruleschange={loadRules} />

            <DevicesPanel {devices} error={devicesError} />

            <SignalStream entries={logEntries} />

            <FocusSimulate />

            {#if showDevPanel}
                <DevPanel />
            {/if}
        </aside>
    </main>
</div>
