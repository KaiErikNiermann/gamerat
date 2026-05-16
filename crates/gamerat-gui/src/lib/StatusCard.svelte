<script lang="ts">
    import type { StatusInfo } from './types.js';

    interface Props {
        version: string | null;
        status: StatusInfo | null;
        focusedAppId: string | null;
        error: string | null;
    }

    const { version, status, focusedAppId, error }: Props = $props();

    function dash(s: string | null | undefined): string {
        return s && s.length > 0 ? s : '—';
    }
</script>

<section class="panel">
    <h2 class="panel-title">⚡ Status</h2>

    {#if error}
        <p class="error-text">{error}</p>
    {:else if status === null}
        <p class="muted">Connecting…</p>
    {:else}
        <dl class="stat-grid">
            <dt>Daemon version</dt>
            <dd>{dash(version)}</dd>

            <dt>Focused app</dt>
            <dd class="live-value">{dash(focusedAppId ?? status.focused_app_id)}</dd>

            <dt>Last switch reason</dt>
            <dd>{dash(status.last_switch_reason)}</dd>

            <dt>Rules loaded</dt>
            <dd>{status.rules_loaded}</dd>
        </dl>
    {/if}
</section>
