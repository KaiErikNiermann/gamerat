<script lang="ts">
    import Icon from './Icon.svelte';
    import { addRule, removeRule } from './ipc.js';
    import { currentLocale } from './locale.js';
    import { m } from './paraglide/messages.js';
    import Select from './Select.svelte';
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

    /** Resolve a rule's referenced profile id to its display name.
     *  Returns the raw id as a fallback when the profile is missing —
     *  that's the only handle left for the user to identify which
     *  orphan record is dangling, since the name no longer exists. */
    function profileLabel(id: string): string {
        return profiles.find((p) => p.id === id)?.name ?? id;
    }

    /** Localized creation date for a rule (epoch seconds → locale date). */
    function formatCreated(createdUnix: number): string {
        const date = new Date(createdUnix * 1000);
        return date.toLocaleDateString(currentLocale());
    }
</script>

<section class="panel">
    <h2 class="panel-title"><Icon name="clipboard" /> {m.rules_title()}</h2>

    <form class="add-form" onsubmit={handleSubmit}>
        <input
            class="input-field flex-1"
            bind:value={glob}
            placeholder={m.rules_glob_placeholder()}
            aria-label={m.rules_glob_aria()}
            required
        />
        <Select
            bind:value={profileId}
            options={[
                { value: '', label: m.rules_profile_placeholder(), disabled: true },
                ...profiles.map((p) => ({
                    value: p.id,
                    label: p.name,
                })),
            ]}
            placeholder={m.rules_profile_placeholder()}
            ariaLabel={m.rules_profile_aria()}
            required
        />
        <button class="btn-primary" type="submit" disabled={submitting || profiles.length === 0}>
            {submitting ? '…' : m.common_add()}
        </button>
    </form>

    {#if profiles.length === 0}
        <p class="muted">{m.rules_create_profile_first()}</p>
    {/if}

    {#if formError}
        <p class="error-text">{formError}</p>
    {/if}

    {#if rules.length === 0}
        <p class="muted">{m.rules_none()}</p>
    {:else}
        <div class="table-wrap">
            <table class="data-table">
                <thead>
                    <tr>
                        <th>{m.rules_th_glob()}</th>
                        <th>{m.rules_th_profile()}</th>
                        <th>{m.rules_th_created()}</th>
                        <th></th>
                    </tr>
                </thead>
                <tbody>
                    {#each rules as rule (rule.app_id_glob)}
                        <tr>
                            <td class="font-mono">{rule.app_id_glob}</td>
                            <td>
                                <span class:error-text={!profileExists(rule.profile_id)}>
                                    {profileLabel(rule.profile_id)}
                                </span>
                                {#if !profileExists(rule.profile_id)}
                                    <span class="muted" title={m.rules_missing_title()}>
                                        {m.rules_missing()}
                                    </span>
                                {/if}
                            </td>
                            <td class="muted">
                                {formatCreated(rule.created_unix)}
                            </td>
                            <td>
                                <button
                                    class="btn-danger-sm"
                                    onclick={() => { void handleDelete(rule.app_id_glob); }}
                                    aria-label={m.rules_delete_aria({ glob: rule.app_id_glob })}
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
