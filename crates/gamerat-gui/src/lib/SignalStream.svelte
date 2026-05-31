<script lang="ts">
    import { match } from 'ts-pattern';
    import Icon from './Icon.svelte';
    import { currentLocale } from './locale.js';
    import { m } from './paraglide/messages.js';
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
        return new Date(ts).toLocaleTimeString(currentLocale());
    }
</script>

<section class="panel">
    <h2 class="panel-title">
        <Icon name="radio" /> {m.signal_title()}
        <button
            type="button"
            class="info-tip"
            aria-label={m.signal_info_aria()}
            data-tip={m.signal_info_tip()}
        >
            <Icon name="info" size={12} />
        </button>
    </h2>

    {#if entries.length === 0}
        <p class="muted">{m.signal_waiting()}</p>
    {:else}
        <ol class="signal-log" aria-label={m.signal_log_aria()} aria-live="polite">
            {#each entries as entry (`${String(entry.ts)}-${entry.kind}`)}
                <li class="log-entry {entryClass(entry)}">
                    <span class="log-time">{formatTime(entry.ts)}</span>
                    <span class="log-body">{formatEntry(entry)}</span>
                </li>
            {/each}
        </ol>
    {/if}
</section>
