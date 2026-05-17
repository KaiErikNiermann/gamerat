<script lang="ts">
    import { match } from 'ts-pattern';
    import Icon from './Icon.svelte';
    import type { LogEntry } from './types.js';

    interface Props {
        entries: LogEntry[];
    }

    const { entries }: Props = $props();

    function formatEntry(entry: LogEntry): string {
        return match(entry)
            .with({ kind: 'focus' }, ({ payload }) => {
                const appId = payload.app_id.length > 0 ? payload.app_id : '(empty)';
                return `focus  ${appId}  "${payload.title}"  src=${payload.source}`;
            })
            .with({ kind: 'switch' }, ({ payload }) =>
                `switch  ${payload.device}  ${String(payload.from_profile)}→${String(payload.to_profile)}  (${payload.reason})`,
            )
            .exhaustive();
    }

    function entryClass(entry: LogEntry): string {
        return match(entry)
            .with({ kind: 'focus' }, () => 'entry-focus')
            .with({ kind: 'switch' }, () => 'entry-switch')
            .exhaustive();
    }

    function formatTime(ts: number): string {
        return new Date(ts).toLocaleTimeString();
    }
</script>

<section class="panel panel-wide">
    <h2 class="panel-title">
        <Icon name="radio" /> Signal stream
        <button
            type="button"
            class="info-tip"
            aria-label="What is signal stream?"
            data-tip="Live feed of D-Bus events the daemon emits — FocusChanged when the active window changes, ProfileSwitched when a rule swaps the active mouse profile. Useful for verifying rules trigger as expected."
        >
            <Icon name="info" size={12} />
        </button>
    </h2>

    {#if entries.length === 0}
        <p class="muted">Waiting for signals…</p>
    {:else}
        <ol class="signal-log" aria-label="Signal log" aria-live="polite">
            {#each entries as entry (`${String(entry.ts)}-${entry.kind}`)}
                <li class="log-entry {entryClass(entry)}">
                    <span class="log-time">{formatTime(entry.ts)}</span>
                    <span class="log-body">{formatEntry(entry)}</span>
                </li>
            {/each}
        </ol>
    {/if}
</section>
