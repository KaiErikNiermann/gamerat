<script lang="ts">
    import Icon from './Icon.svelte';
    import { doSimulateFocus } from './ipc.js';
    import { m } from './paraglide/messages.js';

    let appId = $state('');
    let title = $state('');
    let pending = $state(false);
    let result = $state<string | null>(null);
    let isError = $state(false);

    async function handleSubmit(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (appId.trim().length === 0) return;

        pending = true;
        result = null;
        isError = false;

        try {
            await doSimulateFocus(appId.trim(), title.trim());
            result = m.focus_sim_success();
        } catch (error) {
            result = String(error);
            isError = true;
        } finally {
            pending = false;
        }
    }
</script>

<section class="panel">
    <h2 class="panel-title"><Icon name="target" /> {m.focus_sim_title()}</h2>
    <p class="muted text-sm mb-2">{m.focus_sim_intro()}</p>

    <form class="add-form" onsubmit={handleSubmit}>
        <input
            class="input-field flex-1"
            bind:value={appId}
            placeholder={m.focus_sim_appid_placeholder()}
            aria-label={m.focus_sim_appid_aria()}
            required
        />
        <input
            class="input-field flex-1"
            bind:value={title}
            placeholder={m.focus_sim_title_placeholder()}
            aria-label={m.focus_sim_title_aria()}
        />
        <button class="btn-primary" type="submit" disabled={pending}>
            {pending ? '…' : m.focus_sim_inject()}
        </button>
    </form>

    {#if result}
        <p class={isError ? 'error-text' : 'success-text'}>{result}</p>
    {/if}
</section>
