// Runtime contrast + critical-a11y scan of the real GUI.
//
// Renders the production bundle in headless Chromium with a mocked Tauri
// runtime (see tauri-mock.ts) so every panel populates, then runs axe-core
// across all four CSS theme-resolution paths and the two representative
// modals. The gate is: zero `color-contrast` violations and zero
// serious/critical a11y violations.
//
// Why four "themes": the app resolves colours through three layers —
// `:root` (dark default), `@media (prefers-color-scheme)`, and a
// `[data-theme]` override. The original bug we're guarding against lived in
// the `:root` dark default, which is what *system mode under a dark OS
// preference* actually uses. So we test explicit dark/light AND system mode
// under both emulated OS preferences to cover every path a token travels.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';
import { blockingFindings, formatFindings } from './axe-gate.js';
import { installTauriMock } from './tauri-mock.js';

type Theme = 'dark' | 'light' | 'system';
type ColorScheme = 'dark' | 'light';

interface ThemeCase {
    readonly label: string;
    readonly theme: Theme;
    /** Emulated OS `prefers-color-scheme`. Only observable when `theme`
     *  is `system` (an explicit `data-theme` overrides the media query),
     *  but set on every case so the page state is unambiguous. */
    readonly media: ColorScheme;
}

const THEME_CASES: readonly ThemeCase[] = [
    { label: 'theme=dark', theme: 'dark', media: 'dark' },
    { label: 'theme=light', theme: 'light', media: 'light' },
    // The two that exercise the media-query + :root-default paths:
    { label: 'system + OS dark (:root default)', theme: 'system', media: 'dark' },
    { label: 'system + OS light (@media light)', theme: 'system', media: 'light' },
];

/** Boot the app in a given theme and wait until it's fully rendered: the
 *  daemon gate is cleared (`aria-hidden=false`) AND fixture-backed content
 *  has landed (a known game row is visible — proves the panel fetches
 *  resolved and painted, not just that the shell mounted).
 *
 *  Theme is driven through the app's OWN mechanism — seed
 *  `localStorage['gamerat:theme']` before load so theme.ts's `loadTheme()`
 *  applies it on mount — rather than poking `data-theme` after the fact.
 *  Post-mount attribute pokes race the ThemeToggle `$effect` (which holds
 *  the stored theme) and leave the page half-themed, producing false
 *  contrast hits. `colorScheme` emulation is set to match so the `system`
 *  cases resolve through the right `@media (prefers-color-scheme)` branch. */
async function gotoApp(page: Page, theme: Theme, media: ColorScheme): Promise<void> {
    await installTauriMock(page);
    await page.emulateMedia({ colorScheme: media });
    await page.addInitScript((t) => {
        localStorage.setItem('gamerat:theme', t);
    }, theme);
    await page.goto('/');
    await expect(page.locator('main.app-layout')).toHaveAttribute('aria-hidden', 'false');
    await expect(page.getByText('Counter-Strike 2')).toBeVisible();
}

/** Run axe and fail with a readable dump if any blocking finding
 *  (contrast — including same-colour text axe parks in `incomplete` — or a
 *  serious/critical a11y issue) is present. The dump names the rule, the
 *  offending selector, and axe's per-node failure summary so a red run
 *  points straight at the token/element to fix. See axe-gate.ts. */
async function expectNoViolations(page: Page, label: string): Promise<void> {
    const results = await new AxeBuilder({ page }).withTags(['wcag2a', 'wcag2aa']).analyze();
    const findings = blockingFindings(results);
    expect(findings, `axe blocking findings in ${label}:\n${formatFindings(findings)}`).toEqual([]);
}

for (const { label, theme, media } of THEME_CASES) {
    test(`main view — ${label}`, async ({ page }) => {
        await gotoApp(page, theme, media);
        await expectNoViolations(page, `main view (${label})`);
    });
}

// Modals reuse the shared Modal.svelte shell + form-field tokens; scanning
// one in each explicit theme covers those surfaces. (Explicit data-theme
// overrides the media query, so emulation is matched only for tidiness.)
for (const theme of ['dark', 'light'] as const) {
    test(`settings modal — theme=${theme}`, async ({ page }) => {
        await gotoApp(page, theme, theme);
        await page.getByRole('button', { name: 'Open settings' }).click();
        await expect(page.getByRole('heading', { name: 'Settings' })).toBeVisible();
        await expectNoViolations(page, `settings modal (theme=${theme})`);
    });

    test(`add-manual modal — theme=${theme}`, async ({ page }) => {
        await gotoApp(page, theme, theme);
        await page.getByRole('button', { name: '+ Manual' }).click();
        await expect(page.getByRole('heading', { name: 'Add manual game' })).toBeVisible();
        await expectNoViolations(page, `add-manual modal (theme=${theme})`);
    });
}
