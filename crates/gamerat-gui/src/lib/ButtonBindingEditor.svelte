<script lang="ts">
    import { describeAction, kindName, SPECIAL_OPTIONS } from './button-labels.js';
    import KeyCapture from './KeyCapture.svelte';
    import { KEY_OPTIONS, nameForKeycode } from './keycode-map.js';
    import MacroRecorder from './MacroRecorder.svelte';
    import { BUTTON_ACTION_KIND, MACRO_EVENT_KIND } from './types.js';
    import type { ButtonAction, MacroStep, RatbagButton } from './types.js';

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
    let macroSteps = $state<MacroStep[]>([...button.action.macro_steps]);
    let macroText = $state<string>(macroStepsToText(button.action.macro_steps));
    /** Sync DSL textarea ↔ recorded steps. `recorder` wins when the
     *  user has been recording; `text` wins when the user has typed
     *  into the DSL textarea. Tracked because the two are alternate
     *  inputs into the same `macro_steps` field. */
    let macroSource = $state<'recorder' | 'text'>(
        button.action.macro_steps.length > 0 ? 'recorder' : 'text',
    );
    let keySearch = $state('');
    let saving = $state(false);
    let error = $state<string | null>(null);

    function macroStepPrefix(kind: number): string {
        if (kind === MACRO_EVENT_KIND.KEY_PRESS) return 'p';
        if (kind === MACRO_EVENT_KIND.KEY_RELEASE) return 'r';
        if (kind === MACRO_EVENT_KIND.WAIT) return 'w';
        return '?';
    }

    function macroStepsToText(steps: readonly MacroStep[]): string {
        return steps
            .map((s) => `${macroStepPrefix(s.kind)}:${String(s.value)}`)
            .join(', ');
    }

    function tagToKind(tag: string): number {
        if (tag === 'p') return MACRO_EVENT_KIND.KEY_PRESS;
        if (tag === 'r') return MACRO_EVENT_KIND.KEY_RELEASE;
        return MACRO_EVENT_KIND.WAIT;
    }

    function macroTextToSteps(text: string): MacroStep[] {
        const entries = text
            .split(/[,;\n]/u)
            .map((s) => s.trim())
            .filter((s) => s.length > 0);
        const steps: MacroStep[] = [];
        for (const entry of entries) {
            const match = /^([prw])\s*:\s*(\d+)$/iu.exec(entry);
            if (match === null) {
                throw new Error(`bad macro step "${entry}" (use p:30, r:30, w:25)`);
            }
            const tag = match[1]?.toLowerCase() ?? '';
            const value = Number.parseInt(match[2] ?? '0', 10);
            steps.push({ kind: tagToKind(tag), value });
        }
        return steps;
    }

    const supportedKinds = $derived<readonly number[]>(button.supported_action_types);

    /** Filtered key list for the fallback name-search picker. */
    const keyOptionsFiltered = $derived(() => {
        const needle = keySearch.trim().toLowerCase();
        if (needle.length === 0) return KEY_OPTIONS;
        return KEY_OPTIONS.filter(
            (k) =>
                k.name.toLowerCase().includes(needle) ||
                k.code.toLowerCase().includes(needle),
        );
    });

    function buildAction(): ButtonAction {
        switch (workingKind) {
            case BUTTON_ACTION_KIND.MACRO: {
                const steps =
                    macroSource === 'text' ? macroTextToSteps(macroText) : macroSteps;
                return {
                    kind: BUTTON_ACTION_KIND.MACRO,
                    value: 0,
                    macro_steps: steps,
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
        // KeyCapture / MacroRecorder svelte:window handlers run on
        // the same keydown phase but stopPropagation + preventDefault
        // when they're armed — so this Escape-to-close only fires
        // when no recorder is capturing, which is what we want.
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
            <div class="binding-editor-row">
                <span class="binding-editor-label">Key</span>
                <KeyCapture
                    keycode={workingValue}
                    onchange={(k: number) => {
                        workingValue = k;
                    }}
                />
            </div>

            <details class="binding-editor-fallback">
                <summary>Or pick by name / enter a raw code</summary>
                <div class="binding-editor-fallback-body">
                    <input
                        class="input-field"
                        type="search"
                        bind:value={keySearch}
                        placeholder="search e.g. ‘alt’, ‘ctrl’, ‘arrow’"
                        aria-label="Search keys by name"
                    />
                    <select
                        class="input-field"
                        size="6"
                        value={String(workingValue)}
                        onchange={(e) => {
                            workingValue = Number((e.target as HTMLSelectElement).value);
                        }}
                        aria-label="Pick a key by name"
                    >
                        {#each keyOptionsFiltered() as opt (opt.keycode)}
                            <option value={String(opt.keycode)}>
                                {opt.name} — {opt.code}
                            </option>
                        {/each}
                    </select>
                    <label class="binding-editor-row">
                        <span class="binding-editor-label">Raw Linux keycode</span>
                        <input
                            class="input-field"
                            type="number"
                            min="1"
                            max="767"
                            bind:value={workingValue}
                        />
                        <small class="muted text-xs">
                            Currently selected: <span class="font-mono">{nameForKeycode(workingValue)}</span>
                        </small>
                    </label>
                </div>
            </details>
        {:else if workingKind === BUTTON_ACTION_KIND.MACRO}
            <div class="binding-editor-row">
                <span class="binding-editor-label">Macro</span>
                <MacroRecorder
                    steps={macroSteps}
                    onchange={(next: readonly MacroStep[]) => {
                        macroSteps = [...next];
                        macroText = macroStepsToText(next);
                        macroSource = 'recorder';
                    }}
                />
            </div>

            <details class="binding-editor-fallback">
                <summary>Or edit manually (DSL)</summary>
                <div class="binding-editor-fallback-body">
                    <p class="muted text-xs">
                        Comma-separated <code>p:CODE</code> (press) /
                        <code>r:CODE</code> (release) / <code>w:MS</code> (wait)
                        entries. Editing here overrides the recorder result.
                    </p>
                    <textarea
                        class="input-field binding-editor-macro"
                        rows="3"
                        bind:value={macroText}
                        oninput={() => {
                            macroSource = 'text';
                        }}
                        placeholder="p:30, w:25, r:30"
                    ></textarea>
                </div>
            </details>
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
