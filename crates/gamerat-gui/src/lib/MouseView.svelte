<script lang="ts">
    import Loader2 from '@lucide/svelte/icons/loader-2';
    import { listen } from '@tauri-apps/api/event';
    import { onMount, tick, untrack } from 'svelte';
    import ButtonBindingEditor from './ButtonBindingEditor.svelte';
    import { formatAction } from './button-labels.js';
    import { hasDeviceDefaults } from './device-defaults.js';
    import Icon from './Icon.svelte';
    import {
        PROFILE_INDEX_ACTIVE,
        applyProfile,
        applyToActiveProfile,
        fetchActiveProfileDpi,
        fetchButtons,
        fetchDpiStageDisableCaps,
        fetchLeds,
        upsertProfile,
        writeButton,
        writeLed,
    } from './ipc.js';
    import LedColorEditor from './LedColorEditor.svelte';
    import { m } from './paraglide/messages.js';
    import Select from './Select.svelte';
    import {
        labelTooltip,
        type LabelRef,
    } from './mouse-view-helpers.js';
    import {
        DEFAULT_ACTION,
        addDpiStage,
        bindingForButton,
        cloneProfile,
        debounce,
        ledForIndex,
        removeDpiStage,
        resetProfileToDefaults,
        setActiveDpiStage,
        setBinding,
        setDpiStage,
        setLed,
        setSoftMacro,
    } from './profile-edit.js';
    import { lookupMouseSvg } from './svg-lookup.js';
    import { prepareSvgRoot } from './svg-prep.js';
    import { formatDuration } from './timing.js';
    import type {
        ActiveDpiStageChangedPayload,
        ButtonAction,
        DeviceInfo,
        GameratProfile,
        ProfileLed,
        RatbagButton,
        RatbagLed,
        SoftMacro,
    } from './types.js';
    import { BUTTON_ACTION_KIND, LED_MODE } from './types.js';

    interface LabelPos {
        readonly id: string;
        /** Plain button index (`buttonN`) — null for non-button labels. */
        readonly buttonIndex: number | null;
        /** LED index (`ledN`) — null when this label isn't an LED.
         *  Exactly one of `buttonIndex` / `ledIndex` is non-null for
         *  any clickable label; static labels have both null. */
        readonly ledIndex: number | null;
        readonly text: string;
        readonly x: number;
        readonly y: number;
        readonly side: 'left' | 'right';
    }

    interface Props {
        device: DeviceInfo | null;
        /** When non-null, MouseView is in **profile mode** — labels
         *  show this profile's bindings, edits mutate a draft, save
         *  flows through `upsertProfile`. When null, falls back to
         *  **live-hardware mode**: labels reflect what ratbagd
         *  currently has and edits write through `writeButton`. */
        profile: GameratProfile | null;
        autoswitchEnabled: boolean | null;
        /** Every gamerat profile — drives the "Editing: …" dropdown. */
        profiles: GameratProfile[];
        /** True while a hardware profile swap is in progress (driven
         *  by the daemon's `ProfileSwitching` / `ProfileSwitched`
         *  signals upstream in `App.svelte`). Used to render a
         *  transient "switching…" badge over the mouse stage so the
         *  brief firmware jitter reads as expected. */
        switchingNow: boolean;
        /** Master opt-in for the soft-macro pipeline. Passed through
         *  to the binding editor so it can gate the "Convert to
         *  toggle" affordance with the right tooltip. */
        softwareMacrosEnabled: boolean;
        onprofileschange: () => void;
        onselectprofile: (id: string | null) => void;
    }

    const {
        device,
        profile,
        autoswitchEnabled,
        profiles,
        switchingNow,
        softwareMacrosEnabled,
        onprofileschange,
        onselectprofile,
    }: Props = $props();

    // ───────────────────────────────────────────────────────────────
    // SVG state — same as before.
    // ───────────────────────────────────────────────────────────────
    let svgContent = $state<string>('');
    let svgError = $state<string | null>(null);
    let svgFilename = $state<string>('');
    let labels = $state<LabelPos[]>([]);
    let stage: HTMLDivElement | undefined = $state();

    // ───────────────────────────────────────────────────────────────
    // Live-hardware metadata.
    //
    // Even in profile mode, we fetch the hardware's button list once
    // per device so the binding editor knows each button's
    // `supported_action_types` (the firmware capability gating the
    // kind dropdown). In live mode the full RatbagButton is also the
    // source of truth for the on-screen labels.
    // ───────────────────────────────────────────────────────────────
    let liveButtons = $state<RatbagButton[]>([]);
    let liveButtonsError = $state<string | null>(null);
    let lastLiveFetchKey = $state<string | null>(null);

    /** Snapshot of the active profile's LEDs. Same role as
     *  `liveButtons` — drives the LED color editor's `supported_modes`
     *  / `color_depth` gates and serves as the source of truth for
     *  per-LED state in Base mode (where there's no gamerat profile
     *  record to read from). Empty array when the device's driver
     *  doesn't expose LED objects — the GUI then leaves LED labels
     *  click-disabled, mirroring how non-RGB mice behave today. */
    let liveLeds = $state<RatbagLed[]>([]);
    let liveLedsError = $state<string | null>(null);

    // ───────────────────────────────────────────────────────────────
    // Profile-mode draft. Synced from the `profile` prop on change of
    // id; in-place edits to the same profile don't clobber the user's
    // unsaved work mid-typing.
    // ───────────────────────────────────────────────────────────────
    let draft = $state<GameratProfile | null>(null);
    let saveStatus = $state<'idle' | 'saving' | 'saved' | 'error'>('idle');
    let saveError = $state<string | null>(null);
    /** Wall-clock duration of the most recently completed save / apply,
     *  surfaced as a dimmed suffix next to "saved" so timing anomalies
     *  are visible without manual benchmarking. Null until the first
     *  successful action; reset implicitly (hidden) while saving. */
    let lastActionMs = $state<number | null>(null);

    /** Which button index is currently being edited (the popover is
     *  open for that button). Indexes are stable across profile and
     *  live mode. */
    let editingIndex = $state<number | null>(null);

    /** Which LED index is currently being edited. Mutually exclusive
     *  with `editingIndex` — `handleLabelClick` routes to one or the
     *  other based on which leader-id pattern matched. */
    let editingLedIndex = $state<number | null>(null);

    /** Hardware's "default active" DPI stage as ratbagd reports it.
     *  This is the stage `SetActive` was last called with — **not**
     *  the stage the mouse is actually cycled to. libratbag /
     *  ratbagd have no visibility into the firmware-internal DPI
     *  cycle that the physical DPI-up / DPI-down / DPI-cycle buttons
     *  drive, so we can only show what was last written. Same
     *  limitation Piper has. Initialized once per device load; we
     *  used to poll this but the polling was misleading. */
    let liveActiveDpiStage = $state<number | null>(null);

    /** Hardware's live DPI stages (full list, in order). Fetched
     *  on device change and after Base-mode edits. Used by Base-mode
     *  to render the DPI editor — there's no gamerat profile record
     *  to read from in that mode. */
    let liveDpiStages = $state<readonly number[]>([]);

    /** Per-slot answer to "can this hardware slot be IsDisabled'd?",
     *  matching the resolution slot order. Fetched once per device.
     *  When every entry is `true`, the daemon's `apply_profile_complete`
     *  will hardware-disable any slot beyond the profile's stage count
     *  so the firmware skips it in the DPI cycle. When `false` shows
     *  up anywhere, removed stages stay in the cycle — we annotate the
     *  DPI editor so the user doesn't think the cycle button is broken.
     *  Empty array until the fetch lands; treat unknown as "supported"
     *  (best-effort honesty without breaking the affordance on a
     *  transient daemon hiccup). */
    let dpiDisableCaps = $state<readonly boolean[]>([]);

    /** Derived: `true` exactly when every resolution slot on the device
     *  declares the disable cap. Drives the "− stage" affordance and
     *  the explanatory hint below the DPI editor. */
    const allSlotsCanDisable = $derived(
        dpiDisableCaps.length > 0 && dpiDisableCaps.every(Boolean),
    );

    /** Sentinel id used for the Base-mode draft. The draft otherwise
     *  carries a real gamerat profile id; this lets handlers detect
     *  Base mode via `profile === null` and route saves to
     *  applyToActiveProfile instead of upsertProfile. */
    const BASE_DRAFT_ID = '__base__';

    // Sync the draft when the parent picks a new profile.
    $effect(() => {
        if (profile === null) {
            // Base mode: build a phantom profile from live hardware
            // state so the DPI editor + Reset button render the same
            // way as profile mode. We can only do this once both the
            // live buttons and the live DPI stages have arrived.
            if (liveButtons.length === 0 || liveDpiStages.length === 0) {
                draft = null;
                return;
            }
            // Mirror live buttons + LEDs into the draft so labels
            // re-render after a base-mode binding or LED save (each
            // of those refetches the matching live array). DPI stages
            // and `active_dpi_stage` accumulate user edits between
            // debounced saves, so they're sourced from the existing
            // draft when one's around — clobbering them mid-typing
            // would lose the in-flight value before the debounce
            // fires. First build initialises them from live.
            //
            // `untrack` wraps the `draft` read so the effect doesn't
            // track itself as a dependency: we read draft only to
            // preserve in-flight DPI edits, not to react to draft
            // changes — and re-firing on every assignment here would
            // loop the depth guard.
            const previous = untrack(() =>
                draft?.id === BASE_DRAFT_ID ? draft : null,
            );
            draft = {
                id: BASE_DRAFT_ID,
                name: 'base',
                description: '',
                category: 'agnostic',
                inherits_from: '',
                dpi: previous === null ? [...liveDpiStages] : [...previous.dpi],
                active_dpi_stage: previous === null
                    ? liveActiveDpiStage ?? 0
                    : previous.active_dpi_stage,
                created_unix: 0,
                buttons: liveButtons.map((b) => ({ index: b.index, action: b.action })),
                leds: liveLeds.map((l) => ({
                    index: l.index,
                    mode: l.mode,
                    color: l.color,
                    brightness: l.brightness,
                })),
                // Base mode has no logical profile to attach
                // soft-macros to; leave the vec empty.
                soft_macros: [],
            };
            if (previous === null) {
                saveStatus = 'idle';
                saveError = null;
            }
            return;
        }
        if (draft?.id !== profile.id) {
            // Snapshot through $state.snapshot first — a raw
            // cloneProfile(profile) throws DataCloneError because
            // Svelte 5's reactive proxy has non-cloneable internals.
            // $state.snapshot returns a plain object; structuredClone
            // then deep-copies it so the draft is fully decoupled
            // from the prop.
            draft = cloneProfile(profile);
            saveStatus = 'idle';
            saveError = null;
        }
    });

    // Fetch SVG when device changes.
    $effect(() => {
        const model = device?.model ?? '';
        if (model.length === 0) {
            svgContent = '';
            svgError = null;
            return;
        }
        void loadSvgForModel(model);
    });

    // Re-fetch live button list whenever device changes. Used for
    // supported_action_types in both modes; serves as the actions
    // source in live mode. Also fetches the DPI stages alongside —
    // Base mode needs them to render the DPI editor.
    $effect(() => {
        const path = device?.object_path;
        if (path === undefined) {
            liveButtons = [];
            liveDpiStages = [];
            dpiDisableCaps = [];
            liveLeds = [];
            liveLedsError = null;
            lastLiveFetchKey = null;
            return;
        }
        const key = path;
        if (key === lastLiveFetchKey) return;
        lastLiveFetchKey = key;
        void (async () => {
            liveButtonsError = null;
            liveLedsError = null;
            try {
                liveButtons = await fetchButtons(path, PROFILE_INDEX_ACTIVE);
            } catch (error_) {
                liveButtonsError = String(error_);
                liveButtons = [];
            }
            try {
                const result = await fetchActiveProfileDpi(path);
                liveDpiStages = result.dpi;
                liveActiveDpiStage = result.activeStage;
            } catch {
                liveDpiStages = [];
            }
            try {
                dpiDisableCaps = await fetchDpiStageDisableCaps(path);
            } catch {
                // Daemon couldn't probe caps (older daemon, ratbagd
                // hiccup) — leave the array empty; UI falls back to
                // "no honesty hint, no affordance restriction."
                dpiDisableCaps = [];
            }
            try {
                liveLeds = await fetchLeds(path, PROFILE_INDEX_ACTIVE);
            } catch (error_) {
                liveLedsError = String(error_);
                liveLeds = [];
            }
        })();
    });

    // Subscribe to the daemon's `active-dpi-stage-changed` event
    // (forwarded from ratbagd's PropertiesChanged after a
    // RefreshActive). Requires the libratbag patch in
    // `patches/libratbag/` — without it the daemon's DPI tracker
    // logs once + goes silent, and `liveActiveDpiStage` just stays
    // at whatever the initial fetch returned. Either way no
    // polling on the GUI side.
    onMount(() => {
        const unlisten = listen<ActiveDpiStageChangedPayload>(
            'active-dpi-stage-changed',
            (event) => {
                if (event.payload.device !== device?.object_path) return;
                liveActiveDpiStage = event.payload.stage;
            },
        );
        return () => {
            void unlisten.then((fn) => { fn(); });
        };
    });

    // Re-measure leaders on SVG content / stage resize.
    $effect(() => {
        if (svgContent.length === 0 || stage === undefined) return;

        let observer: ResizeObserver | undefined;
        const target = stage;
        void (async () => {
            await tick();
            measureLeaders();
            observer = new ResizeObserver(() => {
                measureLeaders();
            });
            observer.observe(target);
        })();

        return () => {
            observer?.disconnect();
        };
    });

    async function loadSvgForModel(model: string): Promise<void> {
        svgError = null;
        try {
            const filename = await lookupMouseSvg(model);
            svgFilename = filename;
            const url = `/mice/${filename}`;
            const res = await fetch(url);
            if (!res.ok) {
                throw new Error(`fetch ${filename}: ${String(res.status)}`);
            }
            const contentType = res.headers.get('content-type') ?? '';
            const text = await res.text();
            if (
                contentType.includes('text/html') ||
                /^\s*<!doctype html/i.test(text) ||
                /^\s*<html/i.test(text)
            ) {
                throw new Error(
                    `${url} returned HTML, not an SVG — the dev server's SPA fallback is shadowing the file. ` +
                    `Check that crates/gamerat-gui/public/mice is still a symlink to ../../../data/mice and ` +
                    `restart the dev server.`,
                );
            }
            if (!text.includes('<svg')) {
                throw new Error(
                    `${url} response is not an SVG (got ${String(text.length)} bytes, no <svg tag)`,
                );
            }
            svgContent = sanitizeSvg(text);
        } catch (error) {
            svgError = String(error);
            svgContent = '';
        }
    }

    function measureLeaders(): void {
        if (stage === undefined) return;
        const svgRoot = stage.querySelector('svg');
        if (svgRoot === null) return;
        prepareSvgRoot(svgRoot);

        const stageRect = stage.getBoundingClientRect();
        const next: LabelPos[] = [];

        for (const leader of stage.querySelectorAll<SVGElement>('[id$="-leader"]')) {
            const id = leader.id.slice(0, -'-leader'.length);
            if (id.length === 0) continue;
            const rect = leader.getBoundingClientRect();
            if (rect.width === 0 && rect.height === 0) continue;
            const x = rect.left + rect.width / 2 - stageRect.left;
            const y = rect.top + rect.height / 2 - stageRect.top;
            const style = leader.getAttribute('style') ?? '';
            const side: 'left' | 'right' = style.includes('text-align:end') ? 'left' : 'right';
            const buttonMatch = /^button(\d+)$/u.exec(id);
            const buttonIndex = buttonMatch === null ? null : Number(buttonMatch[1]);
            const ledMatch = /^led(\d+)$/u.exec(id);
            const ledIndex = ledMatch === null ? null : Number(ledMatch[1]);
            next.push({
                id,
                buttonIndex,
                ledIndex,
                text: labelTextFor(id),
                x,
                y,
                side,
            });
        }
        labels = next;
    }

    function sanitizeSvg(raw: string): string {
        return raw
            .replace(/<\?xml[^?]*\?>/u, '')
            .replace(/<!DOCTYPE[^>]*>/u, '')
            .trim();
    }

    function labelTextFor(id: string): string {
        const buttonMatch = /^button(\d+)$/u.exec(id);
        if (buttonMatch !== null) return `B${buttonMatch[1] ?? ''}`;
        const ledMatch = /^led(\d+)$/u.exec(id);
        if (ledMatch !== null) return `LED ${ledMatch[1] ?? ''}`;
        return id;
    }

    // ───────────────────────────────────────────────────────────────
    // Label rendering. Profile mode renders `bindingForButton`'s
    // action from the draft (or the source profile if the draft hasn't
    // been forked yet); live mode renders the on-hardware binding.
    // Gating on `profile` rather than `draft` keeps the labels stable
    // across the brief draft-sync race after a profile is selected.
    // ───────────────────────────────────────────────────────────────
    function activeProfileView(): GameratProfile | null {
        return draft ?? profile;
    }

    /** Discriminated union the template uses to render labels. LED
     *  labels with a meaningful color produce `{ kind: 'led-swatch' }`
     *  so the template can paint an actual color chip next to the
     *  label text; everything else collapses to plain text. */
    type LabelContent =
        | { readonly kind: 'text'; readonly text: string }
        | { readonly kind: 'led-swatch'; readonly text: string; readonly hex: string };

    function liveLabelContent(label: LabelRef): LabelContent {
        if (label.buttonIndex !== null) {
            return { kind: 'text', text: buttonLabelText(label) };
        }
        const ledIdx = label.ledIndex ?? null;
        if (ledIdx !== null) {
            return ledLabelContent(label, ledIdx);
        }
        return { kind: 'text', text: label.text };
    }

    function buttonLabelText(label: LabelRef): string {
        const view = activeProfileView();
        if (view !== null && label.buttonIndex !== null) {
            const action = bindingForButton(view, label.buttonIndex);
            // Distinguish "user hasn't set this yet" from a deliberate
            // Disabled binding — both render as Disabled but with a
            // muted style only for the default. (Implemented via the
            // .leader-label-unset class below.)
            return formatAction(action);
        }
        const found = liveButtons.find((b) => b.index === label.buttonIndex);
        if (found === undefined) return label.text;
        return formatAction(found.action);
    }

    /** Prefer the profile-side override (if any), then the live
     *  hardware snapshot. OFF/CYCLE collapse to a text suffix; for
     *  ON/BREATHING we emit a swatch so the template paints the actual
     *  color next to the label — quicker to parse than a hex string. */
    function ledLabelContent(label: LabelRef, ledIdx: number): LabelContent {
        const view = activeProfileView();
        const profileLed = view === null ? null : ledForIndex(view, ledIdx);
        const liveLed = liveLeds.find((l) => l.index === ledIdx);
        const mode = profileLed?.mode ?? liveLed?.mode;
        if (mode === undefined) return { kind: 'text', text: label.text };
        if (mode === LED_MODE.OFF) {
            return { kind: 'text', text: m.mv_led_off({ label: label.text }) };
        }
        if (mode === LED_MODE.CYCLE) {
            return { kind: 'text', text: m.mv_led_cycle({ label: label.text }) };
        }
        const color = profileLed?.color ?? liveLed?.color;
        if (color === undefined) return { kind: 'text', text: label.text };
        return { kind: 'led-swatch', text: label.text, hex: rgbToHex(color) };
    }

    function rgbToHex(rgb: readonly [number, number, number]): string {
        return (
            '#' +
            rgb
                .map((c) =>
                    Math.max(0, Math.min(255, Math.round(c)))
                        .toString(16)
                        .padStart(2, '0'),
                )
                .join('')
        );
    }

    /** True when the draft has no explicit override for this button —
     *  the label is rendering the default `Disabled` action. Used to
     *  visually distinguish "needs binding" from "deliberately
     *  disabled" in profile mode. */
    function isUnsetInDraft(buttonIndex: number | null): boolean {
        if (buttonIndex === null) return false;
        const view = activeProfileView();
        if (view === null) return false;
        return !view.buttons.some((b) => b.index === buttonIndex);
    }

    function tooltipFor(label: LabelRef): string {
        // In profile mode, build a RatbagButton-shaped row for the
        // tooltip helper so it surfaces the macro sequence the same
        // way it does in live mode.
        const view = activeProfileView();
        if (view !== null && label.buttonIndex !== null) {
            const action = bindingForButton(view, label.buttonIndex);
            return labelTooltip(
                label,
                [{ index: label.buttonIndex, action, supported_action_types: [] }],
            );
        }
        return labelTooltip(label, liveButtons);
    }

    // ───────────────────────────────────────────────────────────────
    // Click → open editor.
    // ───────────────────────────────────────────────────────────────
    function handleLabelClick(label: LabelRef): void {
        if (label.buttonIndex !== null) {
            // Don't open the editor before the live-button metadata
            // lands — the editor needs `supported_action_types` to
            // gate its kind dropdown.
            if (liveButtons.length === 0) return;
            editingIndex = label.buttonIndex;
            editingLedIndex = null;
            return;
        }
        const ledIndex = label.ledIndex ?? null;
        if (ledIndex !== null) {
            // Same gate as buttons — wait for the live LED snapshot
            // so the modal renders against real `supported_modes` /
            // `color_depth` data, not a guess.
            if (liveLeds.length === 0) return;
            // Only open if this device actually exposes the LED we're
            // clicking; the SVG may carry leader labels for hardware
            // variants that don't include all LEDs.
            if (!liveLeds.some((l) => l.index === ledIndex)) return;
            editingLedIndex = ledIndex;
            editingIndex = null;
        }
    }

    /** Hover/focus highlight: recolour the leader for `label` so the
     *  user can trace which label points to which button. Toggles the
     *  accent on both the leader line (`<g id="…-path">`) and the little
     *  anchor square on the button itself (`<rect id="…-leader">`).
     *  Works on `mouseenter`/`focus`, cleared on `mouseleave`/`blur`. */
    function setLeaderPathHover(label: LabelPos, on: boolean): void {
        if (stage === undefined) return;
        stage.querySelector(`#${label.id}-path`)?.classList.toggle('leader-path-active', on);
        stage.querySelector(`#${label.id}-leader`)?.classList.toggle('leader-marker-active', on);
    }

    /** Build the RatbagButton handed to ButtonBindingEditor. In
     *  profile mode the action comes from the draft (falling back to
     *  the source profile if the draft hasn't been forked yet); the
     *  `supported_action_types` come from the live metadata so the
     *  editor can gate its kind dropdown correctly. */
    function editorTargetFor(buttonIndex: number): RatbagButton {
        const live = liveButtons.find((b) => b.index === buttonIndex);
        const view = activeProfileView();
        const action: ButtonAction =
            view === null
                ? (live?.action ?? DEFAULT_ACTION)
                : bindingForButton(view, buttonIndex);
        return {
            index: buttonIndex,
            action,
            supported_action_types: live?.supported_action_types ?? [],
        };
    }

    // ───────────────────────────────────────────────────────────────
    // Auto-save (auto mode) and manual save / apply (manual mode).
    // ───────────────────────────────────────────────────────────────
    // `debounce` expects a void-returning function; wrap the async
    // save in an IIFE so the Promise it returns is consumed inside
    // the wrapper (TS would otherwise complain about
    // no-misused-promises).
    /** True when MouseView is editing live hardware via `applyToActiveProfile`
     *  rather than a gamerat profile via `upsertProfile`. */
    function isBaseMode(): boolean {
        return profile === null;
    }

    /** Push a snapshot to the right backend: gamerat profile store
     *  for profile mode, ratbagd's active hardware profile for
     *  Base mode. */
    async function persistSnapshot(snapshot: GameratProfile): Promise<void> {
        if (isBaseMode()) {
            if (device === null) throw new Error('no device');
            await applyToActiveProfile(
                device.object_path,
                [...snapshot.dpi],
                snapshot.active_dpi_stage,
                snapshot.buttons,
            );
            return;
        }
        await upsertProfile(snapshot);
    }

    /** Run a save/apply unit of work while driving the status pill and
     *  recording how long it took. The timer covers exactly `work` —
     *  the post-success `onprofileschange` refetch is deliberately
     *  excluded so the surfaced duration reflects the action the user
     *  took, not the list refresh that follows. Returns whether it
     *  succeeded so callers can gate follow-up steps. */
    async function runTimedSave(work: () => Promise<void>): Promise<boolean> {
        saveStatus = 'saving';
        saveError = null;
        const start = performance.now();
        try {
            await work();
            lastActionMs = performance.now() - start;
            saveStatus = 'saved';
            return true;
        } catch (error_) {
            saveStatus = 'error';
            saveError = String(error_);
            return false;
        }
    }

    const debouncedSave = debounce((snapshot: GameratProfile) => {
        void (async () => {
            const ok = await runTimedSave(() => persistSnapshot(snapshot));
            // Only the profile-list refresh applies in profile mode —
            // Base mode doesn't touch the profile store.
            if (ok && !isBaseMode()) onprofileschange();
        })();
    }, 500);

    function markDirty(): void {
        if (draft === null) return;
        // Base-mode edits always write through — there's no "save vs
        // apply" distinction because the only target IS the live
        // hardware. The debounce coalesces rapid edits (typing a DPI
        // value) into a single round-trip.
        if (autoswitchEnabled === true || isBaseMode()) {
            saveStatus = 'saving';
            debouncedSave(cloneProfile(draft));
        } else {
            // Manual mode (profile mode only): keep the draft dirty
            // until the user hits Save or Apply.
            saveStatus = 'idle';
        }
    }

    async function manualSave(): Promise<void> {
        const current = draft;
        if (current === null) return;
        const ok = await runTimedSave(() => persistSnapshot(current));
        if (ok && !isBaseMode()) onprofileschange();
    }

    async function manualApply(): Promise<void> {
        const current = draft;
        if (current === null) return;
        const baseMode = isBaseMode();
        // Measure the whole save→apply round-trip as one action so the
        // surfaced duration matches what the user clicked ("Save +
        // apply"), not just the save half. Base mode's persistSnapshot
        // already wrote to hardware, so there's no separate apply.
        const ok = await runTimedSave(async () => {
            await persistSnapshot(current);
            if (!baseMode) await applyProfile(current.id);
        });
        if (ok && !baseMode) onprofileschange();
    }

    /** Profile-mode helpers gate on the `profile` prop (the parent's
     *  authoritative "which mode are we in?" signal), not on `draft`.
     *  The effect that syncs draft from profile can race a fast click,
     *  and using draft as the gate caused the live-mode branch to fire
     *  with the wrong slot — `set_button` then hit `System.Error.ENXIO`
     *  on hardware that wasn't ready for the active-profile write. */
    function ensureDraft(): GameratProfile | null {
        if (draft !== null) return draft;
        if (profile === null) return null;
        const fresh = cloneProfile(profile);
        draft = fresh;
        return fresh;
    }

    async function handleBindingSave(action: ButtonAction): Promise<void> {
        if (editingIndex === null) return;
        const idx = editingIndex;
        if (profile === null) {
            // Live-hardware mode: write through to ratbagd directly.
            if (device === null) return;
            try {
                await writeButton(device.object_path, PROFILE_INDEX_ACTIVE, idx, action);
                liveButtons = await fetchButtons(device.object_path, PROFILE_INDEX_ACTIVE);
            } catch (error_) {
                liveButtonsError = String(error_);
            }
            return;
        }
        const base = ensureDraft();
        if (base === null) return;
        draft = setBinding(base, idx, action);
        markDirty();
    }

    /** "Convert to toggle" path: the user accepted the binding
     *  editor's offer to drop an unbalanced firmware macro in favour
     *  of a software soft-toggle. Fold the soft-macro into the draft
     *  profile and clear the conflicting MACRO action on the same
     *  button — the daemon's `prepare_buttons_for_apply` will override
     *  that button's firmware action with a trampoline KEY at apply
     *  time. Profile mode only; the binding editor gates the
     *  affordance via its `canEditSoftMacros` prop. */
    function handleBindingSoftMacroSave(softMacro: SoftMacro): void {
        const base = ensureDraft();
        if (base === null) return;
        // 1. Replace the soft-macro entry for this button.
        let next = setSoftMacro(base, softMacro.button_index, softMacro);
        // 2. Strip any conflicting firmware-side MACRO action on the
        //    same button — the user explicitly opted out of it.
        next = setBinding(next, softMacro.button_index, {
            kind: BUTTON_ACTION_KIND.NONE,
            value: 0,
            macro_steps: [],
        });
        draft = next;
        markDirty();
    }

    /** Mirror of `handleBindingSave` for the LED color editor. In
     *  profile mode the new state is folded into the draft and
     *  flows through the normal save pipeline; in Base mode we write
     *  directly via `set_led` and re-fetch `liveLeds` so the label
     *  text reflects the applied color immediately. */
    async function handleLedSave(state: Omit<ProfileLed, 'index'>): Promise<void> {
        if (editingLedIndex === null) return;
        const idx = editingLedIndex;
        if (profile === null) {
            if (device === null) return;
            try {
                await writeLed(device.object_path, PROFILE_INDEX_ACTIVE, idx, {
                    index: idx,
                    ...state,
                });
                liveLeds = await fetchLeds(device.object_path, PROFILE_INDEX_ACTIVE);
            } catch (error_) {
                liveLedsError = String(error_);
            }
            return;
        }
        const base = ensureDraft();
        if (base === null) return;
        draft = setLed(base, idx, state);
        markDirty();
    }

    function handleDpiChange(stageIdx: number, value: number): void {
        const base = ensureDraft();
        if (base === null) return;
        draft = setDpiStage(base, stageIdx, value);
        markDirty();
    }
    function handleDpiAdd(): void {
        const base = ensureDraft();
        if (base === null) return;
        const max = device?.max_dpi_stages ?? Number.POSITIVE_INFINITY;
        if (base.dpi.length >= max) return;
        draft = addDpiStage(base, max);
        markDirty();
    }
    function handleDpiRemove(stageIdx: number): void {
        const base = ensureDraft();
        if (base === null) return;
        draft = removeDpiStage(base, stageIdx);
        markDirty();
    }
    function handleDpiActive(stageIdx: number): void {
        const base = ensureDraft();
        if (base === null) return;
        draft = setActiveDpiStage(base, stageIdx);
        markDirty();
    }

    /** "Reset to defaults" — restore canonical Left/Right/Middle/
     *  Back/Forward bindings on the first five buttons, Disabled on
     *  the rest, single 800-DPI stage. Available in profile mode only;
     *  base/live mode would be a one-button shot at ratbagd's setter
     *  per binding, which we'd rather not do silently. */
    function handleResetDefaults(): void {
        const base = ensureDraft();
        if (base === null) return;
        const indices = liveButtons.map((b) => b.index);
        // The model string (`bustype:vid:pid:version`) keys our
        // per-device defaults table — known mice get their real
        // factory bindings, unknown ones fall back to the generic
        // mouse 1–5 + rest-disabled mapping.
        draft = resetProfileToDefaults(base, indices, device?.model ?? '');
        markDirty();
    }

    function saveStatusLabel(): string {
        switch (saveStatus) {
            case 'saving': {
                return m.mv_status_saving();
            }
            case 'saved': {
                return m.mv_status_saved();
            }
            case 'error': {
                return m.mv_status_error({ error: saveError ?? 'unknown' });
            }
            default: {
                return '';
            }
        }
    }

    /** Dimmed timing suffix shown only once an action has completed
     *  successfully. Hidden while saving / on error / before the first
     *  save, so the number never competes with the status word. */
    const savedTiming = $derived(
        saveStatus === 'saved' && lastActionMs !== null
            ? formatDuration(lastActionMs)
            : null,
    );
