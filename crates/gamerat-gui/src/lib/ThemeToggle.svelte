<script lang="ts">
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
        <!-- monitor / display icon -->
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <rect x="1.5" y="2.5" width="13" height="9" rx="1" fill="none" stroke="currentColor" stroke-width="1.3" />
            <line x1="5" y1="13.5" x2="11" y2="13.5" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" />
        </svg>
    {:else if theme === 'light'}
        <!-- sun icon -->
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <circle cx="8" cy="8" r="3" fill="currentColor" />
            <g stroke="currentColor" stroke-width="1.3" stroke-linecap="round">
                <line x1="8" y1="1.5" x2="8" y2="3" />
                <line x1="8" y1="13" x2="8" y2="14.5" />
                <line x1="1.5" y1="8" x2="3" y2="8" />
                <line x1="13" y1="8" x2="14.5" y2="8" />
                <line x1="3.3" y1="3.3" x2="4.4" y2="4.4" />
                <line x1="11.6" y1="11.6" x2="12.7" y2="12.7" />
                <line x1="3.3" y1="12.7" x2="4.4" y2="11.6" />
                <line x1="11.6" y1="4.4" x2="12.7" y2="3.3" />
            </g>
        </svg>
    {:else}
        <!-- moon icon -->
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <path
                d="M11.5 10.2A5 5 0 0 1 5.8 4.5a5.7 5.7 0 0 0-2.3 4.5 5.7 5.7 0 0 0 5.7 5.7 5.7 5.7 0 0 0 4.5-2.3 5 5 0 0 1-2.2-2.2Z"
                fill="currentColor"
            />
        </svg>
    {/if}
</button>
