<script lang="ts">
    import { invoke } from '@tauri-apps/api/core';
    import { listen } from '@tauri-apps/api/event';
    import Rat from '@lucide/svelte/icons/rat';
    import SettingsIcon from '@lucide/svelte/icons/settings';
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
    import SettingsModal from './lib/SettingsModal.svelte';
    import SignalStream from './lib/SignalStream.svelte';
    import StatusCard from './lib/StatusCard.svelte';
    import ThemeToggle from './lib/ThemeToggle.svelte';
    import {
        fetchAutoswitch,
        fetchDevices,
        fetchFocusBridge,
        fetchGames,
        fetchProfileDpi,
        fetchProfiles,
        fetchRatbagdCompat,
        fetchRules,
        fetchSlotMap,
        fetchSoftInputState,
        fetchSoftwareMacrosEnabled,
        fetchStatus,
        fetchVersion,
        repairFocusBridge,
    } from './lib/ipc.js';
    import type {
        DeviceInfo,
        FocusBridgeState,
        FocusChangedPayload,
        GameEntry,
        GameratProfile,
        LogEntry,
        ProfileSwitchedPayload,
        ProfileSwitchingPayload,
        RatbagdCompatInfo,
        Rule,
        SlotInfo,
        SoftInputState,
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
    /** KDE focus-bridge health (the gamerat-focus KWin script). `null`
     *  until the first probe; drives the StatusCard error + Repair. */
    let focusBridge = $state<FocusBridgeState | null>(null);
    let repairingBridge = $state<boolean>(false);

    /** Soft-input subsystem runtime state. `null` while the first
     *  probe is in flight; drives the StatusCard's "Soft input" pill
     *  and gates the binding editor's "Convert to toggle" affordance. */
    let softInput = $state<SoftInputState | null>(null);
    /** Master opt-in flag (mirrors the daemon's `SoftwareMacrosEnabled`
     *  property). Cached separately from `softInput` because the
     *  binding editor needs to know "could this feature be used right
     *  now?" — and `disabled` is ambiguous between "user opted out"
     *  and "/dev/uinput unavailable". */
    let softwareMacrosEnabled = $state<boolean>(false);
    let recheckingSoftInput = $state<boolean>(false);

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
        // NameHasOwner via the session bus rather than a property
        // call on state.proxy: the proxy was built at GUI launch and
        // its property cache can get stuck if the daemon was down
        // then. NameHasOwner asks dbus-broker directly, so it
        // doesn't care about our proxy's state at all.
        try {
            const alive = await invoke<boolean>('daemon_alive');
            daemonLastError = alive ? null : 'gamerat-daemon name not on session bus';
            return alive;
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
        // Re-probe the focus bridge each online tick so a mid-session
        // KWin script drop (Plasma update, compositor restart) surfaces
        // within ~10s rather than going silently unnoticed.
        if (online) void loadFocusBridge();
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
            loadFocusBridge(),
            loadSoftInput(),
            loadAutoswitch(),
        ]);
        // Base DPI + active-slot lookup both depend on the first
        // device existing, so they have to run after `loadDevices`
        // resolves rather than racing in parallel. Cheap (one D-Bus
        // round-trip each) — fire-and-forget.
        void loadBaseDpi();
        void loadActiveSlot();
    }

    async function loadAutoswitch(): Promise<void> {
        try {
            autoswitchEnabled = await fetchAutoswitch();
        } catch {
            // Daemon down — gate modal already covers this.
            autoswitchEnabled = null;
        }
    }

    /** Live-updated from FocusChanged signals — overrides status.focused_app_id. */
    let liveFocusedAppId = $state<string | null>(null);

    let rules = $state<Rule[]>([]);
    let devices = $state<DeviceInfo[]>([]);
    let devicesError = $state<string | null>(null);
    let games = $state<GameEntry[]>([]);
    let profiles = $state<GameratProfile[]>([]);

    // Hoisted so MouseView's binding labels and the per-row Apply
    // button in ProfilesPanel both react to the same source of
    // truth without having to re-fetch on every render.
    // `null` while the initial fetch is in flight.
    let autoswitchEnabled = $state<boolean | null>(null);

    // Currently-selected gamerat profile for editing. Picked by
    // either ProfilesPanel's row click or MouseView's "Editing:"
    // dropdown — both surfaces converge here. `null` means
    // "(no profile selected — Desktop baseline)" which is the
    // default app-launch state.
    let selectedProfileId = $state<string | null>(null);

    /** The profile object matching selectedProfileId, or null. */
    const selectedProfile = $derived<GameratProfile | null>(
        selectedProfileId === null
            ? null
            : (profiles.find((p) => p.id === selectedProfileId) ?? null),
    );

    // Memoised so MouseView's `device` prop is stable across parent
    // re-renders. Reading `devices[0]` directly in the template
    // creates a fresh proxy view each render — that was kicking the
    // child's button-fetch effect into a feedback loop with the
    // dev-log SvelteSet through loggedInvoke.
    const firstDevice = $derived<DeviceInfo | null>(devices[0] ?? null);

    /** Signal stream log — most recent first, capped at MAX_LOG_ENTRIES. */
    let logEntries = $state<LogEntry[]>([]);

    /** Monotonic counter bumped whenever the slot map needs to
     *  re-fetch: profile-switched signals, manual apply, daemon
     *  reconnect. DevicesPanel watches it via $effect. */
    let slotMapRevision = $state<number>(0);

    /** DPI summary for the first device's Desktop slot (slot 0). Used
     *  by ProfilesPanel to render the Base row's DPI column in the
     *  same shape as the user-profile rows — without it that column
     *  is empty and the downstream Apply button lands at a different
     *  x-position. Null until the first fetch (or no device present). */
    let baseDpi = $state<{ dpi: readonly number[]; activeStage: number } | null>(null);

    /** Refresh `baseDpi` from the first device's slot 0. Called on
     *  device-list changes + profile-switched signals (since a switch
     *  back to slot 0 from MouseView's Base-mode editor can edit the
     *  DPI in place). Silent on error — leaves `baseDpi = null`, which
     *  the panel renders as a "—" fallback. */
    async function loadBaseDpi(): Promise<void> {
        const path = firstDevice?.object_path;
        if (path === undefined) {
            baseDpi = null;
            return;
        }
        try {
            baseDpi = await fetchProfileDpi(path, 0);
        } catch {
            baseDpi = null;
        }
    }

    /** Currently-active slot on the first device, used by ProfilesPanel
     *  to render a "live now" indicator on whichever row corresponds
     *  to the hardware-active profile. `null` while we don't know:
     *  no device, or the slot-map fetch hasn't run / failed. */
    let activeSlot = $state<SlotInfo | null>(null);

    /** Refresh `activeSlot` from the first device's slot map. Same
     *  triggers as `loadBaseDpi`: device-list changes + profile-
     *  switched signals. Cheap fire-and-forget; silent on error. */
    async function loadActiveSlot(): Promise<void> {
        const path = firstDevice?.object_path;
        if (path === undefined) {
            activeSlot = null;
            return;
        }
        try {
            const slots = await fetchSlotMap(path);
            activeSlot = slots.find((s) => s.is_active) ?? null;
        } catch {
            activeSlot = null;
        }
    }

    /** Whether a profile swap is in flight. Set true on
     *  `profile-switching`, cleared on `profile-switched` (with a
     *  ~250 ms minimum hold so fast commits still flash long enough
     *  to be perceptible). MouseView renders a small overlay badge
     *  while this is true so the hardware-jitter window reads as
     *  expected, not broken. */
    let switchingNow = $state<boolean>(false);
    let switchingClearAt = $state<number>(0);

    /** Settings modal visibility. */
    let settingsOpen = $state<boolean>(false);

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

    async function loadFocusBridge(): Promise<void> {
        try {
            focusBridge = await fetchFocusBridge();
        } catch {
            // Probe is best-effort; leave the row hidden on IPC failure
            // rather than surfacing a misleading error.
            focusBridge = 'unknown';
        }
    }

    async function loadSoftInput(): Promise<void> {
        try {
            [softInput, softwareMacrosEnabled] = await Promise.all([
                fetchSoftInputState(),
                fetchSoftwareMacrosEnabled(),
            ]);
        } catch {
            // Best-effort probe — leave the master flag as-is and surface
            // 'disabled' so the UI degrades gracefully instead of
            // claiming the feature is available.
            softInput = 'disabled';
        }
    }

    /** Manual "Re-check" trigger from the StatusCard's soft-input
     *  unavailable hint. The user fixes their input-group membership
     *  in another terminal + restarts the daemon, then clicks here
     *  rather than reloading the GUI. */
    async function recheckSoftInput(): Promise<void> {
        recheckingSoftInput = true;
        try {
            await loadSoftInput();
        } finally {
            recheckingSoftInput = false;
        }
    }

    async function repairBridge(): Promise<void> {
        repairingBridge = true;
        try {
            focusBridge = await repairFocusBridge();
        } catch {
            focusBridge = 'unknown';
        } finally {
            repairingBridge = false;
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
            // A live kwin-sourced event is proof the bridge is up — flip
            // the indicator optimistically so it self-heals between
            // 10s probes (e.g. right after a Repair starts delivering).
            if (payload.source === 'kwin' && focusBridge !== 'active') {
                focusBridge = 'active';
            }
            pushLogEntry({ kind: 'focus', ts: Date.now(), payload });
            logEvent('focus-changed', payload);
        });

        // Listen for ProfileSwitching events — fired pre-commit so
        // the UI can flash the "switching…" badge over the
        // firmware-jitter window. We hold for at least 250 ms even
        // if ProfileSwitched arrives faster, so very-quick commits
        // still produce a perceptible flash.
        const SWITCHING_MIN_HOLD_MS = 250;
        const unsubSwitching = listen<ProfileSwitchingPayload>(
            'profile-switching',
            (event) => {
                const payload = event.payload;
                logEvent('profile-switching', payload);
                switchingNow = true;
                switchingClearAt = Date.now() + SWITCHING_MIN_HOLD_MS;
            },
        );

        // Listen for ProfileSwitched events.
        const unsubSwitch = listen<ProfileSwitchedPayload>('profile-switched', (event) => {
            const payload = event.payload;
            pushLogEntry({ kind: 'switch', ts: Date.now(), payload });
            logEvent('profile-switched', payload);

            // Clear the switching indicator with the min-hold delay
            // so very-fast commits still flash long enough to read.
            const remainingHold = Math.max(0, switchingClearAt - Date.now());
            setTimeout(() => {
                switchingNow = false;
            }, remainingHold);

            // A profile switch might change the active profile on a device; reload.
            void loadDevices();
            // And bump the slot-map revision so DevicesPanel
            // re-fetches its slot table.
            slotMapRevision += 1;
            // Base-mode edits applied via `apply_to_active_profile`
            // also fire ProfileSwitched (with reason
            // `manual:base-edit`), so re-pull the Base row's DPI
            // summary to stay in sync with the live hardware.
            void loadBaseDpi();
            // The active slot just changed — re-pull it so
            // ProfilesPanel's "live now" indicator follows.
            void loadActiveSlot();
            // System notification (if enabled) is dispatched by the
            // daemon itself via org.freedesktop.Notifications — that
            // way it works even when the GUI is closed, and avoids
            // tauri-plugin-notification's Linux block_on bug.
        });

        // Return a cleanup function that unregisters both listeners.
        return () => {
            void unsubFocus.then((fn) => { fn(); });
            void unsubSwitching.then((fn) => { fn(); });
            void unsubSwitch.then((fn) => { fn(); });
            if (pollTimer !== undefined) clearTimeout(pollTimer);
            if (pingTickTimer !== undefined) clearInterval(pingTickTimer);
        };
    });

