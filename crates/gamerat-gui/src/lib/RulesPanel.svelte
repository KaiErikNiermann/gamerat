<script lang="ts">
    import { addRule, removeRule } from './ipc.js';
    import type { Rule } from './types.js';

    interface Props {
        rules: Rule[];
        onruleschange: () => void;
    }

    const { rules, onruleschange }: Props = $props();

    let glob = $state('');
    let profileIndex = $state(0);
    let submitting = $state(false);
    let formError = $state<string | null>(null);
    let deleteErrors = $state<Record<string, string>>({});

    async function handleSubmit(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (glob.trim().length === 0) return;

        submitting = true;
        formError = null;
        try {
            await addRule(glob.trim(), profileIndex);
            glob = '';
            profileIndex = 0;
            onruleschange();
        } catch (error) {
            formError = String(error);
        } finally {
            submitting = false;
        }
    }

    function getDeleteError(glob_: string): string | undefined {
        return Object.entries(deleteErrors).find(([k]) => k === glob_)?.[1];
    }

    async function handleDelete(appIdGlob: string): Promise<void> {
        // Clear any previous error for this glob before retrying.
        deleteErrors = Object.fromEntries(
            Object.entries(deleteErrors).filter(([k]) => k !== appIdGlob),
        );
        try {
            await removeRule(appIdGlob);
            onruleschange();
        } catch (error) {
            deleteErrors = { ...deleteErrors, [appIdGlob]: String(error) };
        }
    }
</script>

<section class="panel">
    <h2 class="panel-title">📋 Rules</h2>

    <form class="add-form" onsubmit={handleSubmit}>
        <input
            class="input-field flex-1"
            bind:value={glob}
            placeholder="app_id glob (e.g. steam_app_*)"
            aria-label="App ID glob"
            required
        />
        <input
            class="input-field w-24"
            type="number"
            bind:value={profileIndex}
            min="0"
            max="255"
            aria-label="Profile index"
        />
        <button class="btn-primary" type="submit" disabled={submitting}>
            {submitting ? '…' : 'Add'}
        </button>
    </form>

    {#if formError}
        <p class="error-text">{formError}</p>
    {/if}

    {#if rules.length === 0}
        <p class="muted">No rules yet.</p>
    {:else}
        <div class="table-wrap">
            <table class="data-table">
                <thead>
                    <tr>
                        <th>Glob</th>
                        <th>Profile</th>
                        <th>Created</th>
                        <th></th>
                    </tr>
                </thead>
                <tbody>
                    {#each rules as rule (rule.app_id_glob)}
                        <tr>
                            <td class="font-mono">{rule.app_id_glob}</td>
                            <td class="text-center">{rule.profile_index}</td>
                            <td class="muted">{new Date(rule.created_unix * 1000).toLocaleDateString()}</td>
                            <td>
                                <button
                                    class="btn-danger-sm"
                                    onclick={() => handleDelete(rule.app_id_glob)}
                                    aria-label="Delete rule {rule.app_id_glob}"
                                >
                                    ✕
                                </button>
                                {#if getDeleteError(rule.app_id_glob)}
                                    <span class="error-text text-xs">{getDeleteError(rule.app_id_glob)}</span>
                                {/if}
                            </td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</section>
