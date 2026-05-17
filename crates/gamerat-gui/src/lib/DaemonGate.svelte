<script lang="ts">
    /**
     * Full-screen blocker shown when the gamerat-daemon isn't on the
     * session bus. App.svelte mounts this above the rest of the UI
     * whenever its health check is in 'checking' / 'offline' state,
     * dismissing it on the transition back to 'online'.
     *
     * Why block the whole UI: the GUI is a thin client over the
     * daemon's D-Bus interface — without it there's literally nothing
     * to show (no rules, no devices, no buttons). Rather than
     * silently showing empty panels and a "(no events yet)" hint,
     * surface the missing piece up-front so the user knows the next
     * action is "start the daemon", not "configure something".
     */

    interface Props {
        /** 'checking' on first paint, 'offline' once a ping fails. */
        state: 'checking' | 'offline';
        /** Last error string from the ping helper, if any. */
        lastError: string | null;
        /** Approximate seconds since the last ping attempt. */
        secondsSinceLastPing: number;
    }

    const { state, lastError, secondsSinceLastPing }: Props = $props();
</script>

<div class="daemon-gate" role="alertdialog" aria-modal="true" aria-labelledby="daemon-gate-title">
    <div class="daemon-gate-card">
        <div class="daemon-gate-spinner" aria-hidden="true">
            <span></span>
            <span></span>
            <span></span>
        </div>

        <h2 class="daemon-gate-title" id="daemon-gate-title">
            {state === 'checking' ? 'Connecting to gamerat-daemon…' : 'gamerat-daemon is not running'}
        </h2>

        <p class="daemon-gate-body">
            The GUI talks to the daemon over D-Bus. Until it's up there's no live
            state to show (rules, profiles, and games live in config files, but the
            mouse, focus events, and bindings need the daemon).
        </p>

        <pre class="daemon-gate-cmd">cargo run -p gamerat-daemon</pre>

        <p class="daemon-gate-body muted text-xs">
            Or if you've installed the systemd unit:
        </p>
        <pre class="daemon-gate-cmd">systemctl --user start gamerat</pre>

        <div class="daemon-gate-status muted text-xs">
            <span>Checking every few seconds — last attempt {secondsSinceLastPing}s ago.</span>
            {#if lastError !== null}
                <details>
                    <summary>Last error</summary>
                    <code>{lastError}</code>
                </details>
            {/if}
        </div>
    </div>
</div>
