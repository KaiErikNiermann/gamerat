import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';
import { getLocale } from './lib/paraglide/runtime.js';

// Reflect the Paraglide-resolved locale on <html lang> for the a11y tree
// and `:lang()` styling. index.html ships lang="en"; this corrects it to
// the actual active locale (browser preference / stored choice).
document.documentElement.lang = getLocale();

const target = document.querySelector('#app');
if (!(target instanceof HTMLElement)) {
    throw new TypeError('#app root element missing from index.html');
}

export default mount(App, { target });
