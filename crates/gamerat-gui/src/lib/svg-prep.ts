/**
 * Prepare an upstream mouse SVG for inline rendering in the hero
 * panel. Pure DOM op so it's testable without spinning up the full
 * MouseView component.
 *
 * What this does:
 *   - `overflow="visible"` so leader rects sitting just outside the
 *     SVG's canonical viewBox aren't clipped (without it the
 *     `getBoundingClientRect` reads come back 0×0 and the HTML
 *     leader-labels never get placed — see `MouseView.svelte`).
 *   - Inline styles that size the SVG responsively inside the hero
 *     panel.
 *
 * What it deliberately does NOT do:
 *   - `removeAttribute('width' | 'height')` on an `<svg>`. WebKit
 *     logs "Invalid value for <svg> attribute width=\"\"" when the
 *     attribute transitions through an empty state, even though the
 *     attribute itself ends up unset. CSS `width` overrides the
 *     attribute anyway, so we just don't touch the upstream
 *     attributes at all.
 */

const MAX_HEIGHT_PX = 560;

export function prepareSvgRoot(svg: SVGSVGElement): void {
    svg.setAttribute('overflow', 'visible');
    svg.style.width = '100%';
    svg.style.height = 'auto';
    svg.style.maxHeight = `${String(MAX_HEIGHT_PX)}px`;
    svg.style.display = 'block';
    svg.style.marginInline = 'auto';
}
