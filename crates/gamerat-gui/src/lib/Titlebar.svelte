<script lang="ts">
    /**
     * Custom window titlebar. Replaces the native decorations
     * (`decorations: false` in tauri.conf.json) so the chrome can pick
     * up our theme tokens — the OS-native bar otherwise sticks out as
     * a thick light-grey forehead on Plasma.
     *
     * Layout:
     *   [ rat icon ]  gamerat                         _  □  ✕
     *                       <— drag region —>
     *
     * Buttons go through @tauri-apps/api/window so they work
     * uniformly across platforms / compositors.
     */

    import Rat from '@lucide/svelte/icons/rat';
    import Minimize2 from '@lucide/svelte/icons/minus';
    import Maximize2 from '@lucide/svelte/icons/square';
    import Close from '@lucide/svelte/icons/x';
    import { getCurrentWindow } from '@tauri-apps/api/window';

    const appWindow = getCurrentWindow();

    async function handleMinimize(): Promise<void> {
        await appWindow.minimize();
    }

    async function handleMaximize(): Promise<void> {
        // toggleMaximize handles maximize ↔ unmaximize transparently.
        await appWindow.toggleMaximize();
    }

    async function handleClose(): Promise<void> {
        await appWindow.close();
    }
</script>

<header class="titlebar" data-tauri-drag-region>
    <span class="titlebar-brand" data-tauri-drag-region>
        <span class="titlebar-rat" aria-hidden="true">
            <Rat size={14} />
        </span>
        <span class="titlebar-title">gamerat</span>
    </span>

    <div class="titlebar-spacer" data-tauri-drag-region></div>

    <div class="titlebar-controls">
        <button
            type="button"
            class="titlebar-btn"
            aria-label="Minimize"
            title="Minimize"
            onclick={() => { void handleMinimize(); }}
        >
            <Minimize2 size={14} />
        </button>
        <button
            type="button"
            class="titlebar-btn"
            aria-label="Maximize"
            title="Maximize"
            onclick={() => { void handleMaximize(); }}
        >
            <Maximize2 size={12} />
        </button>
        <button
            type="button"
            class="titlebar-btn titlebar-btn-close"
            aria-label="Close"
            title="Close"
            onclick={() => { void handleClose(); }}
        >
            <Close size={14} />
        </button>
    </div>
</header>
