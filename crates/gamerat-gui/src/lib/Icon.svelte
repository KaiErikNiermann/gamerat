<script lang="ts">
    /**
     * Inline SVG icon set. One-stop registry so we don't sprinkle raw
     * <svg> blocks across every panel header. All glyphs draw from the
     * stroke=currentColor convention — colour follows the surrounding
     * text token, no per-theme variants.
     */

    export type IconName =
        | 'mouse'
        | 'gear'
        | 'bolt'
        | 'clipboard'
        | 'radio'
        | 'gamepad'
        | 'target'
        | 'info';

    interface Props {
        name: IconName;
        size?: number;
    }

    const { name, size = 14 }: Props = $props();
</script>

<svg
    class="icon"
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    stroke-width="1.8"
    stroke-linecap="round"
    stroke-linejoin="round"
    aria-hidden="true"
>
    {#if name === 'mouse'}
        <rect x="6" y="3" width="12" height="18" rx="6" />
        <line x1="12" y1="7" x2="12" y2="11" />
    {:else if name === 'gear'}
        <circle cx="12" cy="12" r="3" />
        <path
            d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1A2 2 0 1 1 4.4 17l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1 1.7 1.7 0 0 0-.3-1.8l-.1-.1A2 2 0 1 1 7 4.4l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1Z"
        />
    {:else if name === 'bolt'}
        <polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2" fill="currentColor" stroke="none" />
    {:else if name === 'clipboard'}
        <rect x="8" y="3" width="8" height="3" rx="0.5" />
        <path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2" />
    {:else if name === 'radio'}
        <circle cx="12" cy="12" r="2" />
        <path d="M16.2 7.8a6 6 0 0 1 0 8.4M7.8 16.2a6 6 0 0 1 0-8.4" />
        <path d="M19 5a10 10 0 0 1 0 14M5 19A10 10 0 0 1 5 5" />
    {:else if name === 'gamepad'}
        <line x1="6" y1="10" x2="10" y2="10" />
        <line x1="8" y1="8" x2="8" y2="12" />
        <circle cx="15.5" cy="11" r="0.5" fill="currentColor" />
        <circle cx="17.5" cy="13" r="0.5" fill="currentColor" />
        <path d="M17.32 5H6.68A5.68 5.68 0 0 0 1 10.68V14a4 4 0 0 0 7.32 2.26L9.5 14h5l1.18 2.26A4 4 0 0 0 23 14v-3.32A5.68 5.68 0 0 0 17.32 5Z" />
    {:else if name === 'target'}
        <circle cx="12" cy="12" r="9" />
        <circle cx="12" cy="12" r="5" />
        <circle cx="12" cy="12" r="1.2" fill="currentColor" />
    {:else if name === 'info'}
        <circle cx="12" cy="12" r="9" />
        <line x1="12" y1="11" x2="12" y2="16" />
        <circle cx="12" cy="8" r="0.6" fill="currentColor" />
    {/if}
</svg>

<style>
    .icon {
        display: inline-block;
        vertical-align: -2px;
        flex-shrink: 0;
    }
</style>
