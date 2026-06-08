import { describe, expect, it } from 'vitest';

import { prepareSvgRoot } from './svg-prep.js';

function makeSvg(): SVGSVGElement {
    // eslint-disable-next-line unicorn/prefer-https -- SVG namespace URI is a fixed XML identifier, not a network URL
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    // Mimic the upstream Piper SVGs: explicit pixel width / height +
    // a viewBox to drive the internal coordinate system.
    svg.setAttribute('width', '467.83105');
    svg.setAttribute('height', '400.87393');
    svg.setAttribute('viewBox', '0 0 467.83105 398.87393');
    return svg;
}

describe('prepareSvgRoot', () => {
    it('sets overflow="visible" so out-of-viewBox leaders render', () => {
        const svg = makeSvg();
        prepareSvgRoot(svg);
        expect(svg.getAttribute('overflow')).toBe('visible');
    });

    it('does NOT remove or blank the width / height attributes', () => {
        // Regression guard: WebKit logs "Invalid value for <svg>
        // attribute width=" whenever the attribute is set to an
        // empty string or removed mid-frame. The fix keeps the
        // upstream attributes intact and sizes via CSS instead.
        const svg = makeSvg();
        prepareSvgRoot(svg);
        expect(svg.getAttribute('width')).toBe('467.83105');
        expect(svg.getAttribute('height')).toBe('400.87393');
        expect(svg.hasAttribute('width')).toBe(true);
        expect(svg.hasAttribute('height')).toBe(true);
    });

    it('preserves the upstream viewBox', () => {
        const svg = makeSvg();
        prepareSvgRoot(svg);
        expect(svg.getAttribute('viewBox')).toBe('0 0 467.83105 398.87393');
    });

    it('applies responsive CSS sizing', () => {
        const svg = makeSvg();
        prepareSvgRoot(svg);
        expect(svg.style.width).toBe('100%');
        expect(svg.style.height).toBe('auto');
        expect(svg.style.maxHeight).toBe('560px');
        expect(svg.style.display).toBe('block');
    });

    it('is idempotent — repeated calls don\'t blow up or alter attributes', () => {
        const svg = makeSvg();
        prepareSvgRoot(svg);
        prepareSvgRoot(svg);
        prepareSvgRoot(svg);
        expect(svg.getAttribute('width')).toBe('467.83105');
        expect(svg.getAttribute('height')).toBe('400.87393');
        expect(svg.getAttribute('overflow')).toBe('visible');
    });
});
