<script lang="ts">
    /**
     *  Modal backdrop + accessibility shell shared by every modal in
     *  the app. Wraps the standard `.binding-editor-backdrop` element
     *  with the boilerplate every dialog otherwise re-implements:
     *  role + aria-modal + the click-outside + Escape-to-close handlers.
     *
     *  Caller passes the card markup as children (a form, a div, or a
     *  bare panel — anything). The parent owns the close callback;
     *  any per-dialog guard (e.g. "don't close while a write is in
     *  flight") lives in the callback itself, not here.
     */
    import type { Snippet } from 'svelte';

    interface Props {
        /** aria-label for the backdrop. Required — every dialog should
         *  surface a meaningful label for screen readers. */
        label: string;
        /** Fired on click-outside-the-card or Escape. Guards (busy
         *  state, dirty form, etc.) belong in the parent's handler so
         *  the Modal stays single-purpose. */
        onclose: () => void;
        children: Snippet;
    }

    const { label, onclose, children }: Props = $props();
</script>

<div
    class="binding-editor-backdrop"
    role="dialog"
    aria-modal="true"
    aria-label={label}
    onclick={(e) => {
        if (e.target === e.currentTarget) onclose();
    }}
    onkeydown={(e) => {
        if (e.key === 'Escape') onclose();
    }}
    tabindex="-1"
>
    {@render children()}
</div>
