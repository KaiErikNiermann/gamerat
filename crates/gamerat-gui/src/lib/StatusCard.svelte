<script lang="ts">
    import Icon from './Icon.svelte';
    import type { RatbagdCompatInfo, StatusInfo } from './types.js';

    interface Props {
        version: string | null;
        status: StatusInfo | null;
        focusedAppId: string | null;
        error: string | null;
        ratbagdCompat: RatbagdCompatInfo | null;
    }

    const { version, status, focusedAppId, error, ratbagdCompat }: Props = $props();

    function dash(s: string | null | undefined): string {
        return s && s.length > 0 ? s : '—';
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
                        title="The daemon hasn't received any focus events yet. On KDE Plasma 6, install the KWin script bridge — see data/kwin-script/README.md or run scripts/install-kwin-script.sh. On Sway / Hyprland it works out of the box via wlr-foreign-toplevel-management."
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
        </dl>
    {/if}
</section>
