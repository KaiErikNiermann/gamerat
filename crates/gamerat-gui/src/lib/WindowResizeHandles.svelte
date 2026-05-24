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
     *      Tao's implicit handler stays inert from window creation —
     *      no race window at startup.
     *   2. To start an explicit resize from one of our edge strips,
     *      invoke the custom Rust command `start_explicit_resize_drag`,
     *      which atomically flips `set_resizable(true)` and then calls
     *      `start_resize_dragging(direction)` on the same tokio task.
     *      Doing this as two separate JS-side IPC calls let tokio
     *      interleave the futures and sometimes ran
     *      `begin_resize_drag` before `set_resizable(true)` had landed
     *      at the GTK main thread — the drag would silently fail.
     *   3. Drag-end detection uses Tauri's `onResized` event with a
     *      200 ms idle window. `mouseup` is unreliable here because
     *      webkit dispatches a *synthetic* mouseup when GTK takes the
     *      pointer grab in `begin_resize_drag`, which used to fire
     *      our restore mid-drag and abort the resize.
     *   4. Once the user stops dragging (no resize events for 200 ms)
     *      we flip `setResizable(false)` back. A 2 s hard timeout
     *      covers the click-without-drag case (no resize events at all).
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

    import { invoke } from '@tauri-apps/api/core';
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

    /** Milliseconds without an `onResized` event after which we assume
     *  the user has stopped dragging and flip resizable back off.
     *  Lower = faster snap-back (smaller window where Tao's implicit
     *  handler could re-arm); higher = more tolerance for users who
     *  pause mid-drag. 200ms is comfortably below human re-click
     *  latency. */
    const RESIZE_IDLE_RESTORE_MS = 200;

    /** Hard ceiling in case the user mousedowns on an edge but never
     *  actually drags (no resize events fire). We still restore
     *  resizable=false eventually to keep the bug fix intact. */
    const RESIZE_HARD_TIMEOUT_MS = 2000;

    async function runResize(direction: Direction): Promise<void> {
        let restored = false;
        let idleTimer: ReturnType<typeof setTimeout> | null = null;
        let unlistenResize: (() => void) | null = null;

        const restore = (): void => {
            if (restored) return;
            restored = true;
            if (idleTimer !== null) clearTimeout(idleTimer);
            clearTimeout(hardTimer);
            unlistenResize?.();
            void appWindow.setResizable(false);
        };

        // Drag-end detection via Tauri's `onResized` event:
        //
        // We used to listen for `mouseup` on the document, but webkit
        // dispatches a synthetic mouseup when GTK takes the pointer
        // grab in `begin_resize_drag` — which fired our restore
        // mid-drag and aborted the resize before it had visibly
        // happened. `onResized` only fires on real geometry changes,
        // so we can't be tricked by synthetic input events. The idle
        // window catches the trailing edge: when resize events stop
        // arriving for ~200 ms, the user has let go and we restore.
        //
        // Hard timeout backstop covers the no-movement case (user
        // clicked an edge but never dragged → no resize events at
        // all → no idle timer ever armed). 2 s is short enough that
        // a stray click can't leak resizable=true for long.
        unlistenResize = await appWindow.onResized(() => {
            if (idleTimer !== null) clearTimeout(idleTimer);
            idleTimer = setTimeout(restore, RESIZE_IDLE_RESTORE_MS);
        });
        const hardTimer = setTimeout(restore, RESIZE_HARD_TIMEOUT_MS);

        // Atomic enable + start-drag via a single custom Tauri
        // command so they run serially on the same tokio task. Two
        // separate `appWindow.setResizable + startResizeDragging`
        // calls let tokio interleave their futures, sometimes
        // running `begin_resize_drag` before `set_resizable(true)`
        // had reached the GTK main thread — and the drag silently
        // failed. One IPC round-trip, one ordered handler, no race.
        try {
            await invoke('start_explicit_resize_drag', { direction });
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
