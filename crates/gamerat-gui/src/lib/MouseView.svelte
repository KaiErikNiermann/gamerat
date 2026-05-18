<script lang="ts">
    import Loader2 from '@lucide/svelte/icons/loader-2';
    import { onDestroy, tick } from 'svelte';
    import ButtonBindingEditor from './ButtonBindingEditor.svelte';
    import { formatAction } from './button-labels.js';
    import Icon from './Icon.svelte';
    import {
        PROFILE_INDEX_ACTIVE,
        applyProfile,
        fetchActiveDpiStage,
        fetchButtons,
        upsertProfile,
        writeButton,
    } from './ipc.js';
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
        removeDpiStage,
        resetProfileToDefaults,
        setActiveDpiStage,
        setBinding,
        setDpiStage,
    } from './profile-edit.js';
    import { lookupMouseSvg } from './svg-lookup.js';
    import { prepareSvgRoot } from './svg-prep.js';
    import type {
        ButtonAction,
        DeviceInfo,
        GameratProfile,
        RatbagButton,
    } from './types.js';

    interface LabelPos {
        readonly id: string;
        /** Plain button index (`buttonN`) — null for non-button labels. */
        readonly buttonIndex: number | null;
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
        onprofileschange: () => void;
        onselectprofile: (id: string | null) => void;
    }

    const {
        device,
        profile,
        autoswitchEnabled,
        profiles,
        switchingNow,
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

    // ───────────────────────────────────────────────────────────────
    // Profile-mode draft. Synced from the `profile` prop on change of
    // id; in-place edits to the same profile don't clobber the user's
    // unsaved work mid-typing.
    // ───────────────────────────────────────────────────────────────
    let draft = $state<GameratProfile | null>(null);
    let saveStatus = $state<'idle' | 'saving' | 'saved' | 'error'>('idle');
    let saveError = $state<string | null>(null);

    /** Which button index is currently being edited (the popover is
     *  open for that button). Indexes are stable across profile and
     *  live mode. */
    let editingIndex = $state<number | null>(null);

    /** Hardware's live active DPI stage. Polled every 1.5s from the
     *  daemon (`get_active_dpi_stage`); the UI's stage indicator
     *  prefers this over the profile record so on-mouse DPI-up /
     *  DPI-down presses are reflected immediately. `null` means
     *  "no read yet" — we fall back to the record. */
    let liveActiveDpiStage = $state<number | null>(null);

    // Sync the draft when the parent picks a new profile.
    $effect(() => {
        if (profile === null) {
            draft = null;
            saveStatus = 'idle';
            saveError = null;
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
    // source in live mode.
    $effect(() => {
        const path = device?.object_path;
        if (path === undefined) {
            liveButtons = [];
            lastLiveFetchKey = null;
            return;
        }
        const key = path;
        if (key === lastLiveFetchKey) return;
        lastLiveFetchKey = key;
        void (async () => {
            liveButtonsError = null;
            try {
                liveButtons = await fetchButtons(path, PROFILE_INDEX_ACTIVE);
            } catch (error_) {
                liveButtonsError = String(error_);
                liveButtons = [];
            }
        })();
    });

    // Poll the device's currently-active DPI stage. The hardware
    // changes stage on DPI-up / DPI-down / DPI-cycle button presses
    // without sending us a signal, so without this poll the UI would
    // keep showing whatever stage the profile record nominates as
    // active even after the user has rolled past it. 1500 ms is a
    // good middle-ground: snappy enough that the indicator catches
    // up almost immediately, slow enough that we're not flooding
    // dbus / dev-log.
    //
    // Svelte 5's proxy machinery occasionally invalidates this effect
    // on parent reactive flushes even when `device.object_path` is
    // unchanged — same pattern that bit `DevicesPanel`'s slot-map
    // effect. Each spurious re-run would tear down + re-arm the
    // interval AND fire an immediate fetch, which spammed dev-log to
    // the point of `effect_update_depth_exceeded`. The
    // `lastPolledPath` dedupe makes re-runs with the same path no-op.
    let lastPolledPath: string | null = null;
    let activeDpiPoll: ReturnType<typeof setInterval> | undefined;
    function stopDpiPoll(): void {
        if (activeDpiPoll !== undefined) {
            clearInterval(activeDpiPoll);
            activeDpiPoll = undefined;
        }
    }
    // Final teardown on unmount — the effect intentionally doesn't
    // return a cleanup (see comment below), so something has to stop
    // the interval when the component goes away.
    onDestroy(stopDpiPoll);
    $effect(() => {
        const path = device?.object_path ?? null;
        // Path unchanged → spurious Svelte reactive flush; ignore it
        // and let the existing interval keep firing. (Returning a
        // cleanup here would tear down the interval before the body
        // re-runs, and the early-return below would then never
        // re-schedule it — so we manage the interval lifecycle by
        // hand inside the body instead.)
        if (path === lastPolledPath) return;
        lastPolledPath = path;
        stopDpiPoll();
        if (path === null) {
            liveActiveDpiStage = null;
            return;
        }
        const pollFn = (): void => {
            void (async () => {
                try {
                    const stage = await fetchActiveDpiStage(path);
                    // Bail if the device changed between fire and resolve.
                    if (lastPolledPath === path) liveActiveDpiStage = stage;
                } catch {
                    // Indicator falls back to the profile record.
                }
            })();
        };
        pollFn();
        activeDpiPoll = setInterval(pollFn, 1500);
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
            next.push({
                id,
                buttonIndex,
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

    function liveLabelText(label: LabelRef): string {
        if (label.buttonIndex === null) return label.text;
        const view = activeProfileView();
        if (view !== null) {
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
        if (label.buttonIndex === null) return;
        // Don't open the editor before the live-button metadata
        // lands — the editor needs `supported_action_types` to gate
        // its kind dropdown.
        if (liveButtons.length === 0) return;
        editingIndex = label.buttonIndex;
    }

    /** Hover/focus highlight: toggle a class on the matching
     *  `<g id="button{N}-path">` inside the SVG so the leader line
     *  pops in the accent colour. Lets the user visually trace which
     *  label points to which button. Works on `mouseenter`/`focus`,
     *  cleared on `mouseleave`/`blur`. */
    function setLeaderPathHover(label: LabelPos, on: boolean): void {
        if (stage === undefined) return;
        const path = stage.querySelector(`#${label.id}-path`);
        if (path === null) return;
        path.classList.toggle('leader-path-active', on);
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
    const debouncedSave = debounce((snapshot: GameratProfile) => {
        void (async () => {
            saveStatus = 'saving';
            saveError = null;
            try {
                await upsertProfile(snapshot);
                saveStatus = 'saved';
                // Tell the parent so its `profiles` list refreshes —
                // the dropdown text and ProfilesPanel row stay
                // accurate.
                onprofileschange();
            } catch (error_) {
                saveStatus = 'error';
                saveError = String(error_);
            }
        })();
    }, 500);

    function markDirty(): void {
        if (draft === null) return;
        if (autoswitchEnabled === true) {
            // Auto mode: debounced save runs silently.
            saveStatus = 'saving';
            debouncedSave(cloneProfile(draft));
        } else {
            // Manual mode: keep the draft dirty until the user hits
            // Save or Apply. saveStatus going 'idle' from a previous
            // saved state would be confusing — leave it alone.
            saveStatus = 'idle';
        }
    }

    async function manualSave(): Promise<void> {
        if (draft === null) return;
        saveStatus = 'saving';
        saveError = null;
        try {
            await upsertProfile(draft);
            saveStatus = 'saved';
            onprofileschange();
        } catch (error_) {
            saveStatus = 'error';
            saveError = String(error_);
        }
    }

    async function manualApply(): Promise<void> {
        if (draft === null) return;
        await manualSave();
        if (saveStatus === 'saved') {
            try {
                await applyProfile(draft.id);
            } catch (error_) {
                saveStatus = 'error';
                saveError = String(error_);
            }
        }
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

    function handleDpiChange(stageIdx: number, value: number): void {
        const base = ensureDraft();
        if (base === null) return;
        draft = setDpiStage(base, stageIdx, value);
        markDirty();
    }
    function handleDpiAdd(): void {
        const base = ensureDraft();
        if (base === null) return;
        draft = addDpiStage(base);
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
        draft = resetProfileToDefaults(base, indices);
        markDirty();
    }

    function saveStatusLabel(): string {
        switch (saveStatus) {
            case 'saving': {
                return 'saving…';
            }
            case 'saved': {
                return 'saved';
            }
            case 'error': {
                return `error: ${saveError ?? 'unknown'}`;
            }
            default: {
                return '';
            }
        }
    }
</script>

<section class="panel mouse-view-panel">
    <h2 class="panel-title"><Icon name="mouse" /> Mouse</h2>

    {#if device === null}
        <p class="muted">No device connected.</p>
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
                <span>Editing</span>
                <select
                    class="input-field"
                    value={profile?.id ?? ''}
                    onchange={(e) => {
                        const v = (e.target as HTMLSelectElement).value;
                        onselectprofile(v === '' ? null : v);
                    }}
                    title="Pick a saved profile to edit, or 'Base' to see / write the active slot directly."
                >
                    <option value="">Base</option>
                    {#each profiles as p (p.id)}
                        <option value={p.id}>{p.name}</option>
                    {/each}
                </select>
            </label>
        </div>

        {#if svgError !== null}
            <p class="error-text">{svgError}</p>
        {:else if svgContent.length === 0}
            <p class="muted">loading SVG…</p>
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
                        <span>Switching…</span>
                    </div>
                {/if}

                {#each labels as label (label.id)}
                    <button
                        type="button"
                        class="leader-label"
                        class:leader-label-active={editingIndex === label.buttonIndex}
                        class:leader-label-static={label.buttonIndex === null}
                        class:leader-label-unset={isUnsetInDraft(label.buttonIndex)}
                        data-side={label.side}
                        style:left="{String(label.x)}px"
                        style:top="{String(label.y)}px"
                        disabled={label.buttonIndex === null}
                        title={tooltipFor(label)}
                        onclick={() => { handleLabelClick(label); }}
                        onmouseenter={() => { setLeaderPathHover(label, true); }}
                        onmouseleave={() => { setLeaderPathHover(label, false); }}
                        onfocus={() => { setLeaderPathHover(label, true); }}
                        onblur={() => { setLeaderPathHover(label, false); }}
                    >
                        {liveLabelText(label)}
                    </button>
                {/each}
            </div>

            <!-- Status / hint area -->
            <div class="mouse-status-row">
                {#if liveButtonsError !== null}
                    <p class="error-text mouse-hint">{liveButtonsError}</p>
                {:else if liveButtons.length === 0}
                    <p class="muted text-xs mouse-hint">Loading bindings…</p>
                {:else if profile === null}
                    <p class="muted text-xs mouse-hint">
                        Editing the base layer — clicks write directly to the
                        active hardware slot. Pick a profile above to edit a
                        saved record instead.
                    </p>
                {:else}
                    <p class="muted text-xs mouse-hint">
                        Editing profile <strong>{(draft ?? profile).name}</strong>.
                        Click any label to rebind that button — changes
                        are {autoswitchEnabled === true ? 'auto-saved' : 'saved on Save / Apply'}.
                    </p>
                {/if}
            </div>

            {#if profile !== null}
                {@const view = draft ?? profile}
                {@const activeStage = liveActiveDpiStage ?? view.active_dpi_stage}
                <!-- DPI editor — lifted out of ProfilesPanel so DPI
                     and bindings get edited together. The "active"
                     indicator prefers `liveActiveDpiStage` (polled
                     from the device) over the profile record, so
                     on-mouse DPI cycles are reflected immediately. -->
                <div class="dpi-editor">
                    <span class="profile-form-label-text">DPI stages</span>
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
                                    aria-label={`DPI stage ${String(idx)}`}
                                />
                                <label class="dpi-stage-active-label">
                                    <input
                                        type="radio"
                                        name="active-stage"
                                        checked={idx === activeStage}
                                        onchange={() => { handleDpiActive(idx); }}
                                    />
                                    active
                                </label>
                                <button
                                    class="btn-danger-sm"
                                    type="button"
                                    onclick={() => { handleDpiRemove(idx); }}
                                    disabled={view.dpi.length === 1}
                                    title="Remove stage"
                                >
                                    ✕
                                </button>
                            </div>
                        {/each}
                    </div>
                    <button class="btn-ghost-sm" type="button" onclick={handleDpiAdd}>+ add stage</button>
                </div>

                <!-- Save / apply controls. Auto mode shows a status
                     pill (debounced save fires silently); manual mode
                     adds explicit Save / Apply buttons.
                     "Reset to defaults" is always available and just
                     rewrites the draft — actual hardware writes still
                     go through the same save/apply pipeline. -->
                <div class="mouse-save-row">
                    <button
                        class="btn-ghost-sm"
                        type="button"
                        onclick={handleResetDefaults}
                        title="Restore canonical Left/Right/Middle/Back/Forward bindings on buttons 1–5, clear the rest, reset DPI to 800."
                    >
                        Reset to defaults
                    </button>
                    {#if autoswitchEnabled === true}
                        <span
                            class="mouse-save-status"
                            data-state={saveStatus}
                            aria-live="polite"
                        >
                            {saveStatusLabel()}
                        </span>
                    {:else}
                        <span class="mouse-save-status" data-state={saveStatus}>
                            {saveStatusLabel()}
                        </span>
                        <button
                            class="btn-ghost"
                            type="button"
                            onclick={manualSave}
                            disabled={saveStatus === 'saving'}
                        >
                            Save
                        </button>
                        <button
                            class="btn-primary"
                            type="button"
                            onclick={manualApply}
                            disabled={saveStatus === 'saving'}
                            title="Save the profile and write it to the device now."
                        >
                            Save + apply
                        </button>
                    {/if}
                </div>
            {/if}

            {#if editingIndex !== null}
                <ButtonBindingEditor
                    button={editorTargetFor(editingIndex)}
                    onsave={handleBindingSave}
                    onclose={() => { editingIndex = null; }}
                />
            {/if}
        {/if}
    {/if}
</section>
