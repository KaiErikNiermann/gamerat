<script lang="ts">
    import Icon from './Icon.svelte';
    import { applyProfile, removeProfile, upsertProfile } from './ipc.js';
    import type { GameratProfile } from './types.js';

    interface Props {
        profiles: GameratProfile[];
        /** Currently selected profile (highlighted row, drives
         *  MouseView's edit target). Null when nothing is selected. */
        selectedProfileId: string | null;
        /** When false (manual mode), per-row Apply buttons are
         *  enabled. When true (auto mode), apply is decided by rules,
         *  so the buttons are disabled with an explanatory title. */
        autoswitchEnabled: boolean | null;
        onprofileschange: () => void;
        onselect: (id: string | null) => void;
    }

    const {
        profiles,
        selectedProfileId,
        autoswitchEnabled,
        onprofileschange,
        onselect,
    }: Props = $props();

    // ───────────────────────────────────────────────────────────────
    // Create-profile modal state. The form only carries metadata —
    // DPI stages + button bindings are edited in MouseView once the
    // profile is selected, not here.
    // ───────────────────────────────────────────────────────────────

    let modalOpen = $state(false);
    let formId = $state('');
    let formName = $state('');
    let formDescription = $state('');
    let formCategory = $state<'agnostic' | 'specific'>('agnostic');
    let formInheritsFrom = $state('');
    let submitting = $state(false);
    let formError = $state<string | null>(null);

    const agnosticProfiles = $derived(profiles.filter((p) => p.category === 'agnostic'));

    function openCreate(): void {
        modalOpen = true;
        formId = '';
        formName = '';
        formDescription = '';
        formCategory = 'agnostic';
        formInheritsFrom = '';
        formError = null;
    }

    function closeCreate(): void {
        modalOpen = false;
    }

    async function handleSubmit(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (formId.trim().length === 0 || formName.trim().length === 0) {
            formError = 'id and name are required';
            return;
        }
        submitting = true;
        formError = null;
        const id = formId.trim();
        try {
            await upsertProfile({
                id,
                name: formName.trim(),
                description: formDescription,
                category: formCategory,
                inherits_from: formInheritsFrom,
                // Sensible defaults — the user fleshes these out in
                // MouseView's profile-mode editor after creating.
                dpi: [800],
                active_dpi_stage: 0,
                created_unix: 0,
                buttons: [],
            });
            onprofileschange();
            // Auto-select the new profile so the user lands directly
            // in the editor. Matches the user's request:
            // "create -> select -> surface bindings/DPI for editing".
            onselect(id);
            closeCreate();
        } catch (error) {
            formError = String(error);
        } finally {
            submitting = false;
        }
    }

    async function handleDelete(id: string): Promise<void> {
        try {
            await removeProfile(id);
            if (selectedProfileId === id) onselect(null);
            onprofileschange();
        } catch (error) {
            formError = `delete ${id}: ${String(error)}`;
        }
    }

    async function handleApply(id: string): Promise<void> {
        try {
            await applyProfile(id);
            onprofileschange();
        } catch (error) {
            formError = `apply ${id}: ${String(error)}`;
        }
    }

    function applyTitle(): string {
        if (autoswitchEnabled === null) return 'Daemon offline';
        return autoswitchEnabled
            ? 'Autoswitch is on — profile selection is decided by rules. Turn off autoswitch in the header to apply manually.'
            : 'Write this profile to the device now.';
    }
</script>

