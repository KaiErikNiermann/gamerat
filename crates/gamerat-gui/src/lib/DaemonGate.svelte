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

    import { m } from './paraglide/messages.js';

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
            {state === 'checking' ? m.gate_connecting() : m.gate_offline()}
        </h2>

        <p class="daemon-gate-body">{m.gate_body()}</p>

        <pre class="daemon-gate-cmd">cargo run -p gamerat-daemon</pre>

        <p class="daemon-gate-body muted text-xs">{m.gate_systemd_hint()}</p>
        <pre class="daemon-gate-cmd">systemctl --user start gamerat</pre>

        <div class="daemon-gate-status muted text-xs">
            <span>{m.gate_checking({ seconds: secondsSinceLastPing })}</span>
            {#if lastError !== null}
                <details>
                    <summary>{m.gate_last_error()}</summary>
                    <code>{lastError}</code>
                </details>
            {/if}
        </div>
    </div>
</div>
