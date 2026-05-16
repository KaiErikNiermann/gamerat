<script lang="ts">
    import { doSimulateFocus } from './ipc.js';

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
            result = 'Injected successfully.';
        } catch (error) {
            result = String(error);
            isError = true;
        } finally {
            pending = false;
        }
    }
</script>

<section class="panel">
    <h2 class="panel-title">🎯 Simulate focus</h2>
    <p class="muted text-sm mb-2">
        Injects a synthetic focus event — the daemon runs it through the rule matcher
        and emits <code>FocusChanged</code> (and <code>ProfileSwitched</code> if a rule matches).
    </p>

    <form class="add-form" onsubmit={handleSubmit}>
        <input
            class="input-field flex-1"
            bind:value={appId}
            placeholder="app_id (e.g. org.mozilla.firefox)"
            aria-label="App ID"
            required
        />
        <input
            class="input-field flex-1"
            bind:value={title}
            placeholder="title (optional)"
            aria-label="Window title"
        />
        <button class="btn-primary" type="submit" disabled={pending}>
            {pending ? '…' : 'Inject'}
        </button>
    </form>

    {#if result}
        <p class={isError ? 'error-text' : 'success-text'}>{result}</p>
    {/if}
</section>
