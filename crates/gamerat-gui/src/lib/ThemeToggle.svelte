<script lang="ts">
    import Monitor from '@lucide/svelte/icons/monitor';
    import Moon from '@lucide/svelte/icons/moon';
    import Sun from '@lucide/svelte/icons/sun';
    import { applyTheme, loadTheme, nextTheme, saveTheme, type Theme } from './theme.js';

    let theme = $state<Theme>(loadTheme());

    // Apply once on mount and again whenever the user clicks.
    $effect(() => {
        applyTheme(theme);
        saveTheme(theme);
    });

    function cycle(): void {
        theme = nextTheme(theme);
    }

    function label(t: Theme): string {
        if (t === 'system') return 'theme: follow system';
        if (t === 'light') return 'theme: light';
        return 'theme: dark';
    }
</script>

<button
    type="button"
    class="theme-toggle"
    onclick={cycle}
    title={label(theme)}
    aria-label={label(theme)}
>
    {#if theme === 'system'}
        <Monitor size={14} />
    {:else if theme === 'light'}
        <Sun size={14} />
    {:else}
        <Moon size={14} />
    {/if}
</button>
