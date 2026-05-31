<script lang="ts">
    import CircleAlert from '@lucide/svelte/icons/circle-alert';
    import Icon from './Icon.svelte';
    import { m } from './paraglide/messages.js';
    import type {
        FocusBridgeState,
        RatbagdCompatInfo,
        SoftInputState,
        StatusInfo,
    } from './types.js';

    /** Placeholder issues URL — points at the project repo's bug
     *  tracker so users who exhaust the in-popover remediations have
     *  a one-click route to a report. Bump if we move forges. */
    const ISSUES_URL = 'https://github.com/KaiErikNiermann/gamerat/issues/new';

    interface Props {
        version: string | null;
        status: StatusInfo | null;
        focusedAppId: string | null;
        error: string | null;
        ratbagdCompat: RatbagdCompatInfo | null;
        /** KDE focus-bridge health. `null` while the first probe is in
         *  flight; `not-applicable` on non-KDE sessions (row hidden). */
        focusBridge: FocusBridgeState | null;
        /** True while a Repair round-trip is running. */
        repairingBridge: boolean;
        /** Soft-input subsystem runtime state. `null` while the first
         *  probe is in flight; row stays visible thereafter to surface
         *  the master flag's state. */
        softInput: SoftInputState | null;
        /** True while a soft-input re-probe is in flight. */
        recheckingSoftInput: boolean;
        onrepairbridge: () => void;
        /** Re-fetch the soft-input state from the daemon. Used by the
         *  "Re-check" button so the user doesn't have to reload after
         *  fixing the input-group membership in another terminal. */
        onrechecksoftinput: () => void;
    }

    const {
        version,
        status,
        focusedAppId,
        error,
        ratbagdCompat,
        focusBridge,
        repairingBridge,
        softInput,
        recheckingSoftInput,
        onrepairbridge,
        onrechecksoftinput,
    }: Props = $props();

    function dash(s: string | null | undefined): string {
        return s && s.length > 0 ? s : '—';
    }

    /** The focus-bridge row only makes sense on a KWin session — hide it
     *  entirely for wlr / X11 (`not-applicable`) and before the first
     *  probe lands (`null`). */
    const showBridgeRow = $derived(
        focusBridge !== null && focusBridge !== 'not-applicable',
    );

    function bridgePillLabel(s: FocusBridgeState): string {
        if (s === 'active') return m.status_bridge_active();
        if (s === 'not-loaded') return m.status_bridge_not_loaded();
        return m.status_bridge_unknown();
    }

    function bridgePillClass(s: FocusBridgeState): string {
        if (s === 'active') return 'compat-pill compat-pill-ok';
        if (s === 'not-loaded') return 'compat-pill compat-pill-err';
        return 'compat-pill compat-pill-warn';
    }

    function softInputPillLabel(s: SoftInputState): string {
        if (s === 'active') return m.status_softinput_active();
        if (s === 'unavailable') return m.status_softinput_inert();
        return m.status_softinput_off();
    }

    function softInputPillClass(s: SoftInputState): string {
        if (s === 'active') return 'compat-pill compat-pill-ok';
        if (s === 'unavailable') return 'compat-pill compat-pill-err';
        return 'compat-pill';
    }

    function compatPillLabel(c: RatbagdCompatInfo): string {
        if (c.kind === 'exact') return m.status_compat_exact({ version: c.api_version ?? c.expected });
        if (c.kind === 'unreachable') return m.status_compat_unreachable_pill();
        return m.status_compat_version({ version: c.api_version ?? '?' });
    }

    function compatPillClass(c: RatbagdCompatInfo): string {
        if (c.kind === 'exact') return 'compat-pill compat-pill-ok';
        if (c.kind === 'unreachable' || c.kind === 'below_min') return 'compat-pill compat-pill-err';
        return 'compat-pill compat-pill-warn';
    }

    /** Localized compatibility warning, derived from the structured `kind`
     *  + version numbers rather than the daemon's English `warning` string
     *  (which we no longer render). Empty for the `exact` case. */
    function compatWarning(c: RatbagdCompatInfo): string {
        const version = c.api_version ?? c.expected;
        switch (c.kind) {
            case 'known_compat': {
                return m.status_compat_known({ version, expected: c.expected });
            }
            case 'below_min': {
                return m.status_compat_below_min({ version });
            }
            case 'above_known': {
                return m.status_compat_above_known({ version, expected: c.expected });
            }
            case 'unreachable': {
                return m.status_compat_unreachable_warning();
            }
            default: {
                return '';
            }
        }
    }
</script>

