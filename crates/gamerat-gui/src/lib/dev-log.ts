/**
 * In-memory ring buffer of every IPC call and Tauri event the GUI
 * makes / receives. Lives in module scope so any code path can append
 * — `ipc.ts` wraps `invoke()`, `App.svelte` logs incoming events.
 *
 * The store is a Svelte reactive array (`$state` via `SvelteMap` is
 * overkill here — we only need append + slice). Capped at MAX_ENTRIES
 * so the buffer can't grow without bound across long sessions.
 */

import { SvelteSet } from 'svelte/reactivity';

const MAX_ENTRIES = 250;

export type DevLogKind = 'invoke' | 'invoke-result' | 'invoke-error' | 'event';

export interface DevLogEntry {
    readonly id: number;
    readonly ts: number;
    readonly kind: DevLogKind;
    readonly label: string;
    /** Truncated JSON-ish preview of the payload. */
    readonly preview: string;
    /** Wall-clock duration in ms — only meaningful on invoke-result / -error. */
    readonly elapsedMs?: number;
}

// Module-scope reactive state. Svelte 5 lets us export $state directly,
// but Svelte's compiler complains if it's a top-level const declaration
// outside a component, so wrap in a getter pattern via a class-style
// object. SvelteSet is reactive without any rune dance.
const entries = new SvelteSet<DevLogEntry>();
let nextId = 1;

/** Append a new entry, evicting oldest once we exceed the cap. */
function append(entry: Omit<DevLogEntry, 'id' | 'ts'>): DevLogEntry {
    const full: DevLogEntry = { ...entry, id: nextId++, ts: Date.now() };
    entries.add(full);
    while (entries.size > MAX_ENTRIES) {
        // SvelteSet preserves insertion order; pop the oldest.
        const oldest = entries.values().next().value;
        if (oldest !== undefined) entries.delete(oldest);
    }
    return full;
}

export function logInvokeStart(command: string, args: unknown): void {
    append({ kind: 'invoke', label: command, preview: preview(args) });
}

export function logInvokeResult(command: string, result: unknown, elapsedMs: number): void {
    append({
        kind: 'invoke-result',
        label: command,
        preview: preview(result),
        elapsedMs,
    });
}

export function logInvokeError(command: string, error: unknown, elapsedMs: number): void {
    append({
        kind: 'invoke-error',
        label: command,
        preview: preview(error),
        elapsedMs,
    });
}

export function logEvent(name: string, payload: unknown): void {
    append({ kind: 'event', label: name, preview: preview(payload) });
}

export function clearDevLog(): void {
    entries.clear();
}

/**
 * Returns the live entry list. Callers should iterate as needed —
 * the SvelteSet reacts to `$derived` consumption.
 */
export function devLogEntries(): SvelteSet<DevLogEntry> {
    return entries;
}

/**
 * Compact a value to a short JSON-ish preview. Strips deeply-nested
 * structures and caps total length so a runaway payload doesn't
 * blow up the panel.
 */
function preview(value: unknown): string {
    if (value === undefined) return 'undefined';
    try {
        const json = JSON.stringify(value, replacer, 0);
        // JSON.stringify can still return undefined for symbols /
        // functions inside otherwise-serialisable shapes.
        if (typeof json !== 'string') return '[unserialisable]';
        return json.length > 240 ? `${json.slice(0, 237)}…` : json;
    } catch {
        return '[unserialisable]';
    }
}

function replacer(_key: string, value: unknown): unknown {
    if (typeof value === 'string' && value.length > 80) {
        return `${value.slice(0, 77)}…`;
    }
    return value;
}
