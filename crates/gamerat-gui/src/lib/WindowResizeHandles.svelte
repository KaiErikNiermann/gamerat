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
     *   3. Drag-end detection keys off the real `mouseup` event at the
     *      `document` level (capture phase). Idle-based detection via
     *      `onResized` was tried first but turned out to be the wrong
     *      abstraction — if the user paused mid-drag (cursor stops,
     *      so no `onResized` for a beat), the idle timer would fire
     *      and pull `setResizable(false)` out from under the
     *      still-active drag. The state we actually care about is
     *      "is the button still held", and `mouseup` is what reports
     *      that.
     *   4. A 30 s ceiling backstops the case where `mouseup` never
     *      arrives at all (GTK's pointer grab can theoretically
     *      swallow it on some compositors). In practice this never
     *      fires; it's only there so a lost mouseup can't permanently
     *      leak `resizable: true` and re-arm Tao's implicit handler.
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

    /** Single source of truth for every handle: the Tauri direction
     *  name we pass to the Rust command, the CSS class-name suffix
     *  (matches `.resize-n` / `.resize-se` / … rules in app.css), and
     *  whether it's an edge strip or a corner square. The template
     *  at the bottom iterates this array once instead of repeating
     *  eight near-identical `<div>` rows. */
    const HANDLES: readonly {
        direction: Direction;
        suffix: string;
        kind: 'edge' | 'corner';
    }[] = [
        { direction: 'North',     suffix: 'n',  kind: 'edge'   },
        { direction: 'South',     suffix: 's',  kind: 'edge'   },
        { direction: 'East',      suffix: 'e',  kind: 'edge'   },
        { direction: 'West',      suffix: 'w',  kind: 'edge'   },
        { direction: 'NorthEast', suffix: 'ne', kind: 'corner' },
        { direction: 'NorthWest', suffix: 'nw', kind: 'corner' },
        { direction: 'SouthEast', suffix: 'se', kind: 'corner' },
        { direction: 'SouthWest', suffix: 'sw', kind: 'corner' },
    ];

    const appWindow = getCurrentWindow();

    /** Backstop for the case where the `mouseup` we'd otherwise wait
     *  on never arrives (GTK's pointer grab can swallow it on some
     *  compositors / drivers). Long on purpose — in normal usage
     *  `mouseup` fires reliably, so this never matters; it's only here
     *  so a lost mouseup can't leak `resizable: true` indefinitely. */
    const RESIZE_SAFETY_TIMEOUT_MS = 30_000;

    async function runResize(direction: Direction): Promise<void> {
        let restored = false;

        const restore = (): void => {
            console.debug('Restoring resizable: false', restored);
            if (restored) return;
            restored = true;
            clearTimeout(safetyTimer);
            document.removeEventListener('mouseup', restore, true);
            globalThis.removeEventListener('mouseup', restore, true);
            console.debug('Resize drag ended; restoring resizable: false');
            void appWindow.setResizable(false);
        };

        // Drag-end detection: just listen for the real `mouseup`.
        //
        // The state we actually want to track is "is the user still
        // holding the button?" — that's what dictates whether the
        // resize should stay enabled. `mouseup` reports the button
        // release directly, so we use it directly. Capture phase on
        // both `document` and `globalThis` covers the platforms that
        // deliver the post-grab mouseup to different targets.
        //
        // Previous attempts at idle-timer-based detection (via
        // Tauri's `onResized` event with a "no resize for 250 ms ⇒
        // user let go" rule) fell over the moment the user paused
        // mid-drag — cursor stops, no resize events, idle timer
        // fires, and we'd stomp `setResizable(false)` while the user
        // was still actively dragging. State, not silence.
        document.addEventListener('mouseup', restore, true);
        globalThis.addEventListener('mouseup', restore, true);
        const safetyTimer = setTimeout(restore, RESIZE_SAFETY_TIMEOUT_MS);

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
            console.debug(`Starting resize drag in direction ${direction}`);
            void runResize(direction);
        };
    }
</script>

<div class="resize-overlay" aria-hidden="true">
    {#each HANDLES as { direction, suffix, kind } (direction)}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
            class="resize-{kind} resize-{suffix}"
            onmousedown={startResize(direction)}
        ></div>
    {/each}
</div>
