<script lang="ts">
    /**
     * Project-wide icon shim backed by `@lucide/svelte`. Callers stay
     * on the existing `<Icon name="..." />` API; the registry below
     * maps our internal names to the concrete lucide components so a
     * package swap is one-file. Glyphs inherit `currentColor` from the
     * surrounding text.
     */

    import Bolt from '@lucide/svelte/icons/zap';
    import Clipboard from '@lucide/svelte/icons/clipboard-list';
    import Gamepad from '@lucide/svelte/icons/gamepad-2';
    import Gear from '@lucide/svelte/icons/settings';
    import InfoIcon from '@lucide/svelte/icons/info';
    import MouseIcon from '@lucide/svelte/icons/mouse';
    import Radio from '@lucide/svelte/icons/radio';
    import Target from '@lucide/svelte/icons/crosshair';

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

<span class="icon" aria-hidden="true">
    {#if name === 'mouse'}
        <MouseIcon {size} />
    {:else if name === 'gear'}
        <Gear {size} />
    {:else if name === 'bolt'}
        <Bolt {size} />
    {:else if name === 'clipboard'}
        <Clipboard {size} />
    {:else if name === 'radio'}
        <Radio {size} />
    {:else if name === 'gamepad'}
        <Gamepad {size} />
    {:else if name === 'target'}
        <Target {size} />
    {:else if name === 'info'}
        <InfoIcon {size} />
    {/if}
</span>

<style>
    .icon {
        display: inline-flex;
        align-items: center;
        vertical-align: -2px;
        flex-shrink: 0;
        color: inherit;
    }
</style>
