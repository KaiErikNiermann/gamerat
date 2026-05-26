<script lang="ts">
    import { SvelteMap } from 'svelte/reactivity';
    import Icon from './Icon.svelte';
    import { addRule, removeRule } from './ipc.js';
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
    }

    const { games, profiles, rules, onruleschange }: Props = $props();

    let filterText = $state('');
    let launcherFilter = $state<string | null>(null);

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
    </div>

    <div class="launcher-chips" role="tablist" aria-label="Filter by launcher">
        <button
            class="chip"
            class:chip-active={launcherFilter === null}
            type="button"
            onclick={() => { launcherFilter = null; }}
        >
            all <span class="chip-count">{games.length}</span>
        </button>
        {#each ['steam', 'lutris', 'heroic', 'other'] as tag (tag)}
            {#if (launcherCounts.get(tag) ?? 0) > 0}
                <button
                    class="chip"
                    class:chip-active={launcherFilter === tag}
                    type="button"
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
                    {#if err !== undefined}
                        <span class="game-row-error" title={err}>!</span>
                    {/if}
                </li>
            {/each}
        </ul>
        <p class="muted games-summary">{visible.length} of {games.length} shown</p>
    {/if}
</section>
