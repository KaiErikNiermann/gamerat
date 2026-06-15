<script lang="ts">
    import { writeAutoswitch } from './ipc.js';
    import { m } from './paraglide/messages.js';

    /**
     * Header pill that mirrors the daemon's AutoSwitchEnabled flag.
     * Controlled by App.svelte: `enabled` comes in as a prop, and
     * `onchange` reports back the new value after a successful write.
     * The previous self-owning version lived behind a stale state
     * that wasn't visible to ProfilesPanel / MouseView / Apply gating.
     *
     * Visually transparent — copy reads "Auto" / "Manual" so the
     * user can tell at a glance which mode they're in.
     */

    interface Props {
        /** Current daemon-side flag. `null` while the initial fetch is
         *  in flight or after the daemon went away. */
        enabled: boolean | null;
        /** Called with the new value after a successful toggle write. */
        onchange: (value: boolean) => void;
    }

    const { enabled, onchange }: Props = $props();

    let pending = $state(false);
    let error = $state<string | null>(null);

    async function toggle(): Promise<void> {
        if (enabled === null) return;
        pending = true;
        error = null;
        const next = !enabled;
        try {
            const applied = await writeAutoswitch(next);
            onchange(applied);
        } catch (error_) {
            error = String(error_);
        } finally {
            pending = false;
        }
    }

    function label(): string {
        if (enabled === null) return '…';
        return m[enabled ? 'autoswitch_auto' : 'autoswitch_manual']();
    }

    function title(): string {
        if (enabled === null) {
            return m.autoswitch_reading();
        }
        if (error !== null) {
            return m.autoswitch_failed({ error });
        }
        return m[enabled ? 'autoswitch_on_title' : 'autoswitch_off_title']();
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
