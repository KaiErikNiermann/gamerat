<script lang="ts">
    /**
     * App-level settings modal — opened from the gear icon in the
     * header. Reads every toggle straight from the daemon on mount
     * so values never go stale, and writes through the matching IPC
     * setters on each change (no Save button — the modal exists
     * only to surface controls, not to batch them).
     *
     * Reuses the `binding-editor-backdrop` modal pattern from
     * `ProfilesPanel.svelte`.
     */

    import X from '@lucide/svelte/icons/x';
    import { onMount } from 'svelte';
    import {
        fetchDesktopReturnDelayMs,
        fetchDesktopReturnEnabled,
        fetchNotifyOnProfileSwitch,
        writeDesktopReturnDelayMs,
        writeDesktopReturnEnabled,
        writeNotifyOnProfileSwitch,
    } from './ipc.js';

    interface Props {
        onclose: () => void;
    }

    const { onclose }: Props = $props();

    let loading = $state(true);
    let loadError = $state<string | null>(null);
    let desktopReturnEnabled = $state<boolean>(true);
    let desktopReturnDelayMs = $state<number>(120_000);
    let notifyOnProfileSwitch = $state<boolean>(false);

    /** Bound to the delay number input; converted from / to ms on
     *  read / write. Unit dropdown picks between s and min. */
    let delayValue = $state<number>(2);
    let delayUnit = $state<'s' | 'min'>('min');

    function delayValueFromMs(ms: number): { value: number; unit: 's' | 'min' } {
        // Prefer minutes when the value lands on a whole-minute
        // boundary; otherwise stay in seconds for precision.
        if (ms >= 60_000 && ms % 60_000 === 0) {
            return { value: ms / 60_000, unit: 'min' };
        }
        return { value: ms / 1000, unit: 's' };
    }

    function msFromDelayValue(value: number, unit: 's' | 'min'): number {
        if (unit === 'min') return Math.round(value * 60_000);
        return Math.round(value * 1000);
    }

    onMount(() => {
        void (async () => {
            try {
                [desktopReturnEnabled, desktopReturnDelayMs, notifyOnProfileSwitch] =
                    await Promise.all([
                        fetchDesktopReturnEnabled(),
                        fetchDesktopReturnDelayMs(),
                        fetchNotifyOnProfileSwitch(),
                    ]);
                const v = delayValueFromMs(desktopReturnDelayMs);
                delayValue = v.value;
                delayUnit = v.unit;
                loadError = null;
            } catch (error) {
                loadError = String(error);
            } finally {
                loading = false;
            }
        })();
    });

    async function handleReturnEnabledChange(value: boolean): Promise<void> {
        const previous = desktopReturnEnabled;
        desktopReturnEnabled = value;
        try {
            await writeDesktopReturnEnabled(value);
        } catch (error) {
            desktopReturnEnabled = previous;
            loadError = `desktop_return_enabled: ${String(error)}`;
        }
    }

    async function handleDelayChange(): Promise<void> {
        const ms = msFromDelayValue(delayValue, delayUnit);
        if (!Number.isFinite(ms) || ms < 0) return;
        const previous = desktopReturnDelayMs;
        desktopReturnDelayMs = ms;
        try {
            await writeDesktopReturnDelayMs(ms);
        } catch (error) {
            desktopReturnDelayMs = previous;
            loadError = `desktop_return_delay_ms: ${String(error)}`;
        }
    }

    async function handleNotifyChange(value: boolean): Promise<void> {
        const previous = notifyOnProfileSwitch;
        notifyOnProfileSwitch = value;
        try {
            await writeNotifyOnProfileSwitch(value);
        } catch (error) {
            notifyOnProfileSwitch = previous;
            loadError = `notify_on_profile_switch: ${String(error)}`;
        }
    }
</script>

<div
    class="binding-editor-backdrop"
    role="dialog"
    aria-modal="true"
    aria-label="Settings"
    onclick={(e) => { if (e.target === e.currentTarget) onclose(); }}
    onkeydown={(e) => { if (e.key === 'Escape') onclose(); }}
    tabindex="-1"
>
    <div class="binding-editor-card settings-card">
        <header class="binding-editor-head">
            <h3 class="binding-editor-title">Settings</h3>
            <button
                type="button"
                class="btn-ghost-sm"
                onclick={onclose}
                aria-label="Close settings"
            >
                <X size={14} />
            </button>
        </header>

        {#if loading}
            <p class="muted">Loading…</p>
        {:else}
            <section class="settings-section">
                <h4 class="settings-section-title">Focus behaviour</h4>
                <label class="settings-row">
                    <input
                        type="checkbox"
                        checked={desktopReturnEnabled}
                        onchange={(e) => {
                            void handleReturnEnabledChange(
                                (e.target as HTMLInputElement).checked,
                            );
                        }}
                    />
                    <span>
                        <strong>Return to base when no rule matches</strong>
                        <small class="muted">
                            Off keeps the current game profile active even when
                            you focus a non-game window — useful if you don't
                            curate the Base slot.
                        </small>
                    </span>
                </label>

                <label class="settings-row" class:settings-row-disabled={!desktopReturnEnabled}>
                    <span class="settings-row-label">Wait before returning</span>
                    <input
                        class="input-field settings-delay-input"
                        type="number"
                        min="0"
                        step={delayUnit === 'min' ? 1 : 5}
                        bind:value={delayValue}
                        onchange={() => { void handleDelayChange(); }}
                        disabled={!desktopReturnEnabled}
                        aria-label="Desktop-return delay value"
                    />
                    <select
                        class="input-field settings-delay-unit"
                        bind:value={delayUnit}
                        onchange={() => { void handleDelayChange(); }}
                        disabled={!desktopReturnEnabled}
                        aria-label="Desktop-return delay unit"
                    >
                        <option value="s">seconds</option>
                        <option value="min">minutes</option>
                    </select>
                </label>
                <p class="muted text-xs settings-section-hint">
                    Brief tab-outs (Discord, Google) shorter than this delay
                    don't kick the profile back. 0 fires immediately.
                </p>
            </section>

            <section class="settings-section">
                <h4 class="settings-section-title">Notifications</h4>
                <label class="settings-row">
                    <input
                        type="checkbox"
                        checked={notifyOnProfileSwitch}
                        onchange={(e) => {
                            void handleNotifyChange(
                                (e.target as HTMLInputElement).checked,
                            );
                        }}
                    />
                    <span>
                        <strong>Notify on profile switch</strong>
                        <small class="muted">
                            Raise a system notification each time a profile
                            switch lands. Off by default — fullscreen gamers
                            usually find these noisy.
                        </small>
                    </span>
                </label>
            </section>

            {#if loadError !== null}
                <p class="error-text">{loadError}</p>
            {/if}
        {/if}
    </div>
</div>