<section class="panel">
    <h2 class="panel-title"><Icon name="bolt" /> {m.status_title()}</h2>

    {#if error}
        <p class="error-text">{error}</p>
    {:else if status === null}
        <p class="muted">{m.status_connecting()}</p>
    {:else}
        <dl class="stat-grid">
            <dt>{m.status_daemon_version()}</dt>
            <dd>{dash(version)}</dd>

            <dt>{m.status_focused_app()}</dt>
            <dd class="live-value">
                {dash(focusedAppId ?? status.focused_app_id)}
                {#if (focusedAppId ?? status.focused_app_id).length === 0}
                    <span class="info-tip" title={m.status_focused_app_hint()}>
                        <small>{m.status_no_events()}</small>
                    </span>
                {/if}
            </dd>

            <dt>{m.status_last_switch_reason()}</dt>
            <dd>{dash(status.last_switch_reason)}</dd>

            <dt>{m.status_rules_loaded()}</dt>
            <dd>{status.rules_loaded}</dd>

            <dt>{m.status_ratbagd()}</dt>
            <dd>
                {#if ratbagdCompat === null}
                    <span class="muted">…</span>
                {:else}
                    <span
                        class={compatPillClass(ratbagdCompat)}
                        title={compatWarning(ratbagdCompat)}
                    >
                        {compatPillLabel(ratbagdCompat)}
                    </span>
                    {#if ratbagdCompat.kind !== 'exact'}
                        <p class="compat-warning">{compatWarning(ratbagdCompat)}</p>
                    {/if}
                {/if}
            </dd>

            {#if softInput !== null}
                <dt>{m.status_softinput()}</dt>
                <dd>
                    <span class="soft-input-row">
                        <span
                            class={softInputPillClass(softInput)}
                            title={m.status_softinput_hint()}
                        >
                            {softInputPillLabel(softInput)}
                        </span>
                        {#if softInput === 'unavailable'}
                            <span class="soft-input-help">
                                <button
                                    class="soft-input-help-trigger"
                                    type="button"
                                    aria-label={m.status_softinput_why()}
                                    aria-describedby="soft-input-popover"
                                >
                                    <CircleAlert size={14} />
                                </button>
                                <div
                                    class="soft-input-popover"
                                    id="soft-input-popover"
                                    role="tooltip"
                                >
                                    <p class="soft-input-popover-title">
                                        {m.status_softinput_popover_title()}
                                    </p>
                                    <p class="soft-input-popover-body">
                                        {m.status_softinput_popover_body()}
                                    </p>
                                    <p class="soft-input-popover-section-title">
                                        {m.status_softinput_try()}
                                    </p>
                                    <ol class="soft-input-popover-steps">
                                        <!-- Shell commands stay literal <code>
                                             (copyable, language-neutral); only
                                             the surrounding prose is localized. -->
                                        <li>
                                            <code>sudo usermod -aG input $USER</code>
                                        </li>
                                        <li>{m.status_softinput_step_relogin()}</li>
                                        <li>
                                            {m.status_softinput_step_restart()}
                                            (<code>systemctl --user restart gamerat-daemon</code>).
                                        </li>
                                        <li>{m.status_softinput_step_reboot()}</li>
                                        <li>{m.status_softinput_step_status()}</li>
                                    </ol>
                                    <div class="soft-input-popover-footer">
                                        <button
                                            class="btn-ghost-sm"
                                            type="button"
                                            onclick={onrechecksoftinput}
                                            disabled={recheckingSoftInput}
                                        >
                                            {recheckingSoftInput
                                                ? m.status_softinput_rechecking()
                                                : m.status_softinput_recheck()}
                                        </button>
                                        <a
                                            class="soft-input-popover-issue-link"
                                            href={ISSUES_URL}
                                            target="_blank"
                                            rel="noopener noreferrer"
                                        >
                                            {m.status_softinput_report()}
                                        </a>
                                    </div>
                                </div>
                            </span>
                        {/if}
                    </span>
                </dd>
            {/if}

            {#if showBridgeRow && focusBridge !== null}
                <dt>{m.status_bridge_label()}</dt>
                <dd>
                    <span
                        class={bridgePillClass(focusBridge)}
                        title={m.status_bridge_hint()}
                    >
                        {bridgePillLabel(focusBridge)}
                    </span>
                    {#if focusBridge === 'not-loaded'}
                        <p class="compat-warning">
                            {m.status_bridge_not_loaded_warning()}
                        </p>
                        <button
                            class="btn-ghost-sm"
                            type="button"
                            onclick={onrepairbridge}
                            disabled={repairingBridge}
                        >
                            {repairingBridge ? m.status_bridge_repairing() : m.status_bridge_repair()}
                        </button>
                    {:else if focusBridge === 'unknown'}
                        <p class="compat-warning">
                            {m.status_bridge_unknown_warning()}
                        </p>
                        <button
                            class="btn-ghost-sm"
                            type="button"
                            onclick={onrepairbridge}
                            disabled={repairingBridge}
                        >
                            {repairingBridge ? m.status_bridge_repairing() : m.status_bridge_repair()}
                        </button>
                    {/if}
                </dd>
            {/if}
        </dl>
    {/if}
</section>
