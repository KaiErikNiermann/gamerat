<script lang="ts">
    import Icon from './Icon.svelte';
    import type { FocusBridgeState, RatbagdCompatInfo, StatusInfo } from './types.js';

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
        onrepairbridge: () => void;
    }

    const {
        version,
        status,
        focusedAppId,
        error,
        ratbagdCompat,
        focusBridge,
        repairingBridge,
        onrepairbridge,
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
        if (s === 'active') return 'KWin bridge ✓';
        if (s === 'not-loaded') return 'KWin bridge not loaded';
        return 'KWin bridge ?';
    }

    function bridgePillClass(s: FocusBridgeState): string {
        if (s === 'active') return 'compat-pill compat-pill-ok';
        if (s === 'not-loaded') return 'compat-pill compat-pill-err';
        return 'compat-pill compat-pill-warn';
    }

    function compatPillLabel(c: RatbagdCompatInfo): string {
        if (c.kind === 'exact') return `ratbagd v${String(c.api_version ?? c.expected)} ✓`;
        if (c.kind === 'unreachable') return 'ratbagd not running';
        const v = c.api_version === null ? '?' : String(c.api_version);
        return `ratbagd v${v}`;
    }

    function compatPillClass(c: RatbagdCompatInfo): string {
        if (c.kind === 'exact') return 'compat-pill compat-pill-ok';
        if (c.kind === 'unreachable' || c.kind === 'below_min') return 'compat-pill compat-pill-err';
        return 'compat-pill compat-pill-warn';
    }
</script>

<section class="panel">
    <h2 class="panel-title"><Icon name="bolt" /> Status</h2>

    {#if error}
        <p class="error-text">{error}</p>
    {:else if status === null}
        <p class="muted">Connecting…</p>
    {:else}
        <dl class="stat-grid">
            <dt>Daemon version</dt>
            <dd>{dash(version)}</dd>

            <dt>Focused app</dt>
            <dd class="live-value">
                {dash(focusedAppId ?? status.focused_app_id)}
                {#if (focusedAppId ?? status.focused_app_id).length === 0}
                    <span
                        class="info-tip"
                        title="The daemon hasn't received any focus events yet. On KDE Plasma the focus bridge below must be loaded — use Repair if it isn't. On Sway / Hyprland it works out of the box via wlr-foreign-toplevel-management."
                    >
                        <small>(no events yet)</small>
                    </span>
                {/if}
            </dd>

            <dt>Last switch reason</dt>
            <dd>{dash(status.last_switch_reason)}</dd>

            <dt>Rules loaded</dt>
            <dd>{status.rules_loaded}</dd>

            <dt>ratbagd</dt>
            <dd>
                {#if ratbagdCompat === null}
                    <span class="muted">…</span>
                {:else}
                    <span
                        class={compatPillClass(ratbagdCompat)}
                        title={ratbagdCompat.warning ?? ''}
                    >
                        {compatPillLabel(ratbagdCompat)}
                    </span>
                    {#if ratbagdCompat.warning !== null && ratbagdCompat.kind !== 'exact'}
                        <p class="compat-warning">{ratbagdCompat.warning}</p>
                    {/if}
                {/if}
            </dd>

            {#if showBridgeRow && focusBridge !== null}
                <dt>Focus bridge</dt>
                <dd>
                    <span
                        class={bridgePillClass(focusBridge)}
                        title="On KDE Plasma, gamerat observes window focus through the gamerat-focus KWin script. Auto-switching only works while it's loaded."
                    >
                        {bridgePillLabel(focusBridge)}
                    </span>
                    {#if focusBridge === 'not-loaded'}
                        <p class="compat-warning">
                            The KWin focus script isn't loaded, so window
                            auto-switching is inactive. Repair loads it now and
                            enables it for future logins.
                        </p>
                        <button
                            class="btn-ghost-sm"
                            type="button"
                            onclick={onrepairbridge}
                            disabled={repairingBridge}
                        >
                            {repairingBridge ? 'Repairing…' : 'Repair'}
                        </button>
                    {:else if focusBridge === 'unknown'}
                        <p class="compat-warning">
                            Couldn't probe KWin. If auto-switching isn't working,
                            try Repair.
                        </p>
                        <button
                            class="btn-ghost-sm"
                            type="button"
                            onclick={onrepairbridge}
                            disabled={repairingBridge}
                        >
                            {repairingBridge ? 'Repairing…' : 'Repair'}
                        </button>
                    {/if}
                </dd>
            {/if}
        </dl>
    {/if}
</section>
