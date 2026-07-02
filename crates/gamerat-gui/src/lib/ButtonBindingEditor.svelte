<script lang="ts">
    import { listen, type UnlistenFn } from '@tauri-apps/api/event';
    import {
        describeAction,
        describeKeys,
        formatSoftMacro,
        kindName,
        SPECIAL_OPTIONS,
    } from './button-labels.js';
    import {
        chordToSteps,
        formatChord,
        MODIFIER_KEYCODES,
        regularKeyPressCount,
        stepsToChord,
    } from './chord.js';
    import Select from './Select.svelte';
    import KeyCapture from './KeyCapture.svelte';
    import { KEY_OPTIONS, nameForKeycode } from './keycode-map.js';
    import MacroRecorder from './MacroRecorder.svelte';
    import Modal from './Modal.svelte';
    import { m } from './paraglide/messages.js';
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
        /** Active soft-macro bound to this button, if any. A sticky
         *  toggle leaves the firmware `button.action` deliberately
         *  `NONE`, so without this the editor would render the button
         *  as "Disabled" and hide the toggle the user set. Passed only
         *  in profile mode (Base mode has no soft-macro layer). */
        softMacro?: SoftMacro | null;
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
        softMacro = null,
        devicePath,
        softwareMacrosEnabled,
        canEditSoftMacros,
        onsave,
        onsavesoftmacro,
        onclose,
    }: Props = $props();

    /** UI-only pseudo-kind for soft-macro toggles. Distinct from every
     *  firmware `BUTTON_ACTION_KIND` (all ≥ 0) so it can share the kind
     *  dropdown's numeric value space without ever reaching the wire —
     *  a toggle saves through `onsavesoftmacro`, never as a
     *  `ButtonAction`. Future soft-macro subtypes get their own panel
     *  the same way. */
    const TOGGLE_KIND = -1;

    /** The button's firmware action carries a live sticky-toggle. */
    const hasActiveToggle =
        softMacro !== null && softMacro.kind === SOFT_MACRO_KIND.STICKY_TOGGLE;

    /** localStorage key for the "don't warn again about unbalanced
     *  macros" opt-out. Mirrors the `gamerat:theme` persistence pattern
     *  in `theme.ts` — same scope (per-webview), same try/catch
     *  fallback behaviour. */
    const UNBALANCED_WARN_KEY = 'gamerat:warn-unbalanced-macro';
    /** Window the daemon arms for the user to press the affected button
     *  after panic-hatch fires. Kept in sync with `PANIC_HATCH_TIMEOUT`
     *  in `crates/gamerat-daemon/src/service.rs`. */
    const PANIC_COUNTDOWN_MS = 5000;

    /** True when the device accepts the firmware KEY action — the
     *  landing kind for a shortcut with no modifiers. */
    const keySupported = button.supported_action_types.includes(BUTTON_ACTION_KIND.KEY);

    /** A modifier+key macro is really a keyboard shortcut; open it in
     *  the KEY editor with the modifier chips pre-filled instead of the
     *  granular step list. Only when KEY is supported (its no-modifier
     *  save path) and there's no toggle overriding this button. */
    const initialChord =
        !hasActiveToggle && keySupported && button.action.kind === BUTTON_ACTION_KIND.MACRO
            ? stepsToChord(button.action.macro_steps)
            : null;

    // Local working copy of the action. Initialised from the
    // current binding so the user can tweak rather than re-type
    // from scratch. An active toggle opens on the synthetic
    // TOGGLE_KIND; a modifier chord opens on KEY with modifiers set.
    function initialKind(): number {
        if (hasActiveToggle) return TOGGLE_KIND;
        if (initialChord !== null) return BUTTON_ACTION_KIND.KEY;
        return button.action.kind;
    }
    let workingKind = $state<number>(initialKind());
    let workingValue = $state<number>(
        initialChord === null ? button.action.value : initialChord.key,
    );
    /** Modifiers held with the KEY-editor key. A non-empty set turns the
     *  binding into a shortcut, saved as a canonical chord macro. */
    let keyModifiers = $state<number[]>(
        initialChord === null ? [] : [...initialChord.modifiers],
    );
    /** True while the macro recorder or a key-capture is armed — used to
     *  block Save so a half-captured macro can't be committed. */
    let capturing = $state<boolean>(false);
    /** Keys the sticky toggle presses/releases together. Seeded from
     *  the existing soft-macro so re-opening shows what's bound. */
    let toggleKeys = $state<number[]>(
        softMacro === null ? [] : [...softMacro.keys],
    );
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
            return localStorage.getItem(UNBALANCED_WARN_KEY) === 'never';
        } catch {
            return false;
        }
    }

    function persistSuppressWarning(value: boolean): void {
        try {
            if (value) {
                localStorage.setItem(UNBALANCED_WARN_KEY, 'never');
            } else {
                localStorage.removeItem(UNBALANCED_WARN_KEY);
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
                throw new Error(m.bind_bad_macro_step({ entry }));
            }
            const tag = match[1]?.toLowerCase() ?? '';
            const value = Number(match[2] ?? '0');
            steps.push({ kind: tagToKind(tag), value });
        }
        return steps;
    }

    const supportedKinds = $derived<readonly number[]>(button.supported_action_types);

    /** Soft toggles are OS-level, so the option is offered whenever a
     *  toggle is already bound (to display it) or the environment can
     *  create one — independent of firmware macro support. */
    const toggleAvailable = $derived<boolean>(
        hasActiveToggle || (softwareMacrosEnabled && canEditSoftMacros),
    );

    /** Kind dropdown: the firmware-supported kinds plus the synthetic
     *  Toggle entry when available. */
    const kindOptions = $derived<readonly { value: number; label: string }[]>([
        ...supportedKinds.map((kind) => ({ value: kind, label: kindName(kind) })),
        ...(toggleAvailable
            ? [{ value: TOGGLE_KIND, label: m.bind_kind_toggle() }]
            : []),
    ]);

    /** Header line: prefer the soft toggle over the (NONE) firmware
     *  action, matching the leader-label in MouseView. */
    const currentDescription = $derived<string>(
        softMacro === null ? describeAction(button.action) : formatSoftMacro(softMacro),
    );

    /** Each firmware kind reads `workingValue` from a different number
     *  space (mouse index 1–15, special `(1<<30)+N`, Linux keycode
     *  1–767). Sharing one variable means a leftover value from the
     *  previous kind leaks into the next editor — e.g. a SPECIAL's
     *  `1073741835` rendering as "Key 1073741835". Coerce to a sane
     *  default on every switch, restoring the saved value when the user
     *  lands back on the binding's original kind. */
    function defaultValueForKind(kind: number): number {
        if (kind === button.action.kind) return button.action.value;
        switch (kind) {
            case BUTTON_ACTION_KIND.MOUSE: {
                return 1; // libratbag buttons are 1-indexed (1 = left)
            }
            case BUTTON_ACTION_KIND.KEY: {
                return 30; // KEY_A — a real, obviously-valid keycode
            }
            case BUTTON_ACTION_KIND.SPECIAL: {
                return SPECIAL_OPTIONS[0]?.value ?? 0;
            }
            default: {
                return 0;
            }
        }
    }

    function handleKindChange(kind: number): void {
        // Switching editors abandons any in-progress capture — clear the
        // guard so Save isn't stuck disabled after the capturer unmounts.
        capturing = false;
        if (kind === TOGGLE_KIND) return; // toggle reads toggleKeys, not workingValue
        workingValue = defaultValueForKind(kind);
    }

    /** Modifiers turn a key into a shortcut, which is stored as a macro;
     *  so the modifier chips are only offered when the device accepts
     *  the MACRO action. */
    const macroSupported = $derived<boolean>(
        supportedKinds.includes(BUTTON_ACTION_KIND.MACRO),
    );

    function toggleModifier(keycode: number): void {
        keyModifiers = keyModifiers.includes(keycode)
            ? keyModifiers.filter((k) => k !== keycode)
            : [...keyModifiers, keycode];
    }

    /** Live "Sends: L Alt + A" preview for the shortcut builder. */
    const shortcutPreview = $derived<string>(
        formatChord({ key: workingValue, modifiers: keyModifiers }),
    );

    /** Non-modifier key presses in the granular macro — a count > 1
     *  can't survive the hidpp20 collapse, so we warn. Guards against
     *  the DSL textarea being mid-edit / invalid. */
    const macroRegularKeyCount = $derived.by<number>(() => {
        if (macroSource !== 'text') return regularKeyPressCount(macroSteps);
        try {
            return regularKeyPressCount(macroTextToSteps(macroText));
        } catch {
            return 0;
        }
    });

    function addToggleKey(keycode: number): void {
        if (toggleKeys.includes(keycode)) return;
        toggleKeys = [...toggleKeys, keycode];
    }

    function removeToggleKey(index: number): void {
        toggleKeys = toggleKeys.filter((_, i) => i !== index);
    }

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
            case BUTTON_ACTION_KIND.KEY: {
                // A plain key stays a KEY action; adding modifiers makes
                // it a shortcut, stored as a canonical chord macro that
                // survives libratbag's macro→key collapse.
                if (keyModifiers.length === 0) {
                    return { kind: BUTTON_ACTION_KIND.KEY, value: workingValue, macro_steps: [] };
                }
                return {
                    kind: BUTTON_ACTION_KIND.MACRO,
                    value: 0,
                    macro_steps: chordToSteps({ key: workingValue, modifiers: keyModifiers }),
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
            if (workingKind === TOGGLE_KIND) {
                await saveToggle();
                return;
            }
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

    /** Persist the working toggle through the soft-macro pipeline. The
     *  Save button is disabled unless there's a handler and at least
     *  one key, so those guards are just defensive. The trampoline
     *  keycode is preserved from the existing macro (or left 0 for the
     *  daemon to allocate on first apply). */
    async function saveToggle(): Promise<void> {
        if (onsavesoftmacro === undefined || toggleKeys.length === 0) return;
        await onsavesoftmacro({
            button_index: button.index,
            kind: SOFT_MACRO_KIND.STICKY_TOGGLE,
            trampoline_keycode: softMacro?.trampoline_keycode ?? 0,
            keys: [...toggleKeys],
        });
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
            return m.bind_toggle_disabled_flag();
        }
        return m.bind_toggle_disabled_profile();
    }

    const convertToToggleDisabledReason = $derived<string | null>(
        disabledReasonFor(softwareMacrosEnabled, canEditSoftMacros),
    );

    /** Format a list of keycodes for the warning text + panic modal.
     *  `nameForKeycode` already falls back to `Key N` for unknown
     *  keycodes, so we never have to format a bare number ourselves. */
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
        if (countdownTimer === null) return;
        clearInterval(countdownTimer);
        countdownTimer = null;
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

<!-- The Modal wrapper hosts the click-outside-cancel + Escape-to-close
     handlers. KeyCapture / MacroRecorder's svelte:window handlers run
     on the same keydown phase but stopPropagation + preventDefault
     when they're armed — so the Modal's Escape handler only fires
     when no recorder is capturing, which is what we want. -->
<Modal
    label={m.bind_modal_label({ index: button.index })}
    {onclose}
>
    <form class="binding-editor-card" onsubmit={handleSave}>
        <header class="binding-editor-head">
            <h3 class="binding-editor-title">
                {m.bind_title({ index: button.index })}
            </h3>
            <button
                type="button"
                class="btn-ghost-sm"
                onclick={onclose}
                aria-label={m.bind_close_aria()}
            >
                {m.common_close_text()}
            </button>
        </header>

        <p class="muted text-xs binding-editor-current">
            {m.bind_current({ action: currentDescription })}
        </p>

        <label class="binding-editor-row">
            <span class="binding-editor-label">{m.bind_kind_label()}</span>
            <Select
                bind:value={workingKind}
                onchange={handleKindChange}
                options={kindOptions}
                ariaLabel={m.bind_kind_aria()}
            />
        </label>

        {#if workingKind === BUTTON_ACTION_KIND.MOUSE}
            <label class="binding-editor-row">
                <span class="binding-editor-label">{m.bind_mouse_index_label()}</span>
                <input
                    class="input-field"
                    type="number"
                    min="1"
                    max="15"
                    bind:value={workingValue}
                />
            </label>
        {:else if workingKind === BUTTON_ACTION_KIND.SPECIAL}
            <label class="binding-editor-row">
                <span class="binding-editor-label">{m.bind_special_label()}</span>
                <Select
                    bind:value={workingValue}
                    options={SPECIAL_OPTIONS.map((opt) => ({
                        value: opt.value,
                        label: opt.label,
                    }))}
                    ariaLabel={m.bind_special_aria()}
                />
            </label>
        {:else if workingKind === BUTTON_ACTION_KIND.KEY}
            <div class="binding-editor-row">
                <span class="binding-editor-label">{m.bind_key_label()}</span>
                <KeyCapture
                    keycode={workingValue}
                    onchange={(k: number) => {
                        workingValue = k;
                    }}
                    onarmedchange={(armed: boolean) => {
                        capturing = armed;
                    }}
                />
            </div>

            {#if macroSupported}
                <div class="binding-editor-row">
                    <span class="binding-editor-label">{m.bind_modifiers_label()}</span>
                    <div class="binding-editor-modifiers" role="group" aria-label={m.bind_modifiers_label()}>
                        {#each MODIFIER_KEYCODES as mod (mod)}
                            <button
                                type="button"
                                class="binding-editor-mod"
                                class:binding-editor-mod-active={keyModifiers.includes(mod)}
                                aria-pressed={keyModifiers.includes(mod)}
                                onclick={() => {
                                    toggleModifier(mod);
                                }}
                            >
                                {nameForKeycode(mod)}
                            </button>
                        {/each}
                    </div>
                </div>
                <p class="muted text-xs binding-editor-shortcut-preview">
                    {m.bind_shortcut_preview({ keys: shortcutPreview })}
                </p>
            {/if}

            <details class="binding-editor-fallback">
                <summary>{m.bind_key_fallback_summary()}</summary>
                <div class="binding-editor-fallback-body">
                    <input
                        class="input-field"
                        type="search"
                        bind:value={keySearch}
                        placeholder={m.bind_key_search_placeholder()}
                        aria-label={m.bind_key_search_aria()}
                    />
                    <select
                        class="input-field"
                        size="6"
                        value={String(workingValue)}
                        onchange={(e) => {
                            workingValue = Number((e.target as HTMLSelectElement).value);
                        }}
                        aria-label={m.bind_key_pick_aria()}
                    >
                        {#each keyOptionsFiltered() as opt (opt.keycode)}
                            <option value={String(opt.keycode)}>
                                {opt.name} — {opt.code}
                            </option>
                        {/each}
                    </select>
                    <label class="binding-editor-row">
                        <span class="binding-editor-label">{m.bind_raw_keycode_label()}</span>
                        <input
                            class="input-field"
                            type="number"
                            min="1"
                            max="767"
                            bind:value={workingValue}
                        />
                        <small class="muted text-xs">
                            {m.bind_currently_selected()} <span class="font-mono">{nameForKeycode(workingValue)}</span>
                        </small>
                    </label>
                </div>
            </details>
        {:else if workingKind === BUTTON_ACTION_KIND.MACRO}
            <div class="binding-editor-row">
                <span class="binding-editor-label">{m.bind_macro_label()}</span>
                <MacroRecorder
                    steps={macroSteps}
                    onchange={(next: readonly MacroStep[]) => {
                        macroSteps = [...next];
                        macroText = macroStepsToText(next);
                        macroSource = 'recorder';
                    }}
                    onrecordingchange={(recording: boolean) => {
                        capturing = recording;
                    }}
                />
            </div>

            <p class="muted text-xs binding-editor-macro-note">{m.bind_macro_sequence_note()}</p>
            {#if macroRegularKeyCount > 1}
                <p class="binding-editor-macro-warn text-xs">{m.bind_macro_multikey_warn()}</p>
            {/if}

            <details class="binding-editor-fallback">
                <summary>{m.bind_macro_dsl_summary()}</summary>
                <div class="binding-editor-fallback-body">
                    <p class="muted text-xs">{m.bind_macro_dsl_help()}</p>
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
        {:else if workingKind === TOGGLE_KIND}
            <div class="binding-editor-row">
                <span class="binding-editor-label">{m.bind_toggle_kind_badge()}</span>
                <div class="binding-editor-toggle">
                    <p class="muted text-xs">{m.bind_toggle_help()}</p>
                    {#if toggleKeys.length > 0}
                        <ul class="binding-editor-toggle-keys">
                            {#each toggleKeys as key, i (String(key) + ':' + String(i))}
                                <li class="binding-editor-toggle-chip">
                                    <span class="font-mono">{nameForKeycode(key)}</span>
                                    <button
                                        type="button"
                                        class="binding-editor-toggle-remove"
                                        aria-label={m.bind_toggle_remove_key({
                                            key: nameForKeycode(key),
                                        })}
                                        onclick={() => {
                                            removeToggleKey(i);
                                        }}
                                    >
                                        ×
                                    </button>
                                </li>
                            {/each}
                        </ul>
                    {:else}
                        <p class="error-text text-xs">{m.bind_toggle_no_keys()}</p>
                    {/if}
                    <KeyCapture
                        keycode={0}
                        showCurrent={false}
                        onchange={addToggleKey}
                        onarmedchange={(armed: boolean) => {
                            capturing = armed;
                        }}
                    />
                </div>
            </div>
        {/if}

        {#if error !== null}
            <p class="error-text">{error}</p>
        {/if}

        {#if pendingWarning !== null}
            <div class="binding-editor-warn" role="alert">
                <p class="binding-editor-warn-title">
                    {m.bind_warn_title({ keys: describeKeys(pendingWarning.stuck) })}
                </p>
                <p class="binding-editor-warn-body muted text-xs">{m.bind_warn_body()}</p>
                <label class="binding-editor-warn-suppress">
                    <input type="checkbox" bind:checked={suppressWarning} />
                    <span>{m.bind_warn_suppress()}</span>
                </label>
                <div class="binding-editor-warn-actions">
                    <button class="btn-ghost" type="button" onclick={cancelWarning}>
                        {m.bind_warn_back()}
                    </button>
                    <button class="btn-ghost" type="button" onclick={commitAsStuck}>
                        {m.bind_warn_keep()}
                    </button>
                    <button class="btn-ghost" type="button" onclick={commitWithReleases}>
                        {m.bind_warn_autorelease()}
                    </button>
                    <button
                        class="btn-primary"
                        type="button"
                        onclick={convertToToggle}
                        disabled={convertToToggleDisabledReason !== null}
                        title={convertToToggleDisabledReason ?? ''}
                    >
                        {m.bind_warn_convert()}
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
                        title={m.bind_panic_btn_title()}
                    >
                        {m.bind_panic_btn()}
                    </button>
                {:else if panic.phase === 'running'}
                    <p class="muted text-xs">{m.bind_panic_running()}</p>
                {:else if panic.phase === 'awaiting'}
                    <p class="binding-editor-panic-title">
                        {m.bind_panic_awaiting({
                            index: button.index,
                            keys: describeKeys(panic.released),
                        })}
                    </p>
                    <p class="muted text-xs">
                        {m.bind_panic_countdown({ seconds: countdownSeconds })}
                    </p>
                    <button class="btn-ghost-sm" type="button" onclick={abortPanic}>
                        {m.bind_panic_cancel()}
                    </button>
                {:else if panic.phase === 'settled'}
                    <p class="binding-editor-panic-title">
                        {#if panic.outcome === 'cancelled'}
                            {m.bind_panic_cancelled()}
                        {:else if panic.outcome === 'superseded'}
                            {m.bind_panic_superseded()}
                        {:else}
                            {m.bind_panic_disabled()}
                        {/if}
                    </p>
                    <button class="btn-ghost-sm" type="button" onclick={dismissPanic}>
                        {m.bind_panic_dismiss()}
                    </button>
                {:else if panic.phase === 'error'}
                    <p class="error-text">{panic.message}</p>
                    <button class="btn-ghost-sm" type="button" onclick={dismissPanic}>
                        {m.bind_panic_dismiss()}
                    </button>
                {/if}
            </div>
        {/if}

        <footer class="binding-editor-actions">
            {#if capturing}
                <span class="muted text-xs binding-editor-capturing-hint">
                    {m.bind_stop_capture_hint()}
                </span>
            {/if}
            <button class="btn-ghost" type="button" onclick={onclose}>{m.common_cancel()}</button>
            <button
                class="btn-primary"
                type="submit"
                disabled={saving ||
                    capturing ||
                    pendingWarning !== null ||
                    (workingKind === TOGGLE_KIND && toggleKeys.length === 0)}
                title={capturing ? m.bind_stop_capture_hint() : ''}
            >
                {m[saving ? 'common_saving' : 'bind_save']()}
            </button>
        </footer>
    </form>
</Modal>
