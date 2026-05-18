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
            e.preventDefault();
            void appWindow.startResizeDragging(direction);
        };
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
