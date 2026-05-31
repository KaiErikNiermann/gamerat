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
    import Modal from './Modal.svelte';
    import {
        fetchDesktopReturnDelayMs,
        fetchDesktopReturnEnabled,
        fetchNotifyOnProfileSwitch,
        fetchSoftwareMacrosEnabled,
        writeDesktopReturnDelayMs,
        writeDesktopReturnEnabled,
        writeNotifyOnProfileSwitch,
        writeSoftwareMacrosEnabled,
    } from './ipc.js';
    import Select from './Select.svelte';
    import { changeLocale, currentLocale, LOCALES, localeLabel } from './locale.js';
    import { m } from './paraglide/messages.js';

    interface Props {
        onclose: () => void;
        /** Fired after the soft-macros master flag is successfully
         *  flipped on the daemon side. Lets the parent re-fetch the
         *  derived `softInput` pill state + the cached
         *  `softwareMacrosEnabled` it threads down to the binding
         *  editor — without it, both go stale until a full reload. */
        onsoftinputchange?: () => void;
    }

    const { onclose, onsoftinputchange }: Props = $props();

    let loading = $state(true);
    let loadError = $state<string | null>(null);
    let desktopReturnEnabled = $state<boolean>(true);
    let desktopReturnDelayMs = $state<number>(120_000);
    let notifyOnProfileSwitch = $state<boolean>(false);
    let softwareMacrosEnabled = $state<boolean>(false);
    /** Initial value of `softwareMacrosEnabled` at load time. Used to
     *  decide whether the "requires daemon restart" hint should show
     *  — only after a user-driven flip mid-session does the live
     *  subsystem disagree with the persisted flag. */
    let softwareMacrosInitial = $state<boolean>(false);

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
                [
                    desktopReturnEnabled,
                    desktopReturnDelayMs,
                    notifyOnProfileSwitch,
                    softwareMacrosEnabled,
                ] = await Promise.all([
                    fetchDesktopReturnEnabled(),
                    fetchDesktopReturnDelayMs(),
                    fetchNotifyOnProfileSwitch(),
                    fetchSoftwareMacrosEnabled(),
                ]);
                softwareMacrosInitial = softwareMacrosEnabled;
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

    async function handleSoftwareMacrosChange(value: boolean): Promise<void> {
        const previous = softwareMacrosEnabled;
        softwareMacrosEnabled = value;
        try {
            await writeSoftwareMacrosEnabled(value);
            // Notify the parent so the StatusCard pill + the binding
            // editor's master-flag gate refresh from the daemon. The
            // daemon's `current_state` returns `Disabled` immediately
            // when the flag goes off (it doesn't need a restart for
            // that direction), so the pill flips on the same tick.
            onsoftinputchange?.();
        } catch (error) {
            softwareMacrosEnabled = previous;
            loadError = `software_macros_enabled: ${String(error)}`;
        }
    }
</script>

<Modal label={m.settings_title()} {onclose}>
    <div class="binding-editor-card settings-card">
        <header class="binding-editor-head">
            <h3 class="binding-editor-title">{m.settings_title()}</h3>
            <button
                type="button"
                class="btn-ghost-sm"
                onclick={onclose}
                aria-label={m.settings_close_aria()}
            >
                <X size={14} />
            </button>
        </header>

        <!-- Language is a client-side preference (no daemon round-trip), so
             it renders regardless of the daemon-backed sections' load state. -->
        <section class="settings-section">
            <h4 class="settings-section-title">{m.settings_language_title()}</h4>
            <label class="settings-row">
                <Select
                    value={currentLocale()}
                    onchange={(next: string) => { changeLocale(next); }}
                    options={LOCALES.map((l) => ({ value: l, label: localeLabel(l) }))}
                    ariaLabel={m.settings_language_title()}
                />
            </label>
            <p class="muted text-xs settings-section-hint">{m.settings_language_desc()}</p>
        </section>

        {#if loading}
            <p class="muted">{m.settings_loading()}</p>
        {:else}
            <section class="settings-section">
                <h4 class="settings-section-title">{m.settings_focus_title()}</h4>
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
                        <strong>{m.settings_return_label()}</strong>
                        <small class="muted">{m.settings_return_desc()}</small>
                    </span>
                </label>

                <label class="settings-row" class:settings-row-disabled={!desktopReturnEnabled}>
                    <span class="settings-row-label">{m.settings_delay_label()}</span>
                    <input
                        class="input-field settings-delay-input"
                        type="number"
                        min="0"
                        step={delayUnit === 'min' ? 1 : 5}
                        bind:value={delayValue}
                        onchange={() => { void handleDelayChange(); }}
                        disabled={!desktopReturnEnabled}
                        aria-label={m.settings_delay_value_aria()}
                    />
                    <Select
                        className="settings-delay-unit"
                        bind:value={delayUnit}
                        onchange={() => { void handleDelayChange(); }}
                        options={[
                            { value: 's', label: m.settings_unit_seconds() },
                            { value: 'min', label: m.settings_unit_minutes() },
                        ]}
                        disabled={!desktopReturnEnabled}
                        ariaLabel={m.settings_delay_unit_aria()}
                    />
                </label>
                <p class="muted text-xs settings-section-hint">{m.settings_delay_hint()}</p>
            </section>

            <section class="settings-section">
                <h4 class="settings-section-title">{m.settings_notif_title()}</h4>
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
                        <strong>{m.settings_notif_label()}</strong>
                        <small class="muted">{m.settings_notif_desc()}</small>
                    </span>
                </label>
            </section>

            <section class="settings-section">
                <h4 class="settings-section-title">{m.settings_soft_title()}</h4>
                <label class="settings-row">
                    <input
                        type="checkbox"
                        checked={softwareMacrosEnabled}
                        onchange={(e) => {
                            void handleSoftwareMacrosChange(
                                (e.target as HTMLInputElement).checked,
                            );
                        }}
                    />
                    <span>
                        <strong>{m.settings_soft_label()}</strong>
                        <small class="muted">{m.settings_soft_desc()}</small>
                    </span>
                </label>
                {#if softwareMacrosEnabled !== softwareMacrosInitial}
                    <p class="muted text-xs settings-section-hint">
                        {m.settings_soft_restart()}
                        (<code>systemctl --user restart gamerat-daemon</code>).
                    </p>
                {/if}
            </section>

            {#if loadError !== null}
                <p class="error-text">{loadError}</p>
            {/if}
        {/if}
    </div>
</Modal>
