/**
 * Small duration utilities shared by the action-feedback UI and the
 * dev log. The GUI surfaces how long user actions take (save, apply,
 * …) right next to their status text so timing anomalies are visible
 * without anyone having to reach for a profiler — if the mouse feels
 * sluggish, the number next to "saved" tells the story.
 */

/** Result of a `timed()` call: the wrapped value + its wall-clock cost. */
export interface TimedResult<T> {
    readonly result: T;
    readonly ms: number;
}

/**
 * Run an async unit of work and report how long it took in
 * milliseconds (sub-ms precision via `performance.now`). The duration
 * covers exactly the awaited body — callers decide what to include.
 */
export async function timed<T>(work: () => Promise<T>): Promise<TimedResult<T>> {
    const start = performance.now();
    const result = await work();
    return { result, ms: performance.now() - start };
}

/**
 * Format a millisecond duration into a compact, human-scaled string:
 *
 *   0.82  → "820µs"
 *   4.3   → "4ms"
 *   847   → "847ms"
 *   1840  → "1.84s"
 *
 * Picks the unit so the number stays short and the magnitude is
 * obvious at a glance. Negative / NaN inputs clamp to "0µs" rather
 * than rendering nonsense in the UI.
 */
export function formatDuration(ms: number): string {
    if (!Number.isFinite(ms) || ms <= 0) return '0µs';
    if (ms < 1) return `${String(Math.round(ms * 1000))}µs`;
    if (ms < 1000) return `${String(Math.round(ms))}ms`;
    return `${(ms / 1000).toFixed(2)}s`;
}
