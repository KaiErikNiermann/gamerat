<script lang="ts">
    import Icon from './Icon.svelte';
    import { addRule, removeRule } from './ipc.js';
    import type { GameratProfile, Rule } from './types.js';

    interface Props {
        rules: Rule[];
        profiles: GameratProfile[];
        onruleschange: () => void;
    }

    const { rules, profiles, onruleschange }: Props = $props();

    let glob = $state('');
    let profileId = $state('');
    let submitting = $state(false);
    let formError = $state<string | null>(null);
    let deleteErrors = $state<Record<string, string>>({});

    async function handleSubmit(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (glob.trim().length === 0 || profileId.length === 0) return;

        submitting = true;
        formError = null;
        try {
            await addRule(glob.trim(), profileId);
            glob = '';
            profileId = '';
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

    // Display helper: rules can reference profile ids that no longer
    // exist (e.g. user deleted the profile but kept the rule). Tag
    // those visually rather than letting them silently look fine.
    function profileExists(id: string): boolean {
        return profiles.some((p) => p.id === id);
    }
</script>

<section class="panel">
    <h2 class="panel-title"><Icon name="clipboard" /> Rules</h2>

    <form class="add-form" onsubmit={handleSubmit}>
        <input
            class="input-field flex-1"
            bind:value={glob}
            placeholder="app_id glob (e.g. steam_app_*)"
            aria-label="App ID glob"
            required
        />
        <select
            class="input-field"
            bind:value={profileId}
            aria-label="Profile"
            required
        >
            <option value="" disabled selected>profile</option>
            {#each profiles as profile (profile.id)}
                <option value={profile.id}>
                    {profile.name} ({profile.id})
                </option>
            {/each}
        </select>
        <button class="btn-primary" type="submit" disabled={submitting || profiles.length === 0}>
            {submitting ? '…' : 'Add'}
        </button>
    </form>

    {#if profiles.length === 0}
        <p class="muted">Create a profile first — rules need something to reference.</p>
    {/if}

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
                            <td>
                                <span class:error-text={!profileExists(rule.profile_id)}>
                                    {rule.profile_id}
                                </span>
                                {#if !profileExists(rule.profile_id)}
                                    <span class="muted" title="No profile with this id exists">
                                        (missing)
                                    </span>
                                {/if}
                            </td>
                            <td class="muted">
                                {new Date(rule.created_unix * 1000).toLocaleDateString()}
                            </td>
                            <td>
                                <button
                                    class="btn-danger-sm"
                                    onclick={() => { void handleDelete(rule.app_id_glob); }}
                                    aria-label="Delete rule {rule.app_id_glob}"
                                >
                                    ✕
                                </button>
                                {#if getDeleteError(rule.app_id_glob)}
                                    <span class="error-text text-xs">
                                        {getDeleteError(rule.app_id_glob)}
                                    </span>
                                {/if}
                            </td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</section>
