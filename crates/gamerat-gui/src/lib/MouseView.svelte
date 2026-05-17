<script lang="ts">
    import { tick } from 'svelte';
    import ButtonBindingEditor from './ButtonBindingEditor.svelte';
    import Icon from './Icon.svelte';
    import { PROFILE_INDEX_ACTIVE, fetchButtons, writeButton } from './ipc.js';
    import {
        findBindingForLabel,
        labelTooltip,
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
        /** Horizontal position of the leader's centre, in pixels from
         *  the stage's left edge. Labels position themselves OUTWARD
         *  from this anchor — left-side labels translate by -100% so
         *  their right edge lands on `x`, right-side labels keep
         *  their left edge at `x`. Matches Piper's MouseMap.do_size_allocate
         *  where label.right_edge = leader.x - spacing (left) or
         *  label.left_edge = leader.x + spacing (right). */
        readonly x: number;
        /** Vertical position in pixels from the stage's top edge. */
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
        if (stage === undefined) return;
        const svgRoot = stage.querySelector('svg');
        if (svgRoot === null) return;

        // SVG sizing helper — tested in svg-prep.test.ts to make sure
        // we don't reintroduce the "Invalid value for <svg> attribute
        // width=\"\"" WebKit warning.
        prepareSvgRoot(svgRoot);

        // Both x and y are measured relative to the stage. Labels are
        // absolute-positioned within the stage at the leader's centre,
        // then translate themselves outward (-100%, 0) for left-side
        // labels so their right edge lands on the anchor — matching
        // Piper's mousemap.do_size_allocate convention.
        const stageRect = stage.getBoundingClientRect();
        const next: LabelPos[] = [];

        for (const leader of stage.querySelectorAll<SVGElement>('[id$="-leader"]')) {
            const id = leader.id.slice(0, -'-leader'.length);
            if (id.length === 0) continue;

            const rect = leader.getBoundingClientRect();
            if (rect.width === 0 && rect.height === 0) continue;

            const x = rect.left + rect.width / 2 - stageRect.left;
            const y = rect.top + rect.height / 2 - stageRect.top;

            // Piper convention: `text-align:end` ⇒ label sits to the
            // LEFT of the leader. Everything else ⇒ to the right.
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
            <!-- Single-stage layout. The SVG sits centered in the
                 stage with capped max-width so labels always have
                 room to live outside it. Labels are absolute-positioned
                 at the leader's measured (x, y) and translate
                 themselves outward — matching Piper's mousemap so
                 the arrows drawn IN the SVG actually point to the
                 labels. overflow: visible on the stage means the
                 rare label that extends beyond the panel just shows;
                 the SVG cap stops it from happening in practice. -->
            <div bind:this={stage} class="mouse-stage">
                <div class="mouse-svg-frame">
                    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
                    {@html svgContent}
                </div>

                {#each labels as label (label.id)}
                    <button
                        type="button"
                        class="leader-label"
                        class:leader-label-active={
                            editingButton !== null
                            && editingButton.index === label.buttonIndex
                        }
                        class:leader-label-static={label.buttonIndex === null}
                        data-side={label.side}
                        style:left="{String(label.x)}px"
                        style:top="{String(label.y)}px"
                        disabled={label.buttonIndex === null}
                        title={labelTooltip(label, buttons)}
                        onclick={() => { handleLabelClick(label); }}
                    >
                        {liveLabelText(label)}
                    </button>
                {/each}
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
