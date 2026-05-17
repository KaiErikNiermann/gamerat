<script lang="ts">
    import Icon from './Icon.svelte';
    import { removeProfile, upsertProfile } from './ipc.js';
    import type { GameratProfile } from './types.js';

    interface Props {
        profiles: GameratProfile[];
        onprofileschange: () => void;
    }

    const { profiles, onprofileschange }: Props = $props();

    type Mode = { kind: 'create' } | { kind: 'edit'; original: string };

    let mode = $state<Mode>({ kind: 'create' });
    let formId = $state('');
    let formName = $state('');
    let formDescription = $state('');
    let formCategory = $state<'agnostic' | 'specific'>('agnostic');
    let formInheritsFrom = $state('');
    let formDpi = $state<number[]>([800]);
    let formActiveStage = $state(0);
    let submitting = $state(false);
    let formError = $state<string | null>(null);

    const agnosticProfiles = $derived(profiles.filter((p) => p.category === 'agnostic'));

    function resetForm(): void {
        mode = { kind: 'create' };
        formId = '';
        formName = '';
        formDescription = '';
        formCategory = 'agnostic';
        formInheritsFrom = '';
        formDpi = [800];
        formActiveStage = 0;
        formError = null;
    }

    function startEditing(profile: GameratProfile): void {
        mode = { kind: 'edit', original: profile.id };
        formId = profile.id;
        formName = profile.name;
        formDescription = profile.description;
        formCategory = profile.category === 'specific' ? 'specific' : 'agnostic';
        formInheritsFrom = profile.inherits_from;
        formDpi = [...profile.dpi];
        formActiveStage = profile.active_dpi_stage;
        formError = null;
    }

    function addDpiStage(): void {
        const last = formDpi.at(-1) ?? 800;
        formDpi = [...formDpi, last];
    }

    function removeDpiStage(idx: number): void {
        if (formDpi.length === 1) return; // schema requires at least one stage
        formDpi = formDpi.filter((_, i) => i !== idx);
        if (formActiveStage >= formDpi.length) {
            formActiveStage = formDpi.length - 1;
        }
    }

    function updateDpiStage(idx: number, value: number): void {
        formDpi = formDpi.map((v, i) => (i === idx ? value : v));
    }

    async function handleSubmit(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (formId.trim().length === 0 || formName.trim().length === 0 || formDpi.length === 0) {
            formError = 'id, name, and at least one DPI stage are required';
            return;
        }
        submitting = true;
        formError = null;
        try {
            const original = profiles.find((p) => p.id === formId);
            await upsertProfile({
                id: formId.trim(),
                name: formName.trim(),
                description: formDescription,
                category: formCategory,
                inherits_from: formInheritsFrom,
                dpi: formDpi,
                active_dpi_stage: formActiveStage,
                // Preserve created_unix on edit; 0 lets the daemon
                // stamp it on create.
                created_unix: original?.created_unix ?? 0,
                // Preserve bindings on edit — button editing lives
                // in the MouseView profile-mode editor, not in this
                // form. New profiles start with no overrides.
                buttons: original?.buttons ?? [],
            });
            resetForm();
            onprofileschange();
        } catch (error) {
            formError = String(error);
        } finally {
            submitting = false;
        }
    }

    function submitLabel(): string {
        return mode.kind === 'edit' ? 'Save changes' : 'Create profile';
    }

    async function handleDelete(id: string): Promise<void> {
        try {
            await removeProfile(id);
            if (mode.kind === 'edit' && mode.original === id) {
                resetForm();
            }
            onprofileschange();
        } catch (error) {
            formError = `delete ${id}: ${String(error)}`;
        }
    }
</script>

<section class="panel">
    <h2 class="panel-title"><Icon name="gear" /> Profiles</h2>

    <form class="profile-form" onsubmit={handleSubmit}>
        <div class="profile-form-row">
            <label class="profile-form-label">
                <span>id</span>
                <input
                    class="input-field font-mono"
                    bind:value={formId}
                    placeholder="fps-low-dpi"
                    pattern="[a-z0-9_-]+"
                    title="lowercase letters, digits, hyphens, underscores"
                    disabled={mode.kind === 'edit'}
                    required
                />
            </label>
            <label class="profile-form-label">
                <span>name</span>
                <input class="input-field" bind:value={formName} placeholder="FPS — low DPI" required />
            </label>
            <label class="profile-form-label">
                <span>category</span>
                <select class="input-field" bind:value={formCategory}>
                    <option value="agnostic">agnostic</option>
                    <option value="specific">specific</option>
                </select>
            </label>
        </div>

        {#if formCategory === 'specific'}
            <label class="profile-form-label">
                <span>inherits from (agnostic)</span>
                <select class="input-field" bind:value={formInheritsFrom}>
                    <option value="">— none —</option>
                    {#each agnosticProfiles as p (p.id)}
                        <option value={p.id}>{p.id}</option>
                    {/each}
                </select>
            </label>
        {/if}

        <label class="profile-form-label">
            <span>description (optional)</span>
            <input class="input-field" bind:value={formDescription} placeholder="shooter sensitivity baseline" />
        </label>

        <div class="dpi-editor">
            <span class="profile-form-label-text">DPI stages</span>
            <div class="dpi-stages">
                {#each formDpi as dpi, idx (idx)}
                    <div class="dpi-stage" class:dpi-stage-active={idx === formActiveStage}>
                        <input
                            class="input-field dpi-stage-input"
                            type="number"
                            min="50"
                            max="32000"
                            step="50"
                            value={dpi}
                            oninput={(e) => {
                                updateDpiStage(idx, Number((e.target as HTMLInputElement).value));
                            }}
                            aria-label={`DPI stage ${String(idx)}`}
                        />
                        <label class="dpi-stage-active-label">
                            <input
                                type="radio"
                                name="active-stage"
                                checked={idx === formActiveStage}
                                onchange={() => { formActiveStage = idx; }}
                            />
                            active
                        </label>
                        <button
                            class="btn-danger-sm"
                            type="button"
                            onclick={() => { removeDpiStage(idx); }}
                            disabled={formDpi.length === 1}
                            title="Remove stage"
                        >
                            ✕
                        </button>
                    </div>
                {/each}
            </div>
            <button class="btn-ghost-sm" type="button" onclick={addDpiStage}>+ add stage</button>
        </div>

        <div class="profile-form-actions">
            <button class="btn-primary" type="submit" disabled={submitting}>
                {submitting ? '…' : submitLabel()}
            </button>
            {#if mode.kind === 'edit'}
                <button class="btn-ghost" type="button" onclick={resetForm}>
                    Cancel
                </button>
            {/if}
        </div>

        {#if formError !== null}
            <p class="error-text">{formError}</p>
        {/if}
    </form>

    {#if profiles.length > 0}
        <h3 class="panel-subtitle">existing</h3>
        <ul class="profile-list">
            {#each profiles as profile (profile.id)}
                <li class="profile-row">
                    <button
                        class="profile-row-main"
                        type="button"
                        onclick={() => { startEditing(profile); }}
                        title="Edit"
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
</section>
