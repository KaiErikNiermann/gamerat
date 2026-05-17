import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';
import { viteStaticCopy } from 'vite-plugin-static-copy';

// Tauri exposes its dev-server host via this env var when running on a
// non-localhost target (e.g. `tauri android dev`). Falls back to
// localhost-only for desktop dev.
const host = process.env['TAURI_DEV_HOST'];

// https://vitejs.dev/config/
export default defineConfig({
    plugins: [
        svelte(),
        tailwindcss(),
        // Mirror the canonical mouse SVGs + svg-lookup.ini from
        // data/mice/ into the served static tree so the frontend can
        // fetch them as /mice/<filename>. No duplication on disk; the
        // plugin copies at dev-serve time and into build/ at build.
        viteStaticCopy({
            targets: [
                { src: '../../data/mice/*.svg', dest: 'mice' },
                { src: '../../data/mice/svg-lookup.ini', dest: 'mice' },
            ],
        }),
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
});
