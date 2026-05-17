<script lang="ts">
    import { tick } from 'svelte';
    import ButtonBindingEditor from './ButtonBindingEditor.svelte';
    import Icon from './Icon.svelte';
    import { PROFILE_INDEX_ACTIVE, fetchButtons, writeButton } from './ipc.js';
    import {
        findBindingForLabel,
        liveLabelText as resolveLabelText,
        type LabelRef,
    } from './mouse-view-helpers.js';
    import { lookupMouseSvg } from './svg-lookup.js';
    import { prepareSvgRoot } from './svg-prep.js';
    import type { ButtonAction, DeviceInfo, RatbagButton } from './types.js';

    interface LabelPos {
        readonly id: string;
        /** Plain button index (`buttonN`) — empty for non-button labels. */
        readonly buttonIndex: number | null;
        readonly text: string;
        /** Vertical position in pixels, measured from the top of the
         *  shared row containing both gutters and the SVG cell.
         *  Horizontal position is determined by which gutter the
         *  label is rendered into — see `leftLabels` / `rightLabels`. */
        readonly y: number;
        readonly side: 'left' | 'right';
    }

    interface Props {
        device: DeviceInfo | null;
    }

    const { device }: Props = $props();

    let svgContent = $state<string>('');
    let svgError = $state<string | null>(null);
    let svgFilename = $state<string>('');
    let labels = $state<LabelPos[]>([]);
    /** Hardware button bindings for the currently-displayed profile. */
    let buttons = $state<RatbagButton[]>([]);
    let buttonsError = $state<string | null>(null);
    /** Profile slot the editor / labels reflect. -1 = "currently active". */
    let viewedProfile = $state<number>(-1);
    /** Button being edited via the popover, or null when closed. */
    let editingButton = $state<RatbagButton | null>(null);

    let stage: HTMLDivElement | undefined = $state();
    let svgCell: HTMLDivElement | undefined = $state();

    /** Split labels into the two gutter buckets at template time. */
    const leftLabels = $derived(labels.filter((l) => l.side === 'left'));
    const rightLabels = $derived(labels.filter((l) => l.side === 'right'));

    // Re-fetch whenever the connected device changes.
    $effect(() => {
        const model = device?.model ?? '';
        if (model.length === 0) {
            svgContent = '';
            svgError = null;
            // Note: don't reset `buttons` here — the dedicated
            // button-fetch effect below owns that field. Two effects
            // writing the same state on every render was one of two
            // bugs that put us in `effect_update_depth_exceeded`.
            return;
        }
        void loadSvgForModel(model);
    });

    // Memoised fetch key. The effect below re-runs whenever Svelte
    // re-passes the device prop — even when the underlying object
    // path is identical — because reading `device?.object_path`
    // takes a fresh proxy each render. The key guards the actual IPC
    // call so we hit the wire only when path or viewed profile truly
    // changed, breaking the feedback loop with the dev-log SvelteSet
    // (which loggedInvoke writes into).
    let lastFetchKey = $state<string | null>(null);

    // Re-fetch button bindings whenever the device OR viewed profile
    // changes. Errors stay visible in `buttonsError` — labels fall
    // back to the legacy "B0" form so we don't show stale data.
    $effect(() => {
        const path = device?.object_path;
        if (path === undefined) {
            if (lastFetchKey !== null) {
                buttons = [];
                lastFetchKey = null;
            }
            return;
        }
        const profileSlot =
            viewedProfile < 0 ? PROFILE_INDEX_ACTIVE : (viewedProfile >>> 0);
        const key = `${path}#${String(profileSlot)}`;
        if (key === lastFetchKey) return;
        lastFetchKey = key;
        void (async () => {
            buttonsError = null;
            try {
                buttons = await fetchButtons(path, profileSlot);
            } catch (error_) {
                buttonsError = String(error_);
                buttons = [];
            }
        })();
    });

    // Re-measure labels when the SVG content lands in the DOM. Also
    // re-measure on container resize — leader bboxes follow the SVG's
    // intrinsic scaling, so the screen-pixel positions move when the
    // panel widens or narrows.
    $effect(() => {
        if (svgContent.length === 0 || stage === undefined || svgCell === undefined) return;

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
            // Vite's SPA fallback returns 200 + the app's index.html
            // when a URL doesn't match a real asset. Without this
            // sniff, sanitizeSvg would happily ingest the HTML and
            // {@html} would render index.html inside the panel —
            // confusing and unhelpful. Both Content-Type and a quick
            // body-prefix check catch the case.
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
                throw new Error(`${url} response is not an SVG (got ${String(text.length)} bytes, no <svg tag)`);
            }
            svgContent = sanitizeSvg(text);
        } catch (error) {
            svgError = String(error);
            svgContent = '';
        }
    }

    function measureLeaders(): void {
        if (stage === undefined || svgCell === undefined) return;
        const svgRoot = svgCell.querySelector('svg');
        if (svgRoot === null) return;

        // SVG sizing is a `prepareSvgRoot` helper so it can be tested
        // in isolation — getting it wrong (removing the upstream
        // width/height attributes or setting them to empty strings)
        // triggers WebKit's "Invalid value for <svg> attribute
        // width=" warning and the regression test in
        // `svg-prep.test.ts` catches it.
        prepareSvgRoot(svgRoot);

        // Y-coordinate is measured relative to the stage's top edge.
        // Both gutters and the SVG cell share that top edge (grid
        // align-items: stretch), so the same y maps cleanly into
        // either gutter's absolute-positioning coordinate system.
        const stageRect = stage.getBoundingClientRect();
        const next: LabelPos[] = [];

        for (const leader of svgCell.querySelectorAll<SVGElement>('[id$="-leader"]')) {
            const id = leader.id.slice(0, -'-leader'.length);
            if (id.length === 0) continue;

            const rect = leader.getBoundingClientRect();
            if (rect.width === 0 && rect.height === 0) continue;

            const y = rect.top + rect.height / 2 - stageRect.top;

            // Piper's convention: `text-align:end` ⇒ label sits to the
            // *left* of the leader. Everything else ⇒ to the right.
            const style = leader.getAttribute('style') ?? '';
            const side: 'left' | 'right' = style.includes('text-align:end') ? 'left' : 'right';

            const buttonMatch = /^button(\d+)$/u.exec(id);
            const buttonIndex = buttonMatch === null ? null : Number(buttonMatch[1]);
            next.push({
                id,
                buttonIndex,
                text: labelTextFor(id),
                y,
                side,
            });
        }
        labels = next;
    }

    // Both `resolveLabelText` and `findBindingForLabel` live in
    // `mouse-view-helpers.ts` so they can be unit-tested without
    // mounting the component. See `mouse-view-helpers.test.ts`.
    function liveLabelText(label: LabelRef): string {
        return resolveLabelText(label, buttons);
    }

    function handleLabelClick(label: LabelRef): void {
        const binding = findBindingForLabel(label, buttons);
        if (binding === null) return;
        editingButton = binding;
    }

    async function saveBinding(action: ButtonAction): Promise<void> {
        if (editingButton === null || device === null) return;
        const profileSlot =
            viewedProfile < 0 ? PROFILE_INDEX_ACTIVE : (viewedProfile >>> 0);
        await writeButton(device.object_path, profileSlot, editingButton.index, action);
        // Optimistic refresh — re-fetch so the live labels reflect
        // whatever ratbagd actually wrote (in case the daemon
        // clamped / massaged the value).
        buttons = await fetchButtons(device.object_path, profileSlot);
    }

    /**
     * Strip `width="..." height="..."` from the SVG root so our CSS
     * can size it responsively. Also drops the `<?xml?>` PI and any
     * `<!DOCTYPE>` — both are illegal as children of an HTML element
     * when injected via Svelte's `{@html}`.
     */
    function sanitizeSvg(raw: string): string {
        return raw
            .replace(/<\?xml[^?]*\?>/u, '')
            .replace(/<!DOCTYPE[^>]*>/u, '')
            .trim();
    }

    function labelTextFor(id: string): string {
        // Match `button0`, `button12`, `led1`, etc. Render concisely:
        // "B0", "LED 1", etc.
        const buttonMatch = /^button(\d+)$/u.exec(id);
        if (buttonMatch !== null) return `B${buttonMatch[1] ?? ''}`;
        const ledMatch = /^led(\d+)$/u.exec(id);
        if (ledMatch !== null) return `LED ${ledMatch[1] ?? ''}`;
        return id;
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
                <span>Profile</span>
                <select
                    class="input-field"
                    value={String(viewedProfile)}
                    onchange={(e) => {
                        viewedProfile = Number((e.target as HTMLSelectElement).value);
                    }}
                    title="Pick which hardware profile's bindings to view/edit"
                >
                    <option value="-1">Active</option>
                    {#each Array.from({ length: device.profile_count }, (_, i) => i) as i (i)}
                        <option value={String(i)}>Slot {i}</option>
                    {/each}
                </select>
            </label>
        </div>

        {#if svgError !== null}
            <p class="error-text">{svgError}</p>
        {:else if svgContent.length === 0}
            <p class="muted">loading SVG…</p>
        {:else}
            <!-- Piper-style 3-column stage: left labels | SVG | right
                 labels. Each label is anchored to its gutter's inner
                 edge, so labels can never overflow the panel
                 regardless of viewport width. Button bindings are
                 buttons, not spans — the previous a11y workaround
                 (pointer-events: auto on top of pointer-events: none)
                 quietly lost on CSS source order and made the labels
                 unclickable. -->
            <div bind:this={stage} class="mouse-stage">
                <div class="mouse-gutter mouse-gutter-left">
                    {#each leftLabels as label (label.id)}
                        <button
                            type="button"
                            class="leader-label"
                            class:leader-label-active={
                                editingButton !== null
                                && editingButton.index === label.buttonIndex
                            }
                            class:leader-label-static={label.buttonIndex === null}
                            data-side="left"
                            style:top="{String(label.y)}px"
                            disabled={label.buttonIndex === null}
                            title={
                                label.buttonIndex === null
                                    ? label.id
                                    : `Edit binding for button ${String(label.buttonIndex)}`
                            }
                            onclick={() => { handleLabelClick(label); }}
                        >
                            {liveLabelText(label)}
                        </button>
                    {/each}
                </div>

                <div bind:this={svgCell} class="mouse-svg-cell">
                    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
                    {@html svgContent}
                </div>

                <div class="mouse-gutter mouse-gutter-right">
                    {#each rightLabels as label (label.id)}
                        <button
                            type="button"
                            class="leader-label"
                            class:leader-label-active={
                                editingButton !== null
                                && editingButton.index === label.buttonIndex
                            }
                            class:leader-label-static={label.buttonIndex === null}
                            data-side="right"
                            style:top="{String(label.y)}px"
                            disabled={label.buttonIndex === null}
                            title={
                                label.buttonIndex === null
                                    ? label.id
                                    : `Edit binding for button ${String(label.buttonIndex)}`
                            }
                            onclick={() => { handleLabelClick(label); }}
                        >
                            {liveLabelText(label)}
                        </button>
                    {/each}
                </div>
            </div>

            {#if buttonsError !== null}
                <p class="error-text mouse-hint">{buttonsError}</p>
            {:else if buttons.length === 0}
                <p class="muted text-xs mouse-hint">Loading bindings…</p>
            {:else}
                <p class="muted text-xs mouse-hint">
                    Click any label to rebind that button — the editor shows only the
                    action kinds the firmware supports per-button.
                </p>
            {/if}

            {#if editingButton !== null}
                <ButtonBindingEditor
                    button={editingButton}
                    onsave={saveBinding}
                    onclose={() => { editingButton = null; }}
                />
            {/if}
        {/if}
    {/if}
</section>
