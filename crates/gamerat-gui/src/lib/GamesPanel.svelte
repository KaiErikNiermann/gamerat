<script lang="ts">
    import { SvelteMap } from 'svelte/reactivity';
    import Icon from './Icon.svelte';
    import { addRule } from './ipc.js';
    import type { GameEntry, GameratProfile } from './types.js';

    interface Props {
        games: GameEntry[];
        profiles: GameratProfile[];
        onruleschange: () => void;
    }

    const { games, profiles, onruleschange }: Props = $props();

    let filterText = $state('');
    let launcherFilter = $state<string | null>(null);

    // Per-game keyed state. SvelteMap sidesteps both
    // security/detect-object-injection (on string-key access) and the
    // svelte/prefer-svelte-reactivity rule that bans plain Map.
    const profileChoice = new SvelteMap<string, string>();
    const pending = new SvelteMap<string, boolean>();
    const flash = new SvelteMap<string, string>();

    const visible = $derived.by(() => {
        const needle = filterText.trim().toLowerCase();
        return games
            .filter((g) => launcherFilter === null || g.launcher === launcherFilter)
            .filter((g) => needle.length === 0 || g.name.toLowerCase().includes(needle))
            .sort(
                (a, b) => a.launcher.localeCompare(b.launcher) || a.name.localeCompare(b.name),
            );
    });

    const launcherCounts = $derived.by(() => {
        const counts = new SvelteMap<string, number>();
        for (const g of games) counts.set(g.launcher, (counts.get(g.launcher) ?? 0) + 1);
        return counts;
    });

    async function handleAdd(game: GameEntry): Promise<void> {
        if (game.app_id_hint.length === 0) {
            flash.set(game.id, 'no app_id_hint — add manually');
            return;
        }
        const profileId = profileChoice.get(game.id) ?? '';
        if (profileId.length === 0) {
            flash.set(game.id, 'pick a profile first');
            return;
        }
        pending.set(game.id, true);
        flash.delete(game.id);
        try {
            await addRule(game.app_id_hint, profileId);
            flash.set(game.id, '✓');
            onruleschange();
        } catch (error) {
            flash.set(game.id, String(error));
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
                <li class="game-row">
                    <span class="launcher-badge launcher-badge-{game.launcher}">
                        {game.launcher}
                    </span>
                    <span class="game-name" title={game.id}>{game.name}</span>
                    <span class="game-hint font-mono">
                        {game.app_id_hint.length === 0 ? '—' : game.app_id_hint}
                    </span>
                    <select
                        class="input-field game-profile"
                        value={profileChoice.get(game.id) ?? ''}
                        onchange={(e) => {
                            profileChoice.set(
                                game.id,
                                (e.target as HTMLSelectElement).value,
                            );
                        }}
                        disabled={profiles.length === 0}
                        aria-label="Profile for {game.name}"
                        title={profiles.length === 0 ? 'Create a profile first' : ''}
                    >
                        <option value="" disabled selected>
                            {profiles.length === 0 ? 'no profiles' : 'profile…'}
                        </option>
                        {#each profiles as profile (profile.id)}
                            <option value={profile.id}>{profile.id}</option>
                        {/each}
                    </select>
                    <button
                        class="btn-primary btn-sm"
                        type="button"
                        disabled={pending.get(game.id) === true
                            || game.app_id_hint.length === 0
                            || profiles.length === 0}
                        onclick={() => { void handleAdd(game); }}
                        title="Add rule: {game.app_id_hint} → {profileChoice.get(game.id) ?? '(pick profile)'}"
                    >
                        + rule
                    </button>
                    {#if flash.has(game.id)}
                        <span
                            class="game-flash"
                            class:game-flash-ok={flash.get(game.id) === '✓'}
                            class:game-flash-err={flash.get(game.id) !== '✓'}
                        >
                            {flash.get(game.id)}
                        </span>
                    {/if}
                </li>
            {/each}
        </ul>
        <p class="muted games-summary">{visible.length} of {games.length} shown</p>
    {/if}
</section>
