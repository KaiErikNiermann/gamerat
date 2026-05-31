<script lang="ts">
    import Pencil from '@lucide/svelte/icons/pencil';
    import Icon from './Icon.svelte';
    import { applyBase, applyProfile, removeProfile, upsertProfile } from './ipc.js';
    import Modal from './Modal.svelte';
    import { m } from './paraglide/messages.js';
    import { generateProfileId } from './profile-edit.js';
    import Select from './Select.svelte';
    import type { GameratProfile, SlotInfo } from './types.js';

    interface Props {
        profiles: GameratProfile[];
        /** Currently selected profile (highlighted row, drives
         *  MouseView's edit target). Null when nothing is selected. */
        selectedProfileId: string | null;
        /** When false (manual mode), per-row Apply buttons are
         *  enabled. When true (auto mode), apply is decided by rules,
         *  so the buttons are disabled with an explanatory title. */
        autoswitchEnabled: boolean | null;
        /** DPI summary for the Base / Desktop slot (slot 0) on the
         *  first device, refreshed by the parent on device changes +
         *  profile-switched signals. Null when no device is present
         *  yet or the fetch failed — the Base row falls back to "—"
         *  in that case (same as before this was plumbed). */
        baseDpi: { dpi: readonly number[]; activeStage: number } | null;
        /** Currently-active slot on the first device. Used to render a
         *  small "live now" dot on whichever row corresponds to the
         *  hardware-active profile — Base when `is_desktop`, otherwise
         *  the row whose id matches `profile_id`. Null while unknown
         *  (no device, slot map not yet fetched, etc.) → no dot. */
        activeSlot: SlotInfo | null;
        onprofileschange: () => void;
        onselect: (id: string | null) => void;
    }

    const {
        profiles,
        selectedProfileId,
        autoswitchEnabled,
        baseDpi,
        activeSlot,
        onprofileschange,
        onselect,
    }: Props = $props();

    /** Per-row "is this profile currently live on the device?" check.
     *  Driven by `activeSlot`; deliberately ignores "active slot is
     *  non-desktop but has an empty `profile_id`" (Piper / unmanaged
     *  territory) — in that case nothing in the Profiles panel reads
     *  as active, which matches reality. */
    const baseIsLive = $derived<boolean>(activeSlot?.is_desktop === true);
    function profileIsLive(profileId: string): boolean {
        return (
            activeSlot !== null
            && !activeSlot.is_desktop
            && activeSlot.profile_id === profileId
        );
    }

    /** Render a `dpi` + `activeStage` pair in the same `*active,…`
     *  shape the per-profile rows use, so the Base row visually
     *  aligns with the rest of the list. */
    function formatDpiSummary(
        dpi: readonly number[],
        activeStage: number,
    ): string {
        return dpi
            .map((d, i) => (i === activeStage ? `*${String(d)}` : String(d)))
            .join(',');
    }

    // ───────────────────────────────────────────────────────────────
    // Create / edit modal state. One shared form covers both flows;
    // `editingProfile` distinguishes them — when non-null we're
    // renaming an existing profile (the name + metadata are
    // editable), when null we're creating from scratch.
    //
    // Profile ids are not user-facing — `set_profile` keys by id and
    // rules / allocator / `inherits_from` all reference profiles by
    // id, so changing one would have to fan out atomically across
    // several stores. Instead we auto-generate Discord-style ids
    // (`<slug>-<4 hex chars>`) on create, and the id stays the same
    // for the life of the profile. The user only ever sees the name.
    //
    // DPI stages + button bindings + LED state are NOT edited here;
    // that lives in MouseView once the profile is selected.
    // ───────────────────────────────────────────────────────────────

    let modalOpen = $state(false);
    let editingProfile = $state<GameratProfile | null>(null);
    let formName = $state('');
    let formDescription = $state('');
    let formCategory = $state<'agnostic' | 'specific'>('agnostic');
    let formInheritsFrom = $state('');
    let submitting = $state(false);
    let formError = $state<string | null>(null);

    const agnosticProfiles = $derived(profiles.filter((p) => p.category === 'agnostic'));

    function openCreate(): void {
        modalOpen = true;
        editingProfile = null;
        formName = '';
        formDescription = '';
        formCategory = 'agnostic';
        formInheritsFrom = '';
        formError = null;
    }

    function openEdit(profile: GameratProfile): void {
        modalOpen = true;
        editingProfile = profile;
        formName = profile.name;
        formDescription = profile.description;
        // Profile category arrives as a wire string; the form's
        // category Select is union-typed, so narrow defensively.
        formCategory = profile.category === 'specific' ? 'specific' : 'agnostic';
        formInheritsFrom = profile.inherits_from;
        formError = null;
    }

    function closeModal(): void {
        modalOpen = false;
        editingProfile = null;
    }

    async function handleSubmit(event: SubmitEvent): Promise<void> {
        event.preventDefault();
        if (formName.trim().length === 0) {
            formError = m.profiles_name_required();
            return;
        }
        submitting = true;
        formError = null;
        try {
            // Edit mode preserves the existing profile's id + DPI /
            // buttons / LEDs / soft-macros / created_unix — only
            // metadata is editable here. Create mode auto-generates
            // a fresh id from the name and lays down sensible
            // defaults (single 800 DPI stage, empty everything else)
            // which the user fleshes out in MouseView next.
            const existing = editingProfile;
            const name = formName.trim();
            const payload: GameratProfile = existing === null
                ? {
                    id: generateProfileId(name, new Set(profiles.map((p) => p.id))),
                    name,
                    description: formDescription,
                    category: formCategory,
                    inherits_from: formInheritsFrom,
                    dpi: [800],
                    active_dpi_stage: 0,
                    created_unix: 0,
                    buttons: [],
                    leds: [],
                    soft_macros: [],
                }
                : {
                    ...existing,
                    name,
                    description: formDescription,
                    category: formCategory,
                    inherits_from: formInheritsFrom,
                };
            await upsertProfile(payload);
            onprofileschange();
            // Auto-select on create so the user lands directly in
            // the editor. Skipped on edit — the selection state is
            // the user's own; clobbering it would be surprising.
            if (existing === null) onselect(payload.id);
            closeModal();
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
            formError = m.profiles_delete_error({ id, error: String(error) });
        }
    }

    async function handleApply(id: string): Promise<void> {
        try {
            await applyProfile(id);
            onprofileschange();
        } catch (error) {
            formError = m.profiles_apply_error({ id, error: String(error) });
        }
    }

    async function handleApplyBase(): Promise<void> {
        try {
            await applyBase();
            onprofileschange();
        } catch (error) {
            formError = m.profiles_apply_base_error({ error: String(error) });
        }
    }

    /** Localized label for a profile category badge / option. */
    function categoryLabel(category: string): string {
        return category === 'specific'
            ? m.profiles_category_specific()
            : m.profiles_category_agnostic();
    }

    function applyTitle(): string {
        if (autoswitchEnabled === null) return m.common_daemon_offline();
        return autoswitchEnabled
            ? m.profiles_apply_title_auto()
            : m.profiles_apply_title_manual();
    }

    function applyBaseTitle(): string {
        if (autoswitchEnabled === null) return m.common_daemon_offline();
        return autoswitchEnabled
            ? m.profiles_apply_base_title_auto()
            : m.profiles_apply_base_title_manual();
    }
