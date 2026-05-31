// Canary: proves the contrast gate actually detects.
//
// A passing contrast suite is only meaningful if the gate would *fail* on a
// real problem. This injects deliberately broken text and asserts the gate
// (axe-gate.ts) flags it, plus a well-contrasted element and asserts the
// gate stays quiet. If this ever goes green-by-doing-nothing (mock
// misconfigured, axe not running, rule disabled), the bad-contrast
// assertions fail loudly — guarding the rest of the suite against silently
// becoming a no-op.
//
// We assert two distinct bad cases because they travel different axe paths:
//   - low-but-determinable contrast → `violations`
//   - exact white-on-white (1:1)    → `incomplete` (the case axe won't
//                                      auto-fail, which the gate promotes)
// The user named white-on-white explicitly, so the 1:1 case is the most
// important thing this canary protects.

import AxeBuilder from '@axe-core/playwright';
import { expect, test } from '@playwright/test';
import { blockingFindings } from './axe-gate.js';
import { installTauriMock } from './tauri-mock.js';

test('canary: gate flags bad contrast (incl. white-on-white) and passes good', async ({
    page,
}) => {
    await installTauriMock(page);
    await page.goto('/');
    await expect(page.locator('main.app-layout')).toHaveAttribute('aria-hidden', 'false');

    await page.evaluate(() => {
        const mk = (id: string, color: string, bg: string): HTMLParagraphElement => {
            const p = document.createElement('p');
            p.id = id;
            p.textContent = `${id} sample text`;
            p.style.color = color;
            p.style.backgroundColor = bg;
            p.style.fontSize = '16px';
            p.style.padding = '8px';
            return p;
        };
        document.body.append(
            mk('canary-faint', '#c8c8c8', '#ffffff'), // ~1.6:1 → violations
            mk('canary-white', '#ffffff', '#ffffff'), // 1:1 → incomplete (promoted)
            mk('canary-good', '#000000', '#ffffff'), // 21:1 → must pass
        );
    });

    for (const id of ['canary-faint', 'canary-white']) {
        const results = await new AxeBuilder({ page })
            .include(`#${id}`)
            .withRules(['color-contrast'])
            .analyze();
        expect(
            blockingFindings(results).map((f) => f.id),
            `${id} should be flagged as a color-contrast finding`,
        ).toContain('color-contrast');
    }

    const good = await new AxeBuilder({ page })
        .include('#canary-good')
        .withRules(['color-contrast'])
        .analyze();
    expect(
        blockingFindings(good),
        'black-on-white text must not be flagged (no false positives)',
    ).toEqual([]);
});
