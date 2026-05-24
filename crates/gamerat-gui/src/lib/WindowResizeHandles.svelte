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
     * Scrollbar passthrough: the resize strips sit directly at the
     * window's `right: 0 / bottom: 0` edges, where scrollbars of any
     * full-bleed scrollable element also live. Without intervention,
     * mousedown on the rightmost few pixels of a scrollbar gets
     * captured by `.resize-e`, hands the mouse off to the OS window
     * manager via `startResizeDragging`, and the matching mouseup
     * never reaches the browser — leaving the scrollbar pinned in
     * its "thumb pressed" state. Hover-based detection here mirrors
     * Firefox's native behaviour: while the cursor sits over any
     * scrollable element's scrollbar hot zone, the strips get
     * `pointer-events: none` and the scrollbar takes the
     * interaction (cursor + click) cleanly.
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

    /** Width of the band along a scrollable element's right /
     *  bottom edge that we treat as "the scrollbar". webkit2gtk's
     *  overlay scrollbars expand to ~12 px on hover; 14 gives a
     *  small safety margin without eating measurably into the
     *  resize strips' (4 px wide) grab zone. */
    const SCROLLBAR_HOTZONE_PX = 14;

    /** Cursor is currently inside some element's scrollbar hot zone.
     *  Drives the `.resize-overlay-scrollbar-passthrough` class on
     *  the overlay below, which switches every strip to
     *  `pointer-events: none`. */
    let scrollbarHovered = $state(false);

    function isResizeOverlayChild(el: HTMLElement): boolean {
        return (
            el.classList.contains('resize-edge') ||
            el.classList.contains('resize-corner') ||
            el.classList.contains('resize-overlay')
        );
    }

    function hotZoneHit(el: HTMLElement, x: number, y: number): boolean {
        // We rely on geometric position rather than `offsetWidth -
        // clientWidth` because that delta is zero for webkit2gtk's
        // overlay scrollbars even when one's clearly visible — the
        // same trap that broke the previous JS attempt.
        const style = getComputedStyle(el);
        const scrollableY =
            (style.overflowY === 'auto' || style.overflowY === 'scroll') &&
            el.scrollHeight > el.clientHeight;
        const scrollableX =
            (style.overflowX === 'auto' || style.overflowX === 'scroll') &&
            el.scrollWidth > el.clientWidth;
        if (!scrollableY && !scrollableX) return false;

        const rect = el.getBoundingClientRect();
        if (scrollableY) {
            const inRightBand =
                x <= rect.right &&
                x >= rect.right - SCROLLBAR_HOTZONE_PX &&
                y >= rect.top &&
                y <= rect.bottom;
            if (inRightBand) return true;
        }
        if (scrollableX) {
            const inBottomBand =
                y <= rect.bottom &&
                y >= rect.bottom - SCROLLBAR_HOTZONE_PX &&
                x >= rect.left &&
                x <= rect.right;
            if (inBottomBand) return true;
        }
        return false;
    }

    function isCursorOverScrollbar(x: number, y: number): boolean {
        // Walk every element painted at the cursor's coords, skipping
        // our own overlay strips (so we see what's underneath even
        // when the cursor is hovering directly on a resize edge).
        // For each candidate, `hotZoneHit` does the actual scrollable
        // + geometry check.
        const stack = document.elementsFromPoint(x, y);
        for (const el of stack) {
            if (!(el instanceof HTMLElement)) continue;
            if (isResizeOverlayChild(el)) continue;
            if (hotZoneHit(el, x, y)) return true;
        }
        return false;
    }

    function onMouseMove(e: MouseEvent): void {
        // Cheap pre-filter: the conflict only matters when the cursor
        // is near a viewport edge (where the resize strips live).
        // Otherwise short-circuit the elementsFromPoint walk and just
        // make sure we're not stuck in the passthrough state.
        const nearRight =
            e.clientX > globalThis.innerWidth - SCROLLBAR_HOTZONE_PX - 1;
        const nearBottom =
            e.clientY > globalThis.innerHeight - SCROLLBAR_HOTZONE_PX - 1;
        if (!nearRight && !nearBottom) {
            if (scrollbarHovered) scrollbarHovered = false;
            return;
        }
        const over = isCursorOverScrollbar(e.clientX, e.clientY);
        if (over !== scrollbarHovered) scrollbarHovered = over;
    }

    onMount(() => {
        document.addEventListener('mousemove', onMouseMove, { passive: true });
        return () => {
            document.removeEventListener('mousemove', onMouseMove);
        };
    });

    function startResize(direction: Direction): (e: MouseEvent) => void {
        return (e: MouseEvent) => {
            // Only react to the primary button — secondary clicks
            // (context menu) shouldn't trigger a resize gesture.
            if (e.button !== 0) return;
            // Belt-and-braces guard for the very-fast-cursor case
            // where a mousedown lands before mousemove has had a
            // chance to flip `scrollbarHovered` (cursor warps, etc.).
            // The passthrough class above is the primary mechanism;
            // this is the safety net.
            if (isCursorOverScrollbar(e.clientX, e.clientY)) return;
            e.preventDefault();
            void appWindow.startResizeDragging(direction);
        };
    }
</script>

<div
    class="resize-overlay"
    class:resize-overlay-scrollbar-passthrough={scrollbarHovered}
    aria-hidden="true"
>
    <div class="resize-edge resize-n"    onmousedown={startResize('North')}></div>
    <div class="resize-edge resize-s"    onmousedown={startResize('South')}></div>
    <div class="resize-edge resize-e"    onmousedown={startResize('East')}></div>
    <div class="resize-edge resize-w"    onmousedown={startResize('West')}></div>
    <div class="resize-corner resize-ne" onmousedown={startResize('NorthEast')}></div>
    <div class="resize-corner resize-nw" onmousedown={startResize('NorthWest')}></div>
    <div class="resize-corner resize-se" onmousedown={startResize('SouthEast')}></div>
    <div class="resize-corner resize-sw" onmousedown={startResize('SouthWest')}></div>
</div>
