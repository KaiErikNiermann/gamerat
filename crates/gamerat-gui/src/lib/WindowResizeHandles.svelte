<script lang="ts">
    /**
     * Invisible edge / corner resize handles for the borderless window
     * (`decorations: false` strips the OS-provided ones too). Each
     * handle dispatches `startResizeDragging(direction)` on mousedown.
     *
     * Stacking: rendered above everything else as a fixed overlay; the
     * inner area is `pointer-events: none` so the page underneath is
     * still clickable. Only the thin edge / corner strips intercept
     * pointer events.
     *
     * Tao implicit-resize workaround — the real fix:
     *
     * Tao (Tauri's windowing library) attaches a GTK-level
     * `connect_button_press_event` handler that auto-triggers
     * `begin_resize_drag` whenever a click lands within 5 logical
     * pixels of any window edge. On Wayland the gate is
     * `is_wayland || !is_decorated`, so on a Wayland session the
     * handler *always* fires for our undecorated window. That fires
     * before the webview sees the click — JS-side guards can't
     * intervene — and was the actual root cause of the
     * scrollbar-gets-stuck bug we chased through several earlier
     * commits.
     *
     * Strategy:
     *
     *   1. Keep `resizable: false` permanently in `tauri.conf.json` so
     *      Tao's implicit handler stays inert from window creation.
     *      No race window at startup.
     *   2. To start an *explicit* resize from one of our edge strips,
     *      temporarily flip `setResizable(true)` then call
     *      `startResizeDragging(direction)`. Both IPC requests travel
     *      the same tao event-loop channel in FIFO order, so
     *      `set_resizable(true)` hits GTK before `begin_resize_drag`.
     *   3. The mouseup listener attached BEFORE the IPC calls flips
     *      `setResizable(false)` back as soon as the user releases —
     *      with a 5-second timeout fallback in case GTK's pointer grab
     *      swallows the mouseup we'd otherwise see.
     *
     * The user can't race the toggle because they're holding the
     * mouse button down for the duration of the drag — no new
     * mousedown event fires until the drag ends, by which point
     * we've already flipped resizable back to false.
     *
     * Capability gotcha: `setResizable` is gated by Tauri's permission
     * system. `core:window:allow-set-resizable` MUST be present in
     * `capabilities/default.json` or every call rejects with
     * "Permissions associated with this command:
     * core:window:allow-set-resizable" and the resize silently fails.
     *
     * Known side effect: double-click on the titlebar drag region no
     * longer maximises. The Tauri command behind that gesture,
     * `internal_toggle_maximize`, is gated by `is_resizable`. The
     * explicit maximise button in our titlebar still works because it
     * goes through `toggle_maximize`, which has no such gate.
     */

    import { getCurrentWindow } from '@tauri-apps/api/window';

    type Direction =
        | 'North'
        | 'South'
        | 'East'
        | 'West'
        | 'NorthEast'
        | 'NorthWest'
        | 'SouthEast'
        | 'SouthWest';

    const appWindow = getCurrentWindow();

    async function runResize(direction: Direction): Promise<void> {
        // Attach the restore listener FIRST. The GTK resize-drag may
        // grab the pointer and consume the mouseup that ends the
        // drag, so we register both a `mouseup` listener (capture
        // phase, on document AND window — different platforms
        // deliver the post-grab mouseup differently) and a long
        // fallback timer. Belt-and-braces; we must not leak the
        // resizable=true state.
        let restored = false;
        const restore = (): void => {
            if (restored) return;
            restored = true;
            document.removeEventListener('mouseup', restore, true);
            globalThis.removeEventListener('mouseup', restore, true);
            clearTimeout(fallbackTimer);
            void appWindow.setResizable(false);
        };
        const fallbackTimer = setTimeout(restore, 5000);
        document.addEventListener('mouseup', restore, true);
        globalThis.addEventListener('mouseup', restore, true);

        // Flip resizable ON, kick off the drag. Both IPC requests
        // travel through the same tao event-loop channel in FIFO
        // order, so set_resizable(true) hits GTK before
        // begin_resize_drag does — no race window for the explicit
        // call. Awaiting `setResizable` requires the
        // `core:window:allow-set-resizable` capability in
        // `capabilities/default.json`; without it the call rejects
        // silently and the drag fails. Don't ask me how I know.
        try {
            await appWindow.setResizable(true);
            await appWindow.startResizeDragging(direction);
        } catch (error) {
            restore();
            throw error;
        }
    }

    function startResize(direction: Direction): (e: MouseEvent) => void {
        return (e: MouseEvent) => {
            // Only react to the primary button — secondary clicks
            // (context menu) shouldn't trigger a resize gesture.
            if (e.button !== 0) return;
            e.preventDefault();
            void runResize(direction);
        };
    }
</script>

<div class="resize-overlay" aria-hidden="true">
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="resize-edge resize-n"    onmousedown={startResize('North')}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="resize-edge resize-s"    onmousedown={startResize('South')}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="resize-edge resize-e"    onmousedown={startResize('East')}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="resize-edge resize-w"    onmousedown={startResize('West')}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="resize-corner resize-ne" onmousedown={startResize('NorthEast')}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="resize-corner resize-nw" onmousedown={startResize('NorthWest')}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="resize-corner resize-se" onmousedown={startResize('SouthEast')}></div>
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="resize-corner resize-sw" onmousedown={startResize('SouthWest')}></div>
</div>
