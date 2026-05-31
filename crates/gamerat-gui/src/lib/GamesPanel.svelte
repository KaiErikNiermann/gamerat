<script lang="ts">
    import { SvelteMap } from 'svelte/reactivity';
    import Icon from './Icon.svelte';
    import {
        addManualGame,
        addRule,
        removeManualGame,
        removeRule,
        rescanGames,
    } from './ipc.js';
    import Modal from './Modal.svelte';
    import Select from './Select.svelte';
    import type { GameEntry, GameratProfile, Rule } from './types.js';

    interface Props {
        games: GameEntry[];
        profiles: GameratProfile[];
        /** Authoritative rules list from the daemon — the per-game
         *  dropdown derives its current value from this so deletions
         *  in RulesPanel are reflected immediately. */
        rules: Rule[];
        onruleschange: () => void;
        /** Re-fetch games from the daemon. Fired after a rescan or a
         *  manual add/remove so the list reflects the new daemon state.
         *  The daemon is the source of truth (it owns the merge of
         *  scanned + manual), so we always re-pull rather than mutating
         *  the prop locally. */
        ongameschange: () => void;
    }

    const { games, profiles, rules, onruleschange, ongameschange }: Props = $props();

    let filterText = $state('');
    let launcherFilter = $state<string | null>(null);

    // ── Rescan ──────────────────────────────────────────────────────
    let rescanning = $state(false);
    let rescanError = $state<string | null>(null);

    async function handleRescan(): Promise<void> {
        rescanning = true;
        rescanError = null;
        try {
            await rescanGames();
            ongameschange();
        } catch (error) {
            rescanError = String(error);
        } finally {
            rescanning = false;
        }
    }

    // ── Add manual game ─────────────────────────────────────────────
    let showAddModal = $state(false);
    let manualName = $state('');
    let manualPath = $state('');
    let manualAppId = $state('');
    let savingManual = $state(false);
    let manualError = $state<string | null>(null);

    /** Suggest a window-match glob from the pasted path's leaf folder
     *  when the user hasn't typed one. Wine/native installs commonly
     *  surface a WM_CLASS resembling the install-dir basename, so it's
     *  a reasonable seed the user can refine — never overwrites a value
     *  they've already entered. */
    function suggestAppIdFromPath(): void {
        if (manualAppId.trim().length > 0) return;
        const leaf = manualPath.split('/').findLast((s) => s.length > 0) ?? '';
        if (leaf.length > 0) manualAppId = leaf;
    }

    function openAddModal(): void {
        manualName = '';
        manualPath = '';
        manualAppId = '';
        manualError = null;
        showAddModal = true;
    }

    function closeAddModal(): void {
        if (savingManual) return;
        showAddModal = false;
    }

    async function handleAddManual(event: Event): Promise<void> {
        event.preventDefault();
        if (manualName.trim().length === 0) {
            manualError = 'Name is required.';
            return;
        }
        savingManual = true;
        manualError = null;
        try {
            await addManualGame(manualName.trim(), manualPath.trim(), manualAppId.trim());
            ongameschange();
            showAddModal = false;
        } catch (error) {
            manualError = String(error);
        } finally {
            savingManual = false;
        }
    }

    async function handleRemoveManual(game: GameEntry): Promise<void> {
        pending.set(game.id, true);
        errorMsg.delete(game.id);
        try {
            await removeManualGame(game.id);
            ongameschange();
        } catch (error) {
            errorMsg.set(game.id, String(error));
        } finally {
            pending.delete(game.id);
        }
    }

    /** In-flight set/clear keyed by game id so the dropdown disables
     *  briefly during the round-trip and doesn't fire a second
     *  request before the first lands. */
    const pending = new SvelteMap<string, boolean>();
    /** Per-row error text (sticky until the next successful change). */
    const errorMsg = new SvelteMap<string, string>();

    /** Quick lookup from app_id_glob → rule. The daemon stores rules
     *  by the glob, and a game's `app_id_hint` is the glob it'll
     *  appear as on focus events, so we key directly on that. */
    const ruleByGlob = $derived.by(() => {
        const map = new SvelteMap<string, Rule>();
        for (const r of rules) map.set(r.app_id_glob, r);
        return map;
    });

    function dropdownTitle(game: GameEntry): string {
        if (game.app_id_hint.length === 0) {
            return 'No app_id_hint — add a rule manually in RULES.';
        }
        if (profiles.length === 0) return 'Create a profile first';
        return (
            'Pick the profile to apply when this game gets focus. ' +
            "Choose 'base' to remove the rule and fall back to the desktop slot."
        );
    }

    const visible = $derived.by(() => {
        const needle = filterText.trim().toLowerCase();
        return games
            .filter((g) => launcherFilter === null || g.launcher === launcherFilter)
            .filter((g) => needle.length === 0 || g.name.toLowerCase().includes(needle))
            .toSorted(
                (a, b) => a.launcher.localeCompare(b.launcher) || a.name.localeCompare(b.name),
            );
    });

    const launcherCounts = $derived.by(() => {
        const counts = new SvelteMap<string, number>();
        for (const g of games) counts.set(g.launcher, (counts.get(g.launcher) ?? 0) + 1);
        return counts;
    });

    /** What the dropdown should show for this game: the existing
     *  rule's profile id, or '' (= "base") when no rule exists. */
    function selectedFor(game: GameEntry): string {
        if (game.app_id_hint.length === 0) return '';
        return ruleByGlob.get(game.app_id_hint)?.profile_id ?? '';
    }

    async function handleChange(game: GameEntry, next: string): Promise<void> {
        if (game.app_id_hint.length === 0) return;
        const current = selectedFor(game);
        if (current === next) return;
        pending.set(game.id, true);
        errorMsg.delete(game.id);
        try {
            // base = no rule (delete the existing one); anything else
            // upserts via addRule (replaces by glob).
            await (next.length === 0
                ? removeRule(game.app_id_hint)
                : addRule(game.app_id_hint, next));
            onruleschange();
        } catch (error) {
            errorMsg.set(game.id, String(error));
        } finally {
            pending.delete(game.id);
        }
    }
