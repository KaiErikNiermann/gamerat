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
     * `connect_button_press_event` handler on Linux that auto-triggers
     * `begin_resize_drag` whenever a click lands within 5 logical
     * pixels of any window edge, gated by
     * `!is_decorated() && is_resizable() && !is_maximized()`. That fires
     * *before* the webview sees the click — JS-side guards can't
     * intervene — and was the actual root cause of the
     * scrollbar-gets-stuck bug we chased through three other commits.
     *
     * `is_decorated() = false` is a hard requirement (we want a custom
     * titlebar) and `is_maximized()` is user-controlled, so the only
     * flag we can lever is `is_resizable`. Strategy: keep it OFF in
     * the default state (so Tao's auto-handler stays inert), and only
     * flip it ON for the duration of an explicit resize drag started
     * from one of our edge strips. The two IPC calls
     * (`setResizable(true)` + `startResizeDragging`) hit the same tao
     * event-loop channel in FIFO order, so by the time
     * `begin_resize_drag` runs at the GTK layer, `is_resizable` has
     * already updated. The user can't race the toggle because they're
     * holding the mouse button down for the duration of the drag —
     * no new mousedown event fires until the drag ends, by which point
     * we've flipped resizable back to false.
     *
     * Side effect: double-click-on-titlebar-to-maximise no longer
     * works (Tauri's `internal_toggle_maximize` is gated by
     * `is_resizable`). The explicit maximise button in the titlebar
     * still works because it goes through `toggle_maximize`, which has
     * no such gate.
     */

    import { onMount } from 'svelte';
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

    onMount(() => {
        // Disable resizable at startup so Tao's auto-resize handler
        // stays inert. `tauri.conf.json` keeps `resizable: true` so
        // window creation / WM hints are normal; we flip it off as
        // soon as the JS layer comes up. Fire-and-forget — if it
        // fails, the worst case is that the implicit 5 px hit-test
        // remains active and we fall back to the old buggy
        // behaviour.
        void appWindow.setResizable(false);
    });

    async function runResize(direction: Direction): Promise<void> {
        // Flip resizable ON, kick off the drag, schedule flipping
        // it back OFF when the user releases. Order matters: both
        // requests go through the same tao event-loop channel in
        // FIFO order, so set_resizable(true) hits GTK before
        // begin_resize_drag does.
        await appWindow.setResizable(true);
        await appWindow.startResizeDragging(direction);

        // Restore resizable=false once the drag ends. GTK takes
        // over the pointer during the drag so the mouseup might
        // not reach the webview reliably — listen on document
        // (capture) AND fall back to a long timeout so we can't
        // get stuck in resizable=true.
        let restored = false;
        const restore = (): void => {
            if (restored) return;
            restored = true;
            document.removeEventListener('mouseup', restore, true);
            clearTimeout(fallbackTimer);
            void appWindow.setResizable(false);
        };
        const fallbackTimer = setTimeout(restore, 5000);
        document.addEventListener('mouseup', restore, true);
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
