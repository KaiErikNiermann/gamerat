<script lang="ts">
    import { describeAction, kindName, SPECIAL_OPTIONS } from './button-labels.js';
    import { BUTTON_ACTION_KIND } from './types.js';
    import type { ButtonAction, RatbagButton } from './types.js';

    interface Props {
        button: RatbagButton;
        onsave: (action: ButtonAction) => Promise<void> | void;
        onclose: () => void;
    }

    const { button, onsave, onclose }: Props = $props();

    // Local working copy of the action. Initialised from the
    // current binding so the user can tweak rather than re-type
    // from scratch.
    let workingKind = $state<number>(button.action.kind);
    let workingValue = $state<number>(button.action.value);
    let macroText = $state<string>(macroStepsToText(button.action.macro_steps));
    let saving = $state(false);
    let error = $state<string | null>(null);

    function macroStepPrefix(kind: number): string {
        if (kind === 1) return 'p';
        if (kind === 2) return 'r';
        if (kind === 3) return 'w';
        return '?';
    }

    function macroStepsToText(steps: readonly { kind: number; value: number }[]): string {
        return steps
            .map((s) => `${macroStepPrefix(s.kind)}:${String(s.value)}`)
            .join(', ');
    }

    function tagToKind(tag: string): number {
        if (tag === 'p') return 1;
        if (tag === 'r') return 2;
        return 3; // 'w'
    }

    function macroTextToSteps(text: string): { kind: number; value: number }[] {
        // Mini DSL: comma-separated entries of "kind:value" where
        // kind ∈ {p, r, w}. Whitespace tolerant. Empty input ⇒ no
        // steps. Invalid tokens throw so the user gets a clear
        // error before we hit the daemon.
        const entries = text
            .split(/[,;\n]/u)
            .map((s) => s.trim())
            .filter((s) => s.length > 0);
        const steps: { kind: number; value: number }[] = [];
        for (const entry of entries) {
            const match = /^([prw])\s*:\s*(\d+)$/iu.exec(entry);
            if (match === null) {
                throw new Error(`bad macro step "${entry}" (use p:30, r:30, w:25)`);
            }
            const tag = match[1]?.toLowerCase() ?? '';
            const value = Number.parseInt(match[2] ?? '0', 10);
            const kind = tagToKind(tag);
            steps.push({ kind, value });
        }
        return steps;
    }

    const supportedKinds = $derived<readonly number[]>(button.supported_action_types);

    function buildAction(): ButtonAction {
        switch (workingKind) {
            case BUTTON_ACTION_KIND.MACRO: {
                return {
                    kind: BUTTON_ACTION_KIND.MACRO,
                    value: 0,
                    macro_steps: macroTextToSteps(macroText),
                };
            }
            case BUTTON_ACTION_KIND.NONE: {
                return { kind: BUTTON_ACTION_KIND.NONE, value: 0, macro_steps: [] };
            }
            default: {
                return {
                    kind: workingKind as ButtonAction['kind'],
                    value: workingValue,
                    macro_steps: [],
                };
            }
        }
    }

    async function handleSave(event: Event): Promise<void> {
        event.preventDefault();
        saving = true;
        error = null;
        try {
            const action = buildAction();
            await onsave(action);
            onclose();
        } catch (error_) {
            error = String(error_);
        } finally {
            saving = false;
        }
    }
</script>

<div
    class="binding-editor-backdrop"
    role="dialog"
    aria-modal="true"
    aria-label={`Edit binding for button ${String(button.index)}`}
    onclick={(e) => {
        // Click outside the card → cancel.
        if (e.target === e.currentTarget) onclose();
    }}
    onkeydown={(e) => {
        if (e.key === 'Escape') onclose();
    }}
    tabindex="-1"
>
    <form class="binding-editor-card" onsubmit={handleSave}>
        <header class="binding-editor-head">
            <h3 class="binding-editor-title">
                Button {button.index} binding
            </h3>
            <button
                type="button"
                class="btn-ghost-sm"
                onclick={onclose}
                aria-label="Close binding editor"
            >
                close
            </button>
        </header>

        <p class="muted text-xs binding-editor-current">
            Currently: {describeAction(button.action)}
        </p>

        <label class="binding-editor-row">
            <span class="binding-editor-label">Kind</span>
            <select class="input-field" bind:value={workingKind}>
                {#each supportedKinds as kind (kind)}
                    <option value={kind}>{kindName(kind)}</option>
                {/each}
            </select>
        </label>

        {#if workingKind === BUTTON_ACTION_KIND.MOUSE}
            <label class="binding-editor-row">
                <span class="binding-editor-label">Mouse button index</span>
                <input
                    class="input-field"
                    type="number"
                    min="0"
                    max="15"
                    bind:value={workingValue}
                />
            </label>
        {:else if workingKind === BUTTON_ACTION_KIND.SPECIAL}
            <label class="binding-editor-row">
                <span class="binding-editor-label">Special action</span>
                <select class="input-field" bind:value={workingValue}>
                    {#each SPECIAL_OPTIONS as opt (opt.value)}
                        <option value={opt.value}>{opt.label}</option>
                    {/each}
                </select>
            </label>
        {:else if workingKind === BUTTON_ACTION_KIND.KEY}
            <label class="binding-editor-row">
                <span class="binding-editor-label">Linux keycode</span>
                <input
                    class="input-field"
                    type="number"
                    min="1"
                    max="767"
                    bind:value={workingValue}
                />
                <small class="muted text-xs">
                    See <code>linux/input-event-codes.h</code>. Examples: 30 = KEY_A,
                    57 = Space, 28 = Enter.
                </small>
            </label>
        {:else if workingKind === BUTTON_ACTION_KIND.MACRO}
            <label class="binding-editor-row">
                <span class="binding-editor-label">
                    Macro steps (DSL: p:CODE, r:CODE, w:MS — comma-separated)
                </span>
                <textarea
                    class="input-field binding-editor-macro"
                    rows="3"
                    bind:value={macroText}
                    placeholder="p:30, w:25, r:30"
                ></textarea>
            </label>
        {/if}

        {#if error !== null}
            <p class="error-text">{error}</p>
        {/if}

        <footer class="binding-editor-actions">
            <button class="btn-ghost" type="button" onclick={onclose}>Cancel</button>
            <button class="btn-primary" type="submit" disabled={saving}>
                {saving ? 'Saving…' : 'Save binding'}
            </button>
        </footer>
    </form>
</div>