</script>

<section class="panel">
    <h2 class="panel-title"><Icon name="gamepad" /> Discovered Games</h2>

    <div class="games-controls">
        <input
            class="input-field flex-1"
            type="search"
            bind:value={filterText}
            placeholder="filter by name…"
            aria-label="Filter games"
        />
        <button
            class="btn-ghost-sm"
            type="button"
            onclick={handleRescan}
            disabled={rescanning}
            title="Re-run the Steam / Lutris / Heroic scanners. Use this if a game is missing — e.g. its library drive mounted after the daemon started."
        >
            {rescanning ? 'Rescanning…' : 'Rescan'}
        </button>
        <button
            class="btn-ghost-sm"
            type="button"
            onclick={openAddModal}
            title="Manually add a game whose folder the scanners can't find."
        >
            + Manual
        </button>
    </div>

    {#if rescanError !== null}
        <p class="error-text text-xs">rescan failed: {rescanError}</p>
    {/if}

    <!-- role="group" (not tablist): these are filter toggle buttons with
         no associated tabpanels, so aria-pressed conveys the active state. -->
    <div class="launcher-chips" role="group" aria-label="Filter by launcher">
        <button
            class="chip"
            class:chip-active={launcherFilter === null}
            type="button"
            aria-pressed={launcherFilter === null}
            onclick={() => { launcherFilter = null; }}
        >
            all <span class="chip-count">{games.length}</span>
        </button>
        {#each ['steam', 'lutris', 'heroic', 'manual', 'other'] as tag (tag)}
            {#if (launcherCounts.get(tag) ?? 0) > 0}
                <button
                    class="chip"
                    class:chip-active={launcherFilter === tag}
                    type="button"
                    aria-pressed={launcherFilter === tag}
                    onclick={() => { launcherFilter = tag; }}
                >
                    {tag} <span class="chip-count">{launcherCounts.get(tag) ?? 0}</span>
                </button>
            {/if}
        {/each}
    </div>

    {#if profiles.length === 0 && games.length > 0}
        <p class="muted text-xs mb-2">
            No profiles yet — the per-game profile picker is disabled until you
            create one in the Profiles panel.
        </p>
    {/if}

    {#if games.length === 0}
        <p class="muted">No games discovered. (Are Steam / Lutris / Heroic installed?)</p>
    {:else if visible.length === 0}
        <p class="muted">No games match the current filter.</p>
    {:else}
        <ul class="games-list">
            {#each visible as game (game.id)}
                {@const selected = selectedFor(game)}
                {@const isPending = pending.get(game.id) === true}
                {@const err = errorMsg.get(game.id)}
                <li class="game-row">
                    <span class="launcher-badge launcher-badge-{game.launcher}">
                        {game.launcher}
                    </span>
                    <span class="game-name" title={game.id}>{game.name}</span>
                    <span class="game-hint font-mono">
                        {game.app_id_hint.length === 0 ? '—' : game.app_id_hint}
                    </span>
                    <Select
                        className="game-profile"
                        value={selected}
                        onchange={(v: string) => {
                            void handleChange(game, v);
                        }}
                        options={[
                            { value: '', label: 'base' },
                            ...profiles.map((p) => ({ value: p.id, label: p.name })),
                        ]}
                        disabled={isPending
                            || game.app_id_hint.length === 0
                            || profiles.length === 0}
                        ariaLabel={`Profile for ${game.name}`}
                        title={dropdownTitle(game)}
                    />
                    {#if game.launcher === 'manual' || err !== undefined}
                        <!-- Trailing affordances share one grid cell so
                             the 5-column row layout holds regardless of
                             which (or both) are present. -->
                        <div class="game-row-trailing">
                            {#if game.launcher === 'manual'}
                                <button
                                    class="btn-danger-sm game-remove-manual"
                                    type="button"
                                    onclick={() => { void handleRemoveManual(game); }}
                                    disabled={isPending}
                                    title="Remove this manual game entry"
                                    aria-label={`Remove manual game ${game.name}`}
                                >
                                    ✕
                                </button>
                            {/if}
                            {#if err !== undefined}
                                <span class="game-row-error" title={err}>!</span>
                            {/if}
                        </div>
                    {/if}
                </li>
            {/each}
        </ul>
        <p class="muted games-summary">{visible.length} of {games.length} shown</p>
    {/if}
</section>

{#if showAddModal}
    <Modal label="Add manual game" onclose={closeAddModal}>
        <form class="binding-editor-card" onsubmit={handleAddManual}>
            <header class="binding-editor-head">
                <h3 class="binding-editor-title">Add manual game</h3>
                <button
                    type="button"
                    class="btn-ghost-sm"
                    onclick={closeAddModal}
                    aria-label="Close"
                >
                    close
                </button>
            </header>

            <p class="muted text-xs">
                For games the Steam / Lutris / Heroic scanners can't find.
                The window match is what actually drives the profile —
                the path is informational.
            </p>

            <label class="binding-editor-row">
                <span class="binding-editor-label">Name</span>
                <input
                    class="input-field"
                    type="text"
                    bind:value={manualName}
                    placeholder="My Game"
                    aria-label="Game name"
                />
            </label>

            <label class="binding-editor-row">
                <span class="binding-editor-label">Install path</span>
                <input
                    class="input-field font-mono"
                    type="text"
                    bind:value={manualPath}
                    onblur={suggestAppIdFromPath}
                    spellcheck="false"
                    autocomplete="off"
                    placeholder="/mnt/games/MyGame"
                    aria-label="Install path"
                />
            </label>

            <label class="binding-editor-row">
                <span class="binding-editor-label">
                    Window match
                    <span class="muted text-xs">(app_id / WM_CLASS)</span>
                </span>
                <input
                    class="input-field font-mono"
                    type="text"
                    bind:value={manualAppId}
                    spellcheck="false"
                    autocomplete="off"
                    placeholder="mygame.exe"
                    aria-label="Window match glob"
                />
            </label>
            <p class="muted text-xs">
                Tip: focus the game, then check the FOCUS panel for the
                app_id it reports — that's the value to paste here.
            </p>

            {#if manualError !== null}
                <p class="error-text">{manualError}</p>
            {/if}

            <footer class="binding-editor-actions">
                <button class="btn-ghost" type="button" onclick={closeAddModal}>Cancel</button>
                <button class="btn-primary" type="submit" disabled={savingManual}>
                    {savingManual ? 'Adding…' : 'Add game'}
                </button>
            </footer>
        </form>
    </Modal>
{/if}
