<script lang="ts">
    import { keycodeFromBrowserCode, nameForKeycode } from './keycode-map.js';
    import { m } from './paraglide/messages.js';
    import { MACRO_EVENT_KIND, type MacroStep } from './types.js';

    /**
     * Live macro recorder. Click Start → captures every keydown /
     * keyup while running, slotting a WAIT step with the delta in
     * milliseconds between consecutive events. Click Stop to bake
     * the result and call `onchange` with the final
     * `MacroStep[]`. The DSL textarea inside `ButtonBindingEditor`
     * stays available for hand-editing afterwards.
     *
     * Stopping is mouse-only (the Stop button) on purpose — every
     * key during the recording window is potentially part of the
     * macro, so we don't use Escape / Enter as terminators.
     */

    interface Props {
        steps: readonly MacroStep[];
        onchange: (steps: readonly MacroStep[]) => void;
    }

    const { steps: initialSteps, onchange }: Props = $props();

    let recording = $state(false);
    let steps = $state<MacroStep[]>([...initialSteps]);
    let lastTs = $state<number | null>(null);
    let unknownCode = $state<string | null>(null);

    function start(): void {
        steps = [];
        lastTs = null;
        unknownCode = null;
        recording = true;
        onchange(steps);
    }

    function stop(): void {
        recording = false;
        onchange(steps);
    }

    function clear(): void {
        steps = [];
        lastTs = null;
        onchange(steps);
    }

    function capture(kind: number, event: KeyboardEvent): void {
        event.preventDefault();
        event.stopPropagation();
        // Skip OS-level autorepeats — they'd spam the macro with
        // press-after-press without releases.
        if (event.repeat) return;
        const keycode = keycodeFromBrowserCode(event.code);
        if (keycode === null) {
            unknownCode = event.code;
            return;
        }
        const ts = performance.now();
        const nextSteps = [...steps];
        if (lastTs !== null) {
            const delta = Math.round(ts - lastTs);
            // Don't emit microscopic waits — the user can't perceive
            // them and ratbagd would discard them anyway.
            if (delta > 4) {
                nextSteps.push({ kind: MACRO_EVENT_KIND.WAIT, value: delta });
            }
        }
        nextSteps.push({ kind, value: keycode });
        steps = nextSteps;
        lastTs = ts;
        onchange(steps);
    }

    function handleKeydown(event: KeyboardEvent): void {
        if (!recording) return;
        capture(MACRO_EVENT_KIND.KEY_PRESS, event);
    }

    function handleKeyup(event: KeyboardEvent): void {
        if (!recording) return;
        capture(MACRO_EVENT_KIND.KEY_RELEASE, event);
    }

    function stepLabel(step: MacroStep): string {
        // ▼/▲ instead of ↓/↑ — line-arrows are too thin to read at
        // the 0.75rem step font. The triangles fill more pixels and
        // are unambiguous even at very small sizes.
        switch (step.kind) {
            case MACRO_EVENT_KIND.KEY_PRESS: {
                return `▼ ${nameForKeycode(step.value)}`;
            }
            case MACRO_EVENT_KIND.KEY_RELEASE: {
                return `▲ ${nameForKeycode(step.value)}`;
            }
            case MACRO_EVENT_KIND.WAIT: {
                return `⏲ ${String(step.value)} ms`;
            }
            default: {
                return `? ${String(step.kind)}:${String(step.value)}`;
            }
        }
    }
</script>

<svelte:window onkeydown={handleKeydown} onkeyup={handleKeyup} />

<div class="macro-recorder">
    <div class="macro-recorder-controls">
        {#if recording}
            <button class="btn-primary macro-recording" type="button" onclick={stop}>
                {m.macro_stop()}
            </button>
            <span class="muted text-xs">{m.macro_capturing()}</span>
        {:else}
            <button class="btn-primary" type="button" onclick={start}>
                {m[steps.length > 0 ? 'macro_rerecord' : 'macro_record']()}
            </button>
            {#if steps.length > 0}
                <button class="btn-ghost-sm" type="button" onclick={clear}>
                    {m.macro_clear()}
                </button>
            {/if}
        {/if}
    </div>

    {#if unknownCode !== null}
        <small class="error-text">{m.macro_unknown_key({ code: unknownCode })}</small>
    {/if}

    <ol class="macro-step-list" aria-label={m.macro_steps_aria()}>
        {#each steps as step, idx (idx)}
            <li class="macro-step font-mono">{stepLabel(step)}</li>
        {:else}
            <li class="muted text-xs macro-step-empty">
                {m[recording ? 'macro_listening' : 'macro_no_steps']()}
            </li>
        {/each}
    </ol>
</div>
