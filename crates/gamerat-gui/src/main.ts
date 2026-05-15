import { mount } from 'svelte';
import App from './App.svelte';
import './app.css';

const target = document.querySelector('#app');
if (!(target instanceof HTMLElement)) {
    throw new Error('#app root element missing from index.html');
}

export default mount(App, { target });
