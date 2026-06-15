import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';
import { getLocale, getTextDirection } from './lib/paraglide/runtime.js';

// Reflect the Paraglide-resolved locale on `<html lang>` for the a11y tree
// and `:lang()` styling (index.html ships lang="en"), plus `dir` so RTL
// locales (ar / he) lay out right-to-left.
document.documentElement.lang = getLocale();
document.documentElement.dir = getTextDirection();

const target = document.querySelector('#app');
if (!(target instanceof HTMLElement)) {
    throw new TypeError('#app root element missing from index.html');
}

export default mount(App, { target });
