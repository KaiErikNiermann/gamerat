<script lang="ts">
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';
    import { describeAction, kindName, SPECIAL_OPTIONS } from './button-labels.js';
    import Select from './Select.svelte';
    import KeyCapture from './KeyCapture.svelte';
    import { KEY_OPTIONS, nameForKeycode } from './keycode-map.js';
    import MacroRecorder from './MacroRecorder.svelte';
    import { cancelPanicHatch, checkMacroBalance, panicHatch } from './ipc.js';
    import { BUTTON_ACTION_KIND, MACRO_EVENT_KIND, SOFT_MACRO_KIND } from './types.js';
    import type {
        ButtonAction,
        MacroStep,
        PanicHatchSettledPayload,
        RatbagButton,
        SoftMacro,
    } from './types.js';

    interface Props {
        button: RatbagButton;
        devicePath: string;
        /** Master opt-in for the soft-macro pipeline. Gates the
         *  "Convert to toggle" affordance in the unbalanced-macro
         *  warning — when `false`, the option is greyed out with a
         *  tooltip pointing the user at Settings. */
        softwareMacrosEnabled: boolean;
        /** `true` when the editor is opened against a managed
         *  `GameratProfile` (not Base mode). Soft-macros can only be
         *  attached to a logical profile because they live in
         *  `GameratProfile.soft_macros`; in Base mode the affordance
         *  is disabled with an explanatory tooltip. */
        canEditSoftMacros: boolean;
        onsave: (action: ButtonAction) => Promise<void> | void;
        /** Called when the user picks "Convert to toggle" from the
         *  unbalanced-macro warning. The host (MouseView) folds the
         *  soft-macro into the active profile's draft and clears the
         *  conflicting MACRO action; the daemon picks up the change
         *  through the normal Save + Apply path. */
        onsavesoftmacro?: (m: SoftMacro) => Promise<void> | void;
        onclose: () => void;
    }

    const {
        button,
        devicePath,
        softwareMacrosEnabled,
        canEditSoftMacros,
        onsave,
        onsavesoftmacro,
        onclose,
    }: Props = $props();

    /** localStorage key for the "don't warn again about unbalanced
     *  macros" opt-out. Mirrors the `gamerat:theme` persistence pattern
     *  in `theme.ts` — same scope (per-webview), same try/catch
     *  fallback behaviour. */
    const UNBALANCED_WARN_KEY = 'gamerat:warn-unbalanced-macro';
    /** Window the daemon arms for the user to press the affected button
     *  after panic-hatch fires. Kept in sync with `PANIC_HATCH_TIMEOUT`
     *  in `crates/gamerat-daemon/src/service.rs`. */
    const PANIC_COUNTDOWN_MS = 5000;

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

    /** Warning state: macro that the daemon reports as leaving keys
     *  pressed. While non-null the save action shows a confirmation
     *  panel instead of writing immediately. */
    interface PendingWarning {
        action: ButtonAction;
        stuck: readonly number[];
    }
    let pendingWarning = $state<PendingWarning | null>(null);
    /** Don't-warn-again preference, lazily read from localStorage. */
    let suppressWarning = $state<boolean>(loadSuppressWarning());

    /** Panic-hatch flow state. Driven by the dedicated "Panic" button
     *  visible only when the current binding is a macro. */
    type PanicState =
        | { phase: 'idle' }
        | { phase: 'running' }
        | {
              phase: 'awaiting';
              released: readonly number[];
              deadline: number;
          }
        | { phase: 'settled'; outcome: PanicHatchSettledPayload['outcome'] }
        | { phase: 'error'; message: string };
    let panic = $state<PanicState>({ phase: 'idle' });
    /** Tick that drives the live countdown text in the awaiting phase.
     *  Also serves as the trigger for `$derived` to re-evaluate. */
    let panicNow = $state<number>(Date.now());
    let countdownTimer: ReturnType<typeof setInterval> | null = null;
    let settledUnlisten: UnlistenFn | null = null;

    function loadSuppressWarning(): boolean {
        try {
            return globalThis.localStorage.getItem(UNBALANCED_WARN_KEY) === 'never';
        } catch {
            return false;
        }
    }

    function persistSuppressWarning(value: boolean): void {
        try {
            if (value) {
                globalThis.localStorage.setItem(UNBALANCED_WARN_KEY, 'never');
            } else {
                globalThis.localStorage.removeItem(UNBALANCED_WARN_KEY);
            }
        } catch {
            // localStorage unavailable (private mode, broken webview):
            // fall back to in-memory only — the user re-confirms on
            // next open. Acceptable per the existing theme.ts policy.
        }
    }

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
        if (saving) return;
        saving = true;
        error = null;
        try {
            const action = buildAction();
            if (action.kind === BUTTON_ACTION_KIND.MACRO && !suppressWarning) {
                const stuck = await checkMacroBalance(action.macro_steps);
                if (stuck.length > 0) {
                    pendingWarning = { action, stuck };
                    return;
                }
            }
            await commitAction(action);
        } catch (error_) {
            error = String(error_);
        } finally {
            saving = false;
        }
    }

    async function commitAction(action: ButtonAction): Promise<void> {
        await onsave(action);
        onclose();
    }

    /** "Auto-add release" path: append a `KEY_RELEASE` step for each
     *  keycode the daemon flagged, then commit. The order matches
     *  insertion order from the analyzer, so multi-key macros release
     *  in the same order they were pressed. */
    async function commitWithReleases(): Promise<void> {
        if (pendingWarning === null) return;
        if (suppressWarning) persistSuppressWarning(true);
        saving = true;
        error = null;
        try {
            const balanced: ButtonAction = {
                ...pendingWarning.action,
                macro_steps: [
                    ...pendingWarning.action.macro_steps,
                    ...pendingWarning.stuck.map((value) => ({
                        kind: MACRO_EVENT_KIND.KEY_RELEASE,
                        value,
                    })),
                ],
            };
            pendingWarning = null;
            await commitAction(balanced);
        } catch (error_) {
            error = String(error_);
        } finally {
            saving = false;
        }
    }

    async function commitAsStuck(): Promise<void> {
        if (pendingWarning === null) return;
        if (suppressWarning) persistSuppressWarning(true);
        saving = true;
        error = null;
        try {
            const action = pendingWarning.action;
            pendingWarning = null;
            await commitAction(action);
        } catch (error_) {
            error = String(error_);
        } finally {
            saving = false;
        }
    }

    function cancelWarning(): void {
        pendingWarning = null;
    }

    /** "Convert to toggle" path: drop the unbalanced macro and bind
     *  the stuck keycodes as a sticky-toggle soft-macro instead. The
     *  host (`MouseView.svelte`) commits the change through the
     *  profile draft + Save + Apply pipeline, so no firmware write
     *  happens here — `onsavesoftmacro` is enough. */
    async function convertToToggle(): Promise<void> {
        if (pendingWarning === null || onsavesoftmacro === undefined) return;
        if (suppressWarning) persistSuppressWarning(true);
        saving = true;
        error = null;
        try {
            const softMacro: SoftMacro = {
                button_index: button.index,
                kind: SOFT_MACRO_KIND.STICKY_TOGGLE,
                // Trampoline keycode is daemon-allocated on first apply.
                trampoline_keycode: 0,
                keys: pendingWarning.stuck,
            };
            pendingWarning = null;
            await onsavesoftmacro(softMacro);
            onclose();
        } catch (error_) {
            error = String(error_);
        } finally {
            saving = false;
        }
    }

    /** Reason the "Convert to toggle" button is disabled, or `null`
     *  when it's actually available. Surfaced as a tooltip so users
     *  know what to flip. */
    function disabledReasonFor(enabled: boolean, canEdit: boolean): string | null {
        if (enabled && canEdit) return null;
        if (!enabled) {
            return 'Enable "Software input augmentation" in Settings to use this.';
        }
        return 'Soft-macros require editing a managed profile (open one from the Profiles panel).';
    }

    const convertToToggleDisabledReason = $derived<string | null>(
        disabledReasonFor(softwareMacrosEnabled, canEditSoftMacros),
    );

    /** Format a list of keycodes for the warning text + panic modal.
     *  `nameForKeycode` already falls back to `Key N` for unknown
     *  keycodes, so we never have to format a bare number ourselves. */
    function describeKeys(keys: readonly number[]): string {
        return keys.map((k) => nameForKeycode(k)).join(', ');
    }

    // ───────────────────────────────────────────────────────────────
    // Panic hatch wiring
    // ───────────────────────────────────────────────────────────────

    /** Visible only when the *saved* binding is a macro — we don't
     *  want to panic-hatch unsaved working-copy changes. */
    const showPanicButton = $derived<boolean>(
        button.action.kind === BUTTON_ACTION_KIND.MACRO,
    );

    const countdownSeconds = $derived<number>(
        panic.phase === 'awaiting'
            ? Math.ceil(Math.max(0, panic.deadline - panicNow) / 1000)
            : 0,
    );

    async function ensureSettleListener(): Promise<void> {
        if (settledUnlisten !== null) return;
        settledUnlisten = await listen<PanicHatchSettledPayload>(
            'panic-hatch-settled',
            (event) => {
                if (
                    event.payload.device !== devicePath ||
                    event.payload.button !== button.index
                ) {
                    return;
                }
                stopCountdown();
                panic = { phase: 'settled', outcome: event.payload.outcome };
            },
        );
    }

    function stopCountdown(): void {
        if (countdownTimer !== null) {
            clearInterval(countdownTimer);
            countdownTimer = null;
        }
    }

    async function triggerPanic(): Promise<void> {
        await ensureSettleListener();
        panic = { phase: 'running' };
        try {
            const result = await panicHatch(devicePath, button.index);
            if (!result.awaiting_press) {
                // Daemon went straight to NONE — surface as settled
                // immediately; the signal won't come because nothing
                // was armed.
                panic = { phase: 'settled', outcome: 'timeout_disabled' };
                return;
            }
            panic = {
                phase: 'awaiting',
                released: result.released_keys,
                deadline: Date.now() + PANIC_COUNTDOWN_MS,
            };
            stopCountdown();
            countdownTimer = setInterval(() => {
                panicNow = Date.now();
            }, 200);
        } catch (error_) {
            panic = { phase: 'error', message: String(error_) };
        }
    }

    async function abortPanic(): Promise<void> {
        if (panic.phase !== 'awaiting') return;
        try {
            await cancelPanicHatch(devicePath, button.index);
            // The settle event will flip us to 'settled' / 'cancelled'.
        } catch (error_) {
            panic = { phase: 'error', message: String(error_) };
        }
    }

    function dismissPanic(): void {
        panic = { phase: 'idle' };
    }

    function teardownPanicListeners(): void {
        stopCountdown();
        if (settledUnlisten !== null) {
            const off = settledUnlisten;
            settledUnlisten = null;
            off();
        }
    }

    // Tear down listeners + timers when the editor unmounts (caller
    // closed the modal). Without this the listener leaks and a
    // subsequent open of the editor for a different button would still
    // receive events for the old one.
    $effect(() => teardownPanicListeners);
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
            <Select
                bind:value={workingKind}
                options={supportedKinds.map((kind) => ({
                    value: kind,
                    label: kindName(kind),
                }))}
                ariaLabel="Binding kind"
            />
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
                <Select
                    bind:value={workingValue}
                    options={SPECIAL_OPTIONS.map((opt) => ({
                        value: opt.value,
                        label: opt.label,
                    }))}
                    ariaLabel="Special action"
                />
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

        {#if pendingWarning !== null}
            <div class="binding-editor-warn" role="alert">
                <p class="binding-editor-warn-title">
                    Macro leaves <strong>{describeKeys(pendingWarning.stuck)}</strong>
                    pressed when the button is released.
                </p>
                <p class="binding-editor-warn-body muted text-xs">
                    The OS will see those keys as held until you bind a release,
                    run <code>gameratctl panic</code>, or unplug the mouse. Keep it
                    if that's intentional (e.g. sticky-keys, hold-to-run), or
                    auto-add a release for a clean momentary press.
                </p>
                <label class="binding-editor-warn-suppress">
                    <input type="checkbox" bind:checked={suppressWarning} />
                    <span>Don't warn me again about unbalanced macros</span>
                </label>
                <div class="binding-editor-warn-actions">
                    <button class="btn-ghost" type="button" onclick={cancelWarning}>
                        Back to editor
                    </button>
                    <button class="btn-ghost" type="button" onclick={commitAsStuck}>
                        Keep as stuck-key
                    </button>
                    <button class="btn-ghost" type="button" onclick={commitWithReleases}>
                        Auto-add release
                    </button>
                    <button
                        class="btn-primary"
                        type="button"
                        onclick={convertToToggle}
                        disabled={convertToToggleDisabledReason !== null}
                        title={convertToToggleDisabledReason ?? ''}
                    >
                        Convert to toggle
                    </button>
                </div>
            </div>
        {/if}

        {#if showPanicButton}
            <div class="binding-editor-panic">
                {#if panic.phase === 'idle'}
                    <button
                        class="btn-ghost-sm binding-editor-panic-btn"
                        type="button"
                        onclick={triggerPanic}
                        title="Force-release any stuck keys this macro left pressed, then auto-disable the binding."
                    >
                        Stuck key? Panic-hatch this button
                    </button>
                {:else if panic.phase === 'running'}
                    <p class="muted text-xs">Asking the daemon…</p>
                {:else if panic.phase === 'awaiting'}
                    <p class="binding-editor-panic-title">
                        Press button {button.index} now to release
                        <strong>{describeKeys(panic.released)}</strong>.
                    </p>
                    <p class="muted text-xs">
                        Auto-disabling in {countdownSeconds}s. Cancel to keep
                        the release-only macro for re-use.
                    </p>
                    <button class="btn-ghost-sm" type="button" onclick={abortPanic}>
                        Cancel auto-disable
                    </button>
                {:else if panic.phase === 'settled'}
                    <p class="binding-editor-panic-title">
                        {#if panic.outcome === 'cancelled'}
                            Auto-disable cancelled — release-only macro left in place.
                        {:else if panic.outcome === 'superseded'}
                            Binding was changed in the meantime — left alone.
                        {:else}
                            Binding disabled. Re-open to bind something new.
                        {/if}
                    </p>
                    <button class="btn-ghost-sm" type="button" onclick={dismissPanic}>
                        Dismiss
                    </button>
                {:else if panic.phase === 'error'}
                    <p class="error-text">{panic.message}</p>
                    <button class="btn-ghost-sm" type="button" onclick={dismissPanic}>
                        Dismiss
                    </button>
                {/if}
            </div>
        {/if}

        <footer class="binding-editor-actions">
            <button class="btn-ghost" type="button" onclick={onclose}>Cancel</button>
            <button
                class="btn-primary"
                type="submit"
                disabled={saving || pendingWarning !== null}
            >
                {saving ? 'Saving…' : 'Save binding'}
            </button>
        </footer>
    </form>
</div>
