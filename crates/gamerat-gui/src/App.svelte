<script lang="ts">
    import { invoke } from '@tauri-apps/api/core';

    let name = $state('');
    let greeting = $state<string | null>(null);

    async function greet(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        greeting = await invoke<string>('greet', { name });
    }
</script>

<main>
    <h1>gamerat-gui</h1>
    <p>Tauri v2 + Svelte 5 scaffold — IPC smoke-test only.</p>

    <form onsubmit={greet}>
        <input bind:value={name} placeholder="your name" />
        <button type="submit">greet</button>
    </form>

    {#if greeting}
        <p>{greeting}</p>
    {/if}
</main>
