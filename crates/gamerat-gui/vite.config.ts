// `vitest/config`'s defineConfig extends Vite's UserConfig with the
// `test` field. Importing from `vite` directly works at runtime but
// strict TS rejects the `test` key without the proper type.
import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { paraglideVitePlugin } from '@inlang/paraglide-js';

// Tauri exposes its dev-server host via this env var when running on a
// non-localhost target (e.g. `tauri android dev`). Falls back to
// localhost-only for desktop dev.
const host = process.env['TAURI_DEV_HOST'];

// public/mice is a symlink → ../../data/mice. Vite serves publicDir
// contents at the URL root in dev, and copies them into build/ at
// build, so the frontend can fetch /mice/<filename> without
// duplicating the upstream SVG set on disk. The symlink target is
// resolved through Vite's fs.allow rules — `..` paths under publicDir
// are followed transparently.
// https://vitejs.dev/config/
export default defineConfig({
    plugins: [
        // Compiles `messages/*.json` → src/lib/paraglide/ (type-safe `m.*()`
        // functions). Runs ahead of Svelte so the generated module exists
        // before components import it; keep its options in sync with the
        // `messages` npm script (used by the non-Vite check/lint/test gates).
        paraglideVitePlugin({
            project: './project.inlang',
            outdir: './src/lib/paraglide',
            strategy: ['localStorage', 'preferredLanguage', 'baseLocale'],
            emitTsDeclarations: true,
        }),
        svelte(),
        tailwindcss(),
    ],

    // Tauri pipes its own dev banner to stdout — don't clobber it.
    clearScreen: false,

    server: {
        port: 1420,
        strictPort: true,
        host: host ?? false,
        // exactOptionalPropertyTypes: only attach hmr when we actually
        // have a host to bind it to.
        ...(host !== undefined && { hmr: { protocol: 'ws' as const, host, port: 1421 } }),
        watch: {
            // src-tauri/ has its own reload loop via cargo-watch.
            ignored: ['**/src-tauri/**'],
        },
    },

    build: {
        // Tauri's tauri.conf.json points frontendDist at ../build (this
        // dir, relative to src-tauri/). Keep them aligned.
        outDir: 'build',
        emptyOutDir: true,
        // Tauri ships a recent Chromium/WebKit; no need to transpile down.
        target: ['es2022', 'chrome120', 'safari17'],
        sourcemap: true,
    },

    test: {
        // jsdom gives us a DOM for components / dataset / fetch
        // shims that touch `document` or `localStorage`. The pure-TS
        // modules (keycode-map, button-labels) don't need it but
        // having it default-on means component tests "just work".
        environment: 'jsdom',
        include: ['src/**/*.test.ts'],
        // Tauri's webview always has globals like `window` /
        // `document`; jsdom provides them too. `globals: true`
        // exposes describe/it/expect without imports so test files
        // stay compact.
        globals: true,
        // Polyfills jsdom's broken Storage globals — see comment in
        // the setup file for details.
        setupFiles: ['./src/lib/test-setup.ts'],
        coverage: {
            provider: 'v8',
            reporter: ['text', 'html'],
            include: ['src/lib/**/*.ts'],
            exclude: ['src/lib/**/*.test.ts', 'src/lib/test-setup.ts'],
        },
    },
});
