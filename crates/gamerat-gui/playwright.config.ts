// Playwright config for the runtime accessibility / contrast suite.
//
// This is deliberately separate from the vitest unit suite: jsdom (the
// vitest env) has no layout engine and returns no computed colours, so
// axe-core's `color-contrast` rule is inert there. Real contrast
// detection needs a real browser → Playwright + headless Chromium.
//
// We run against `vite preview` (a production build) rather than the dev
// server so the scanned surface exactly matches what ships: in dev,
// `import.meta.env.DEV` force-renders the dev-only DevPanel / FocusSimulate
// panels, which we don't want to gate on. Token CSS is identical between
// dev and prod, so contrast fidelity is unaffected.
//
// The app is hard-gated behind Tauri (`invoke('daemon_alive')`); the specs
// inject a `window.__TAURI_INTERNALS__` mock (see tests/a11y/tauri-mock.ts)
// before page load so the full UI renders without a running daemon.

import { defineConfig, devices } from '@playwright/test';

const PORT = 4173;

export default defineConfig({
    testDir: './tests/a11y',
    // a11y assertions are deterministic; no need to retry. Fail fast.
    retries: 0,
    fullyParallel: true,
    // Surface the first real failure clearly rather than burying it.
    reporter: process.env['CI'] ? [['github'], ['list']] : [['list']],
    use: {
        baseURL: `http://localhost:${PORT}`,
    },
    projects: [
        {
            name: 'chromium',
            use: { ...devices['Desktop Chrome'] },
        },
    ],
    // Build once, then serve the production bundle. `reuseExistingServer`
    // lets a locally-running `pnpm preview` be reused during iteration.
    webServer: {
        command: `pnpm build && pnpm preview --port ${PORT} --strictPort`,
        url: `http://localhost:${PORT}`,
        reuseExistingServer: !process.env['CI'],
        timeout: 120_000,
    },
});
