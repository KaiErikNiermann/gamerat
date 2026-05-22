import { describe, expect, it } from 'vitest';

import { formatDuration, timed } from './timing.js';

describe('formatDuration', () => {
    it('renders sub-millisecond durations as microseconds', () => {
        expect(formatDuration(0.82)).toBe('820µs');
        expect(formatDuration(0.001)).toBe('1µs');
    });

    it('renders single- and multi-digit millisecond durations as ms', () => {
        expect(formatDuration(4.3)).toBe('4ms');
        expect(formatDuration(42)).toBe('42ms');
        expect(formatDuration(847)).toBe('847ms');
    });

    it('renders second-scale durations with two decimals', () => {
        expect(formatDuration(1000)).toBe('1.00s');
        expect(formatDuration(1840)).toBe('1.84s');
    });

    it('clamps non-positive and non-finite inputs to 0µs', () => {
        expect(formatDuration(0)).toBe('0µs');
        expect(formatDuration(-5)).toBe('0µs');
        expect(formatDuration(Number.NaN)).toBe('0µs');
        expect(formatDuration(Number.POSITIVE_INFINITY)).toBe('0µs');
    });
});

describe('timed', () => {
    it('returns the work result alongside a non-negative duration', async () => {
        const { result, ms } = await timed(async () => {
            await Promise.resolve();
            return 42;
        });
        expect(result).toBe(42);
        expect(ms).toBeGreaterThanOrEqual(0);
    });

    it('propagates rejections from the wrapped work', async () => {
        await expect(
            timed(async () => {
                await Promise.resolve();
                throw new Error('boom');
            }),
        ).rejects.toThrow('boom');
    });
});
