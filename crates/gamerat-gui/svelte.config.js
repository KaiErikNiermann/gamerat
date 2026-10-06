import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/vite-plugin-svelte').SvelteConfig} */
// eslint-disable-next-line unicorn/no-top-level-side-effects -- config module: the default export is the config
export default {
    preprocess: vitePreprocess(),
};
