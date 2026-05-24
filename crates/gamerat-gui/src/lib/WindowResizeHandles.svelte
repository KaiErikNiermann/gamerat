<script lang="ts">
    /**
     * Invisible edge / corner resize handles for the borderless window
     * (`decorations: false` strips the OS-provided ones too). Each handle
     * dispatches `startResizeDragging(direction)` on mousedown.
     *
     * Stacking: rendered above everything else as a fixed overlay; the
     * inner area is `pointer-events: none` so the page underneath is
     * still clickable. Only the thin edge / corner strips intercept
     * pointer events.
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

    function startResize(direction: Direction): (e: MouseEvent) => void {
        return (e: MouseEvent) => {
            // Only react to the primary button — secondary clicks
            // (context menu) shouldn't trigger a resize gesture.
            if (e.button !== 0) return;
            // The right-edge resize strip sits directly on top of the
            // rightmost 4 pixels of the page's scrollbar (same for the
            // bottom strip + any horizontal scrollbar). Without a guard
            // here, clicking inside that overlap captures the mousedown,
            // hands the mouse off to the OS window manager via
            // startResizeDragging, and the matching mouseup arrives at
            // the OS — never at the browser. The scrollbar then stays
            // pinned in its "thumb pressed" state until something else
            // breaks the lock (another LMB click, MMB, whatever). Bail
            // out before kicking off the OS resize so the gesture is a
            // no-op instead of a stuck-state trap.
            if (isClickOnScrollbar(e)) return;
            e.preventDefault();
            void appWindow.startResizeDragging(direction);
        };
    }

    /**
     * Walk the elements stacked at the cursor's position (skipping our
     * own resize overlay) and return `true` if the cursor sits inside
     * the scrollbar gutter of any of them.
     *
     * Detection: an element with a visible vertical scrollbar has
     * `offsetWidth > clientWidth`; the difference is the scrollbar's
     * width and it lives at the inline-end of the element (right side
     * in LTR). Horizontal scrollbars are the dual case along
     * `offsetHeight` / `clientHeight`. Walking the elementsFromPoint
     * stack (rather than just the bottom element) catches the case
     * where the cursor sits over a child of a scrollable ancestor but
     * still falls inside the ancestor's scrollbar gutter.
     */
    function isClickOnScrollbar(e: MouseEvent): boolean {
        const stack = document.elementsFromPoint(e.clientX, e.clientY);
        for (const el of stack) {
            if (!(el instanceof HTMLElement)) continue;
            // Skip our own overlay — its strips are above the page
            // content in z-order so they'd otherwise mask the
            // scrollable element underneath.
            if (
                el.classList.contains('resize-edge') ||
                el.classList.contains('resize-corner') ||
                el.classList.contains('resize-overlay')
            ) {
                continue;
            }
            const sbW = el.offsetWidth - el.clientWidth;
            const sbH = el.offsetHeight - el.clientHeight;
            if (sbW === 0 && sbH === 0) continue;
            const rect = el.getBoundingClientRect();
            if (sbW > 0 && e.clientX >= rect.right - sbW) return true;
            if (sbH > 0 && e.clientY >= rect.bottom - sbH) return true;
        }
        return false;
    }
</script>

<div class="resize-overlay" aria-hidden="true">
    <div class="resize-edge resize-n"    onmousedown={startResize('North')}></div>
    <div class="resize-edge resize-s"    onmousedown={startResize('South')}></div>
    <div class="resize-edge resize-e"    onmousedown={startResize('East')}></div>
    <div class="resize-edge resize-w"    onmousedown={startResize('West')}></div>
    <div class="resize-corner resize-ne" onmousedown={startResize('NorthEast')}></div>
    <div class="resize-corner resize-nw" onmousedown={startResize('NorthWest')}></div>
    <div class="resize-corner resize-se" onmousedown={startResize('SouthEast')}></div>
    <div class="resize-corner resize-sw" onmousedown={startResize('SouthWest')}></div>
</div>
