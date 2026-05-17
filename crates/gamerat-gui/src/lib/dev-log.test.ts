import { beforeEach, describe, expect, it } from 'vitest';

import {
    clearDevLog,
    devLogEntries,
    logEvent,
    logInvokeError,
    logInvokeResult,
    logInvokeStart,
} from './dev-log.js';

describe('dev-log', () => {
    beforeEach(() => {
        clearDevLog();
    });

    it('records invoke start / result / error with kinds and labels', () => {
        logInvokeStart('list_rules', { foo: 1 });
        logInvokeResult('list_rules', [], 12);
        logInvokeError('list_rules', 'boom', 4);
        const entries = [...devLogEntries()];
        expect(entries.map((e) => e.kind)).toEqual(['invoke', 'invoke-result', 'invoke-error']);
        expect(entries.every((e) => e.label === 'list_rules')).toBe(true);
    });

    it('attaches elapsedMs only to result/error entries', () => {
        logInvokeStart('x', {});
        logInvokeResult('x', null, 7);
        logInvokeError('x', 'oh no', 9);
        const entries = [...devLogEntries()];
        expect(entries[0]?.elapsedMs).toBeUndefined();
        expect(entries[1]?.elapsedMs).toBe(7);
        expect(entries[2]?.elapsedMs).toBe(9);
    });

    it('logs events as their own kind', () => {
        logEvent('focus-changed', { app_id: 'firefox' });
        const entries = [...devLogEntries()];
        expect(entries[0]?.kind).toBe('event');
        expect(entries[0]?.label).toBe('focus-changed');
    });

    it('caps the ring buffer at MAX_ENTRIES and evicts oldest first', () => {
        // We don't expose MAX_ENTRIES; instead push enough to over-fill
        // it and verify the buffer doesn't grow unbounded. 300 > 250
        // (current cap).
        for (let i = 0; i < 300; i++) logInvokeStart(`cmd-${String(i)}`, {});
        const entries = [...devLogEntries()];
        expect(entries.length).toBeLessThanOrEqual(250);
        // The oldest visible entry should be one of the *later* writes.
        // Specifically, cmd-0 must have been evicted.
        expect(entries.some((e) => e.label === 'cmd-0')).toBe(false);
        // And the most recent write must still be present.
        expect(entries.at(-1)?.label).toBe('cmd-299');
    });

    it('serialises large payloads to a truncated preview', () => {
        const huge = 'x'.repeat(500);
        logInvokeStart('big', { huge });
        const entries = [...devLogEntries()];
        const preview = entries[0]?.preview ?? '';
        // Truncated to ≤ 240 chars + ellipsis (per dev-log.ts preview()).
        expect(preview.length).toBeLessThanOrEqual(240 + 1);
    });

    it('tolerates unserialisable values without throwing', () => {
        // Circular references break JSON.stringify — preview() should
        // recover into a placeholder rather than propagating.
        interface CircularRef { self?: unknown }
        const circular: CircularRef = {};
        circular.self = circular;
        expect(() => {
            logInvokeStart('weird', circular);
        }).not.toThrow();
    });

    it('clearDevLog empties the buffer', () => {
        logInvokeStart('a', {});
        logInvokeStart('b', {});
        clearDevLog();
        expect([...devLogEntries()]).toEqual([]);
    });
});