</script>

<section class="panel mouse-view-panel">
    <h2 class="panel-title"><Icon name="mouse" /> {m.mv_title()}</h2>

    {#if device === null}
        <p class="muted">{m.mv_no_device()}</p>
    {:else}
        <div class="mouse-header-row">
            <p class="muted mouse-meta">
                {device.name} — <span class="font-mono">{device.model}</span>
                {#if svgFilename.length > 0}
                    <span class="mouse-meta-sep">·</span>
                    <span class="font-mono">{svgFilename}</span>
                {/if}
            </p>
            <label class="mouse-profile-picker">
                <span>{m.mv_editing()}</span>
                <Select
                    value={profile?.id ?? ''}
                    options={[
                        { value: '', label: m.mv_picker_base() },
                        ...profiles.map((p) => ({ value: p.id, label: p.name })),
                    ]}
                    onchange={(v: string) => {
                        onselectprofile(v === '' ? null : v);
                    }}
                    title={m.mv_picker_title()}
                    ariaLabel={m.mv_picker_aria()}
                />
            </label>
        </div>

        {#if svgError !== null}
            <p class="error-text">{svgError}</p>
        {:else if svgContent.length === 0}
            <p class="muted">{m.mv_loading_svg()}</p>
        {:else}
            <div bind:this={stage} class="mouse-stage">
                <div class="mouse-svg-frame">
                    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
                    {@html svgContent}
                </div>

                {#if switchingNow}
                    <div
                        class="mouse-switching-badge"
                        role="status"
                        aria-live="polite"
                    >
                        <span class="mouse-switching-spin" aria-hidden="true">
                            <Loader2 size={14} />
                        </span>
                        <span>{m.mv_switching()}</span>
                    </div>
                {/if}

                {#each labels as label (label.id)}
                    {@const isClickable =
                        label.buttonIndex !== null
                        || (label.ledIndex !== null
                            && liveLeds.some((l) => l.index === label.ledIndex))}
                    {@const content = liveLabelContent(label)}
                    <button
                        type="button"
                        class="leader-label"
                        class:leader-label-active={
                            (editingIndex !== null && editingIndex === label.buttonIndex)
                            || (editingLedIndex !== null && editingLedIndex === label.ledIndex)
                        }
                        class:leader-label-static={!isClickable}
                        class:leader-label-unset={isUnsetInDraft(label.buttonIndex)}
                        data-side={label.side}
                        style:left="{String(label.x)}px"
                        style:top="{String(label.y)}px"
                        disabled={!isClickable}
                        title={tooltipFor(label)}
                        onclick={() => { handleLabelClick(label); }}
                        onmouseenter={() => { setLeaderPathHover(label, true); }}
                        onmouseleave={() => { setLeaderPathHover(label, false); }}
                        onfocus={() => { setLeaderPathHover(label, true); }}
                        onblur={() => { setLeaderPathHover(label, false); }}
                    >
                        {#if content.kind === 'led-swatch'}
                            <span>{content.text}</span>
                            <span
                                class="led-label-swatch"
                                style:background-color={content.hex}
                                aria-label={m.mv_led_color({ hex: content.hex })}
                            ></span>
                        {:else}
                            {content.text}
                        {/if}
                    </button>
                {/each}
            </div>

            <!-- Status / hint area -->
            <div class="mouse-status-row">
                {#if liveButtonsError !== null}
                    <p class="error-text mouse-hint">{liveButtonsError}</p>
                {:else if liveLedsError !== null}
                    <p class="error-text mouse-hint">{liveLedsError}</p>
                {:else if liveButtons.length === 0}
                    <p class="muted text-xs mouse-hint">{m.mv_loading_bindings()}</p>
                {:else if profile === null}
                    <p class="muted text-xs mouse-hint">{m.mv_base_hint()}</p>
                {:else}
                    <p class="muted text-xs mouse-hint">
                        {m.mv_profile_hint({
                            name: (draft ?? profile).name,
                            mode: autoswitchEnabled === true
                                ? m.mv_save_mode_auto()
                                : m.mv_save_mode_manual(),
                        })}
                    </p>
                {/if}
            </div>

            {@const view = draft ?? profile}
            {#if view !== null}
                {@const activeStage = liveActiveDpiStage ?? view.active_dpi_stage}
                {@const maxStages = device?.max_dpi_stages ?? Number.POSITIVE_INFINITY}
                <!-- DPI editor — lifted out of ProfilesPanel so DPI
                     and bindings get edited together. The "active"
                     indicator prefers `liveActiveDpiStage` (polled
                     from the device) over the profile record, so
                     on-mouse DPI cycles are reflected immediately. -->
                <div class="dpi-editor">
                    <span class="profile-form-label-text">{m.mv_dpi_stages()}</span>
                    <div class="dpi-stages">
                        {#each view.dpi as dpi, idx (idx)}
                            <div class="dpi-stage" class:dpi-stage-active={idx === activeStage}>
                                <input
                                    class="input-field dpi-stage-input"
                                    type="number"
                                    min="50"
                                    max="32000"
                                    step="50"
                                    value={dpi}
                                    oninput={(e) => {
                                        handleDpiChange(
                                            idx,
                                            Number((e.target as HTMLInputElement).value),
                                        );
                                    }}
                                    aria-label={m.mv_dpi_stage_aria({ idx })}
                                />
                                <label class="dpi-stage-active-label">
                                    <input
                                        type="radio"
                                        name="active-stage"
                                        checked={idx === activeStage}
                                        onchange={() => { handleDpiActive(idx); }}
                                    />
                                    {m.mv_dpi_active()}
                                </label>
                                <button
                                    class="btn-danger-sm"
                                    type="button"
                                    onclick={() => { handleDpiRemove(idx); }}
                                    disabled={view.dpi.length === 1}
                                    title={allSlotsCanDisable
                                        ? m.mv_dpi_remove_disable()
                                        : m.mv_dpi_remove_nodisable()}
                                >
                                    ✕
                                </button>
                            </div>
                        {/each}
                    </div>
                    {#if view.dpi.length < maxStages}
                        <button class="btn-ghost-sm" type="button" onclick={handleDpiAdd}>
                            {m.mv_dpi_add({ count: view.dpi.length, max: maxStages })}
                        </button>
                    {:else}
                        <p class="muted text-xs dpi-stage-cap-hint">
                            {m.mv_dpi_cap({ max: maxStages })}
                        </p>
                    {/if}
                    {#if dpiDisableCaps.length > 0}
                        {#if allSlotsCanDisable}
                            <p class="muted text-xs dpi-stage-cap-hint">{m.mv_dpi_disable_hint()}</p>
                        {:else}
                            <p class="muted text-xs dpi-stage-cap-hint">{m.mv_dpi_nodisable_hint()}</p>
                        {/if}
                    {/if}
                </div>

                <!-- Save / apply controls. Auto mode (and any
                     edit in Base mode) shows just a status pill —
                     edits write through immediately, debounced.
                     Manual + profile mode adds explicit Save / Apply
                     buttons so the user can stage edits before
                     committing them.
                     "Reset to defaults" is always available and just
                     rewrites the draft — actual hardware writes still
                     go through the same save/apply pipeline. -->
                <div class="mouse-save-row">
                    <button
                        class="btn-ghost-sm"
                        type="button"
                        onclick={handleResetDefaults}
                        title={hasDeviceDefaults(device?.model ?? '')
                            ? m.mv_reset_known({ device: device?.name ?? m.mv_this_device() })
                            : m.mv_reset_generic({ device: device?.name ?? m.mv_this_device() })}
                    >
                        {m.mv_reset()}
                    </button>
                    {#if autoswitchEnabled === true || profile === null}
                        <span
                            class="mouse-save-status"
                            data-state={saveStatus}
                            aria-live="polite"
                        >
                            {saveStatusLabel()}
                        </span>
                        {#if savedTiming !== null}
                            <span
                                class="mouse-save-timing"
                                title={m.mv_timing_title()}
                            >· {savedTiming}</span>
                        {/if}
                    {:else}
                        <span class="mouse-save-status" data-state={saveStatus}>
                            {saveStatusLabel()}
                        </span>
                        {#if savedTiming !== null}
                            <span
                                class="mouse-save-timing"
                                title={m.mv_timing_title()}
                            >· {savedTiming}</span>
                        {/if}
                        <button
                            class="btn-ghost"
                            type="button"
                            onclick={manualSave}
                            disabled={saveStatus === 'saving'}
                        >
                            {m.common_save()}
                        </button>
                        <button
                            class="btn-primary"
                            type="button"
                            onclick={manualApply}
                            disabled={saveStatus === 'saving'}
                            title={m.mv_save_apply_title()}
                        >
                            {m.mv_save_apply()}
                        </button>
                    {/if}
                </div>
            {/if}

            {#if editingIndex !== null}
                <ButtonBindingEditor
                    button={editorTargetFor(editingIndex)}
                    devicePath={device.object_path}
                    {softwareMacrosEnabled}
                    canEditSoftMacros={profile !== null}
                    onsave={handleBindingSave}
                    onsavesoftmacro={handleBindingSoftMacroSave}
                    onclose={() => { editingIndex = null; }}
                />
            {/if}

            {#if editingLedIndex !== null}
                {@const ledTarget = liveLeds.find((l) => l.index === editingLedIndex)}
                {#if ledTarget !== undefined}
                    {@const editingProfile = activeProfileView()}
                    {@const profileLed = editingProfile === null
                        ? null
                        : ledForIndex(editingProfile, editingLedIndex)}
                    <LedColorEditor
                        led={ledTarget}
                        initial={profileLed}
                        onsave={handleLedSave}
                        onclose={() => { editingLedIndex = null; }}
                    />
                {/if}
            {/if}
        {/if}
    {/if}
</section>