</script>

<section class="panel">
    <header class="profiles-header">
        <h2 class="panel-title"><Icon name="gear" /> {m.profiles_title()}</h2>
        <button class="btn-primary btn-sm" type="button" onclick={openCreate}>{m.profiles_new()}</button>
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
                title={m.profiles_base_title()}
            >
                <span class="profile-row-name">
                    {#if baseIsLive}
                        <span
                            class="profile-row-live-dot"
                            aria-label={m.profiles_live()}
                            title={m.profiles_live()}
                        ></span>
                    {/if}
                    {m.profiles_base_name()}
                </span>
                <span class="profile-row-category" data-category="agnostic">{m.profiles_base_category()}</span>
                <span class="profile-row-dpi font-mono">
                    {baseDpi === null
                        ? '—'
                        : formatDpiSummary(baseDpi.dpi, baseDpi.activeStage)}
                </span>
            </button>
            <button
                class="btn-ghost-sm profile-row-apply"
                type="button"
                onclick={() => { void handleApplyBase(); }}
                disabled={autoswitchEnabled !== false}
                title={applyBaseTitle()}
            >
                {m.common_apply()}
            </button>
            <!-- Visibility-hidden mirrors of the Edit + Delete buttons:
                 occupy exactly the same layout footprint as the real
                 buttons on the per-profile rows below (without
                 hand-tuned fixed widths that drift from the button
                 styling), but are removed from the accessibility
                 tree and focus order. Click handlers are no-ops; the
                 buttons aren't interactive while hidden. -->
            <button
                class="btn-ghost-sm profile-row-edit"
                type="button"
                style="visibility: hidden"
                tabindex={-1}
                aria-hidden="true"
            >
                <Pencil size={14} />
            </button>
            <button
                class="btn-danger-sm"
                type="button"
                style="visibility: hidden"
                tabindex={-1}
                aria-hidden="true"
            >
                ✕
            </button>
        </li>

        {#if profiles.length === 0}
            <li class="profile-row profile-row-empty-hint">
                <p class="muted text-xs">{m.profiles_empty()}</p>
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
                        title={m.profiles_select_title()}
                    >
                        <span class="profile-row-name" title={profile.name}>
                            {#if profileIsLive(profile.id)}
                                <span
                                    class="profile-row-live-dot"
                                    aria-label={m.profiles_live()}
                                    title={m.profiles_live()}
                                ></span>
                            {/if}
                            {profile.name}
                        </span>
                        <span class="profile-row-category" data-category={profile.category}>
                            {categoryLabel(profile.category)}
                        </span>
                        <span class="profile-row-dpi font-mono">
                            {formatDpiSummary(profile.dpi, profile.active_dpi_stage)}
                        </span>
                    </button>
                    <button
                        class="btn-ghost-sm profile-row-apply"
                        type="button"
                        onclick={() => { void handleApply(profile.id); }}
                        disabled={autoswitchEnabled !== false}
                        title={applyTitle()}
                    >
                        {m.common_apply()}
                    </button>
                    <button
                        class="btn-ghost-sm profile-row-edit"
                        type="button"
                        onclick={() => { openEdit(profile); }}
                        aria-label={m.profiles_edit_aria({ name: profile.name })}
                        title={m.profiles_edit_title()}
                    >
                        <Pencil size={14} />
                    </button>
                    <button
                        class="btn-danger-sm"
                        type="button"
                        onclick={() => { void handleDelete(profile.id); }}
                        aria-label={m.profiles_delete_aria({ name: profile.name })}
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
    {@const isEdit = editingProfile !== null}
    <Modal
        label={isEdit ? m.profiles_modal_edit() : m.profiles_modal_create()}
        onclose={closeModal}
    >
        <form class="binding-editor-card" onsubmit={handleSubmit}>
            <header class="binding-editor-head">
                <h3 class="binding-editor-title">
                    {#if isEdit}
                        {m.profiles_modal_edit_named({ name: editingProfile?.name ?? '' })}
                    {:else}
                        {m.profiles_modal_new_title()}
                    {/if}
                </h3>
                <button
                    type="button"
                    class="btn-ghost-sm"
                    onclick={closeModal}
                    aria-label={m.common_close()}
                >
                    {m.profiles_close()}
                </button>
            </header>

            <label class="binding-editor-row">
                <span class="binding-editor-label">{m.profiles_form_name()}</span>
                <input
                    class="input-field"
                    bind:value={formName}
                    placeholder={m.profiles_name_placeholder()}
                    required
                />
            </label>

            <label class="binding-editor-row">
                <span class="binding-editor-label">{m.profiles_form_category()}</span>
                <Select
                    bind:value={formCategory}
                    options={[
                        { value: 'agnostic', label: m.profiles_category_agnostic() },
                        { value: 'specific', label: m.profiles_category_specific() },
                    ]}
                    ariaLabel={m.profiles_category_aria()}
                />
            </label>

            {#if formCategory === 'specific'}
                <label class="binding-editor-row">
                    <span class="binding-editor-label">{m.profiles_form_inherits()}</span>
                    <Select
                        bind:value={formInheritsFrom}
                        options={[
                            { value: '', label: m.profiles_inherits_none() },
                            ...agnosticProfiles.map((p) => ({ value: p.id, label: p.name })),
                        ]}
                        ariaLabel={m.profiles_inherits_aria()}
                    />
                </label>
            {/if}

            <label class="binding-editor-row">
                <span class="binding-editor-label">{m.profiles_form_description()}</span>
                <input
                    class="input-field"
                    bind:value={formDescription}
                    placeholder={m.profiles_desc_placeholder()}
                />
            </label>

            {#if formError !== null}
                <p class="error-text">{formError}</p>
            {/if}

            <footer class="binding-editor-actions">
                <button class="btn-ghost" type="button" onclick={closeModal}>{m.common_cancel()}</button>
                <button class="btn-primary" type="submit" disabled={submitting}>
                    {#if submitting}
                        {isEdit ? m.profiles_saving() : m.profiles_creating()}
                    {:else}
                        {isEdit ? m.common_save() : m.profiles_create_edit()}
                    {/if}
                </button>
            </footer>
        </form>
    </Modal>
{/if}
