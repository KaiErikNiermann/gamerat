// Shared classifier: turn a raw axe result into the list of findings we
// treat as hard failures. Used by both the contrast suite and the canary
// so "what counts as a failure" is defined in exactly one place.
//
// The subtlety that makes this non-trivial: axe does NOT put pure
// white-on-white / black-on-black text in `violations`. A perfect 1:1
// ratio lands in `incomplete` ("needs review"), because same-colour text is
// sometimes intentional (visually-hidden labels). But 1:1 is precisely the
// case the user cares most about, so we promote those incomplete nodes to
// failures — while deliberately ignoring the *other* incomplete reasons
// (background-image / overlap / "couldn't determine"), which are genuine
// manual-review noise rather than definite bugs.

import type AxeBuilder from '@axe-core/playwright';

/** axe's result shape, sourced transitively so we don't need a direct
 *  axe-core dependency (pnpm strict layout wouldn't resolve it). */
type AxeResults = Awaited<ReturnType<InstanceType<typeof AxeBuilder>['analyze']>>;

/** Impacts we fail on for non-contrast rules. */
const BLOCKING_IMPACTS = new Set(['serious', 'critical']);

/** Matches axe's message for definitively same-colour text — the
 *  white-on-white / black-on-black case that lands in `incomplete`. The
 *  ambiguous reasons ("Unable to determine", "background image",
 *  "overlapped") deliberately don't match. */
const SAME_COLOUR = /1:1 contrast ratio/i;

export interface Finding {
    readonly id: string;
    readonly impact: string;
    readonly target: string;
    readonly summary: string;
}

/** Same-colour (1:1 contrast) findings promoted out of a single
 *  `incomplete` color-contrast result. Extracted so the per-node loop
 *  can `continue` without nesting inside the outer `incomplete` loop. */
function sameColourFindings(inc: AxeResults['incomplete'][number]): Finding[] {
    const findings: Finding[] = [];
    for (const node of inc.nodes) {
        const message = [...node.any, ...node.none].map((c) => c.message).join(' ');
        if (!SAME_COLOUR.test(message)) continue;
        findings.push({ id: 'color-contrast', impact: 'serious', target: node.target.join(' '), summary: message });
    }
    return findings;
}

/** Every blocking finding in a scan: all color-contrast and
 *  serious/critical `violations`, plus same-colour `incomplete` nodes. */
export function blockingFindings(results: AxeResults): Finding[] {
    const out: Finding[] = [];

    for (const v of results.violations) {
        if (v.id !== 'color-contrast' && !BLOCKING_IMPACTS.has(v.impact ?? '')) continue;
        for (const node of v.nodes) {
            out.push({
                id: v.id,
                impact: v.impact ?? 'n/a',
                target: node.target.join(' '),
                summary: node.failureSummary ?? '',
            });
        }
    }

    for (const inc of results.incomplete) {
        if (inc.id !== 'color-contrast') continue;
        out.push(...sameColourFindings(inc));
    }

    return out;
}

/** Human-readable dump for a failing assertion message. */
export function formatFindings(findings: readonly Finding[]): string {
    return findings
        .map((f) => `${f.id} [${f.impact}] @ ${f.target}\n      ${f.summary.replaceAll('\n', '\n      ')}`)
        .join('\n\n');
}
