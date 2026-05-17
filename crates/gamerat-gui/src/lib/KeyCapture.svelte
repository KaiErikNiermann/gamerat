<script lang="ts">
    import { keycodeFromBrowserCode, nameForKeycode } from './keycode-map.js';

    /**
     * First-class single-key recorder. Click → arms a global keydown
     * listener; the next keypress captures the Linux keycode and
     * passes it up via `onchange`. We `preventDefault` while
     * recording so accidentally-bound modal-affecting keys (Escape,
     * Tab, Enter) don't close the modal mid-record.
     *
     * Used as the primary KEY-binding input in
     * `ButtonBindingEditor.svelte`. A manual numeric input lives
     * next to it as the fallback / advanced path.
     */

    interface Props {
        keycode: number;
        onchange: (keycode: number) => void;
    }

    const { keycode, onchange }: Props = $props();

    let recording = $state(false);
    let lastUnknown = $state<string | null>(null);

    function start(): void {
        lastUnknown = null;
        recording = true;
    }

    function cancel(): void {
        recording = false;
    }

    function handleKeydown(event: KeyboardEvent): void {
        if (!recording) return;
        // Swallow the event so it doesn't leak into focused inputs /
        // the modal's own keyboard handlers.
        event.preventDefault();
        event.stopPropagation();
        // Browser sometimes fires repeats while a key is held.
        // For single-key capture, take the first non-repeat event.
        if (event.repeat) return;
        const mapped = keycodeFromBrowserCode(event.code);
        if (mapped === null) {
            lastUnknown = event.code;
            // Stay in recording mode so the user can try again.
            return;
        }
        recording = false;
        onchange(mapped);
    }
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="key-capture">
    {#if recording}
        <button
            class="btn-primary key-capture-armed"
            type="button"
            onclick={cancel}
            aria-live="polite"
        >
            Press any key… (click to cancel)
        </button>
        {#if lastUnknown !== null}
            <small class="error-text">
                Unknown key <code>{lastUnknown}</code> — not in the lookup table.
                Use the manual input below as a fallback.
            </small>
        {/if}
    {:else}
        <button class="btn-ghost-sm key-capture-record" type="button" onclick={start}>
            Record key
        </button>
        <span class="key-capture-current font-mono">
            {nameForKeycode(keycode)}
            <small class="muted">({keycode})</small>
        </span>
    {/if}
</div>
