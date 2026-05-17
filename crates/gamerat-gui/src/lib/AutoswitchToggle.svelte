<script lang="ts">
    import { onMount } from 'svelte';
    import { fetchAutoswitch, writeAutoswitch } from './ipc.js';

    /**
     * Header pill that mirrors the daemon's AutoSwitchEnabled flag.
     * Click toggles it; the daemon persists the new value to
     * `$XDG_CONFIG_HOME/gamerat/settings.toml`.
     *
     * Visually transparent — copy reads "Auto" / "Manual" so the
     * user can tell at a glance which mode they're in without
     * decoding an icon. Disabled while pending so rapid clicks
     * don't double-toggle past the daemon's write.
     */

    let enabled = $state<boolean | null>(null);
    let pending = $state(false);
    let error = $state<string | null>(null);

    onMount(() => {
        void (async () => {
            try {
                enabled = await fetchAutoswitch();
            } catch (error_) {
                error = String(error_);
                enabled = null;
            }
        })();
    });

    async function toggle(): Promise<void> {
        if (enabled === null) return;
        pending = true;
        error = null;
        const next = !enabled;
        try {
            enabled = await writeAutoswitch(next);
        } catch (error_) {
            error = String(error_);
        } finally {
            pending = false;
        }
    }

    function label(): string {
        if (enabled === null) return '…';
        return enabled ? 'Auto' : 'Manual';
    }

    function title(): string {
        if (enabled === null) {
            return 'Reading daemon AutoSwitch state…';
        }
        if (error !== null) {
            return `Toggle failed: ${error}`;
        }
        return enabled
            ? 'Rule-driven profile switching is enabled. Click to disable.'
            : 'Manual mode — focus events do NOT switch profiles. Click to enable autoswitch.';
    }
</script>

<button
    type="button"
    class="autoswitch-toggle"
    class:autoswitch-on={enabled === true}
    class:autoswitch-off={enabled === false}
    class:autoswitch-error={error !== null}
    disabled={pending || enabled === null}
    onclick={toggle}
    aria-label={title()}
    title={title()}
>
    <span class="autoswitch-dot" aria-hidden="true"></span>
    {label()}
</button>