<section class="panel">
    <header class="profiles-header">
        <h2 class="panel-title"><Icon name="gear" /> Profiles</h2>
        <button class="btn-primary btn-sm" type="button" onclick={openCreate}>+ New profile</button>
    </header>

    {#if profiles.length === 0}
        <p class="muted">
            No profiles yet. Create one with the button above — DPI stages and
            button bindings are edited in the Mouse view once you select the
            profile here.
        </p>
    {:else}
        <ul class="profile-list">
            {#each profiles as profile (profile.id)}
                <li
                    class="profile-row"
                    class:profile-row-selected={selectedProfileId === profile.id}
                >
                    <button
                        class="profile-row-main"
                        type="button"
                        onclick={() => { onselect(profile.id); }}
                        title="Select for editing — surfaces bindings + DPI in the Mouse view."
                    >
                        <span class="profile-row-id font-mono">{profile.id}</span>
                        <span class="profile-row-name">{profile.name}</span>
                        <span class="profile-row-category" data-category={profile.category}>
                            {profile.category}
                        </span>
                        <span class="profile-row-dpi font-mono">
                            {profile.dpi
                                .map((d, i) =>
                                    i === profile.active_dpi_stage ? `*${String(d)}` : String(d),
                                )
                                .join(',')}
                        </span>
                    </button>
                    <button
                        class="btn-ghost-sm profile-row-apply"
                        type="button"
                        onclick={() => { void handleApply(profile.id); }}
                        disabled={autoswitchEnabled !== false}
                        title={applyTitle()}
                    >
                        Apply
                    </button>
                    <button
                        class="btn-danger-sm"
                        type="button"
                        onclick={() => { void handleDelete(profile.id); }}
                        aria-label="Delete profile {profile.id}"
                    >
                        ✕
                    </button>
                </li>
            {/each}
        </ul>
    {/if}

    {#if formError !== null}
        <p class="error-text">{formError}</p>
    {/if}
</section>

{#if modalOpen}
    <div
        class="binding-editor-backdrop"
        role="dialog"
        aria-modal="true"
        aria-label="Create a new profile"
        onclick={(e) => {
            if (e.target === e.currentTarget) closeCreate();
        }}
        onkeydown={(e) => {
            if (e.key === 'Escape') closeCreate();
        }}
        tabindex="-1"
    >
        <form class="binding-editor-card" onsubmit={handleSubmit}>
            <header class="binding-editor-head">
                <h3 class="binding-editor-title">New profile</h3>
                <button
                    type="button"
                    class="btn-ghost-sm"
                    onclick={closeCreate}
                    aria-label="Close"
                >
                    close
                </button>
            </header>

            <label class="binding-editor-row">
                <span class="binding-editor-label">id</span>
                <input
                    class="input-field font-mono"
                    bind:value={formId}
                    placeholder="fps-low-dpi"
                    pattern="[a-z0-9_-]+"
                    title="lowercase letters, digits, hyphens, underscores"
                    required
                />
            </label>

            <label class="binding-editor-row">
                <span class="binding-editor-label">name</span>
                <input
                    class="input-field"
                    bind:value={formName}
                    placeholder="FPS — low DPI"
                    required
                />
            </label>

            <label class="binding-editor-row">
                <span class="binding-editor-label">category</span>
                <select class="input-field" bind:value={formCategory}>
                    <option value="agnostic">agnostic</option>
                    <option value="specific">specific</option>
                </select>
            </label>

            {#if formCategory === 'specific'}
                <label class="binding-editor-row">
                    <span class="binding-editor-label">inherits from (agnostic)</span>
                    <select class="input-field" bind:value={formInheritsFrom}>
                        <option value="">— none —</option>
                        {#each agnosticProfiles as p (p.id)}
                            <option value={p.id}>{p.id}</option>
                        {/each}
                    </select>
                </label>
            {/if}

            <label class="binding-editor-row">
                <span class="binding-editor-label">description (optional)</span>
                <input
                    class="input-field"
                    bind:value={formDescription}
                    placeholder="shooter sensitivity baseline"
                />
            </label>

            {#if formError !== null}
                <p class="error-text">{formError}</p>
            {/if}

            <footer class="binding-editor-actions">
                <button class="btn-ghost" type="button" onclick={closeCreate}>Cancel</button>
                <button class="btn-primary" type="submit" disabled={submitting}>
                    {submitting ? 'Creating…' : 'Create + edit'}
                </button>
            </footer>
        </form>
    </div>
{/if}
