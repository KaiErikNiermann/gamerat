<script lang="ts">
    import Icon from './Icon.svelte';
    import { applyBase, applyProfile, removeProfile, upsertProfile } from './ipc.js';
    import Select from './Select.svelte';
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
                leds: [],
                soft_macros: [],
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

    async function handleApplyBase(): Promise<void> {
        try {
            await applyBase();
            onprofileschange();
        } catch (error) {
            formError = `apply base: ${String(error)}`;
        }
    }

    function applyTitle(): string {
        if (autoswitchEnabled === null) return 'Daemon offline';
        return autoswitchEnabled
            ? 'Autoswitch is on — profile selection is decided by rules. Turn off autoswitch in the header to apply manually.'
            : 'Write this profile to the device now.';
    }

    function applyBaseTitle(): string {
        if (autoswitchEnabled === null) return 'Daemon offline';
        return autoswitchEnabled
            ? 'Autoswitch is on — base is applied automatically when no rule matches. Turn off autoswitch to apply manually.'
            : 'Switch the device back to the reserved Desktop slot now.';
    }
</script>

<section class="panel">
    <header class="profiles-header">
        <h2 class="panel-title"><Icon name="gear" /> Profiles</h2>
        <button class="btn-primary btn-sm" type="button" onclick={openCreate}>+ New profile</button>
    </header>

    <!-- Persistent "Base" row pinned at the top of the list. Always
         present and never deletable — it represents the reserved
         Desktop slot, the canonical no-game baseline. Selecting it
         drops MouseView into Base / live-hardware mode; Apply forces
         the device back to that slot regardless of the current
         autoswitch state (the autoswitch gating mirrors the per-
         profile Apply: disabled in auto mode, since the daemon would
         immediately re-apply on the next focus event). -->
    <ul class="profile-list">
        <li
            class="profile-row profile-row-base"
            class:profile-row-selected={selectedProfileId === null}
        >
            <button
                class="profile-row-main"
                type="button"
                onclick={() => { onselect(null); }}
                title="Edit the live hardware bindings on the reserved Desktop slot."
            >
                <span class="profile-row-id font-mono">base</span>
                <span class="profile-row-name">base</span>
                <span class="profile-row-category" data-category="agnostic">desktop</span>
                <span class="profile-row-dpi font-mono">—</span>
            </button>
            <button
                class="btn-ghost-sm profile-row-apply"
                type="button"
                onclick={() => { void handleApplyBase(); }}
                disabled={autoswitchEnabled !== false}
                title={applyBaseTitle()}
            >
                Apply
            </button>
            <!-- Spacer where the delete button would sit on a normal
                 row — keeps the column grid aligned without rendering
                 a real button. -->
            <span class="profile-row-delete-placeholder" aria-hidden="true"></span>
        </li>

        {#if profiles.length === 0}
            <li class="profile-row profile-row-empty-hint">
                <p class="muted text-xs">
                    No user profiles yet. Create one with the button above —
                    DPI stages and button bindings are edited in the Mouse view
                    once you select the profile here.
                </p>
            </li>
        {:else}
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
                        <span class="profile-row-id font-mono" title={profile.id}>{profile.id}</span>
                        <span class="profile-row-name" title={profile.name}>{profile.name}</span>
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
        {/if}
    </ul>

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
                <Select
                    bind:value={formCategory}
                    options={[
                        { value: 'agnostic', label: 'agnostic' },
                        { value: 'specific', label: 'specific' },
                    ]}
                    ariaLabel="Profile category"
                />
            </label>

            {#if formCategory === 'specific'}
                <label class="binding-editor-row">
                    <span class="binding-editor-label">inherits from (agnostic)</span>
                    <Select
                        bind:value={formInheritsFrom}
                        options={[
                            { value: '', label: '— none —' },
                            ...agnosticProfiles.map((p) => ({ value: p.id, label: p.id })),
                        ]}
                        ariaLabel="Inherits from"
                    />
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
