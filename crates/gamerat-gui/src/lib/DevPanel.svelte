<script lang="ts">
    import Icon from './Icon.svelte';
    import { clearDevLog, devLogEntries, type DevLogEntry } from './dev-log.js';

    let filterKind = $state<'all' | 'invoke' | 'event'>('all');
    let autoscroll = $state(true);

    // SvelteSet is reactive — passing it through a $derived gives us
    // a chronologically-stable array view.
    const entries = $derived<DevLogEntry[]>(
        [...devLogEntries()].filter((entry) => {
            if (filterKind === 'all') return true;
            if (filterKind === 'event') return entry.kind === 'event';
            return entry.kind !== 'event';
        }),
    );

    // Counters surface "are events flowing?" without forcing the user
    // to read every line. Use $derived.by so the closure sees fresh
    // values each tick rather than capturing the array reference.
    const focusEventCount = $derived.by(() =>
        [...devLogEntries()].filter((e) => e.kind === 'event' && e.label === 'focus-changed').length,
    );
    const switchEventCount = $derived.by(() =>
        [...devLogEntries()].filter((e) => e.kind === 'event' && e.label === 'profile-switched').length,
    );
    const invokeErrorCount = $derived.by(() =>
        [...devLogEntries()].filter((e) => e.kind === 'invoke-error').length,
    );

    let logEl: HTMLDivElement | undefined = $state();

    // Scroll to the latest entry whenever the list grows, but only if
    // the user hasn't disabled autoscroll. The `entries.size` read at
    // the top registers SvelteSet reactivity so this effect re-runs
    // when a row is appended.
    $effect(() => {
        // eslint-disable-next-line @typescript-eslint/no-unnecessary-condition
        if (entries.length === 0 || !autoscroll) return;
        const el = logEl;
        if (el === undefined) return;
        // queueMicrotask defers the read until after Svelte has
        // flushed the new <div> to the DOM.
        queueMicrotask(() => {
            el.scrollTop = el.scrollHeight;
        });
    });

    function classForKind(kind: DevLogEntry['kind']): string {
        switch (kind) {
            case 'invoke': {
                return 'dev-row dev-row-invoke';
            }
            case 'invoke-result': {
                return 'dev-row dev-row-result';
            }
            case 'invoke-error': {
                return 'dev-row dev-row-error';
            }
            case 'event': {
                return 'dev-row dev-row-event';
            }
        }
    }

    function formatTime(ts: number): string {
        const d = new Date(ts);
        const hh = String(d.getHours()).padStart(2, '0');
        const mm = String(d.getMinutes()).padStart(2, '0');
        const ss = String(d.getSeconds()).padStart(2, '0');
        const ms = String(d.getMilliseconds()).padStart(3, '0');
        return `${hh}:${mm}:${ss}.${ms}`;
    }

    function kindGlyph(kind: DevLogEntry['kind']): string {
        switch (kind) {
            case 'invoke': {
                return '→';
            }
            case 'invoke-result': {
                return '←';
            }
            case 'invoke-error': {
                return '✗';
            }
            case 'event': {
                return '⚡';
            }
        }
    }
</script>

<section class="panel dev-panel">
    <h2 class="panel-title">
        <Icon name="bolt" /> Dev — IPC stream
        <span class="dev-badge">dev only</span>
    </h2>

    <div class="dev-counters">
        <div class="dev-counter">
            <span class="dev-counter-label">focus events</span>
            <span class="dev-counter-value" class:dev-counter-zero={focusEventCount === 0}>
                {focusEventCount}
            </span>
        </div>
        <div class="dev-counter">
            <span class="dev-counter-label">profile switches</span>
            <span class="dev-counter-value" class:dev-counter-zero={switchEventCount === 0}>
                {switchEventCount}
            </span>
        </div>
        <div class="dev-counter">
            <span class="dev-counter-label">invoke errors</span>
            <span
                class="dev-counter-value"
                class:dev-counter-bad={invokeErrorCount > 0}
            >
                {invokeErrorCount}
            </span>
        </div>
    </div>

    {#if focusEventCount === 0}
        <p class="muted text-xs dev-hint">
            No focus events received yet. The daemon needs the KWin Script
            installed (<code>data/kwin-script/gamerat-focus</code>) and the
            <code>gamerat-daemon</code> process running with
            <code>--backend auto</code>. Without those, the dispatch loop
            never sees window switches.
        </p>
    {/if}

    <div class="dev-controls">
        <div class="dev-filter-group" role="tablist" aria-label="Filter dev log">
            <button
                class="chip"
                class:chip-active={filterKind === 'all'}
                type="button"
                onclick={() => { filterKind = 'all'; }}
            >
                all
            </button>
            <button
                class="chip"
                class:chip-active={filterKind === 'invoke'}
                type="button"
                onclick={() => { filterKind = 'invoke'; }}
            >
                invokes
            </button>
            <button
                class="chip"
                class:chip-active={filterKind === 'event'}
                type="button"
                onclick={() => { filterKind = 'event'; }}
            >
                events
            </button>
        </div>
        <label class="dev-autoscroll">
            <input type="checkbox" bind:checked={autoscroll} />
            autoscroll
        </label>
        <button class="btn-ghost-sm" type="button" onclick={clearDevLog}>
            clear
        </button>
    </div>

    <div bind:this={logEl} class="dev-log" aria-live="polite">
        {#each entries as entry (entry.id)}
            <div class={classForKind(entry.kind)}>
                <span class="dev-time">{formatTime(entry.ts)}</span>
                <span class="dev-glyph">{kindGlyph(entry.kind)}</span>
                <span class="dev-label">{entry.label}</span>
                {#if entry.elapsedMs !== undefined}
                    <span class="dev-elapsed">{entry.elapsedMs.toFixed(0)}ms</span>
                {/if}
                <span class="dev-preview">{entry.preview}</span>
            </div>
        {:else}
            <p class="muted dev-empty">No IPC traffic yet.</p>
        {/each}
    </div>
</section>