</script>

<div class="app-shell">

    <header class="app-header">
        <span class="app-logo" aria-hidden="true">
            <Rat size={22} />
        </span>
        <h1 class="app-title">gamerat</h1>
        <span class="app-subtitle">daemon control panel</span>
        <div class="app-header-spacer"></div>
        <AutoswitchToggle
            enabled={autoswitchEnabled}
            onchange={(value: boolean) => { autoswitchEnabled = value; }}
        />
        <button
            type="button"
            class="settings-icon-btn"
            aria-label="Open settings"
            title="Open settings"
            onclick={() => { settingsOpen = true; }}
        >
            <SettingsIcon size={16} />
        </button>
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
            <MouseView
                device={firstDevice}
                profile={selectedProfile}
                {autoswitchEnabled}
                profiles={profiles}
                {switchingNow}
                {softwareMacrosEnabled}
                onprofileschange={loadProfiles}
                onselectprofile={(id: string | null) => { selectedProfileId = id; }}
            />
        </section>

        <aside class="app-sidebar">
            <StatusCard
                {version}
                {status}
                focusedAppId={liveFocusedAppId}
                error={statusError}
                {ratbagdCompat}
                {focusBridge}
                {repairingBridge}
                {softInput}
                {recheckingSoftInput}
                onrepairbridge={() => { void repairBridge(); }}
                onrechecksoftinput={() => { void recheckSoftInput(); }}
            />

            <ProfilesPanel
                {profiles}
                {selectedProfileId}
                {autoswitchEnabled}
                {baseDpi}
                {activeSlot}
                onprofileschange={loadProfiles}
                onselect={(id: string | null) => { selectedProfileId = id; }}
            />

            <RulesPanel {rules} {profiles} onruleschange={loadRules} />

            <GamesPanel {games} {profiles} {rules} onruleschange={loadRules} />

            <DevicesPanel
                {devices}
                error={devicesError}
                {slotMapRevision}
                onpurgecomplete={() => { void reloadAll(); }}
            />

            <SignalStream entries={logEntries} />

            <FocusSimulate />

            {#if showDevPanel}
                <DevPanel />
            {/if}
        </aside>
    </main>

    {#if settingsOpen}
        <SettingsModal
            onclose={() => { settingsOpen = false; }}
            onsoftinputchange={() => { void recheckSoftInput(); }}
        />
    {/if}
</div>
