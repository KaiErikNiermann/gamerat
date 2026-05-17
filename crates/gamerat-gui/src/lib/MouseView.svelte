<script lang="ts">
    import { tick } from 'svelte';
    import ButtonBindingEditor from './ButtonBindingEditor.svelte';
    import { formatAction } from './button-labels.js';
    import Icon from './Icon.svelte';
    import { PROFILE_INDEX_ACTIVE, fetchButtons, writeButton } from './ipc.js';
    import { lookupMouseSvg } from './svg-lookup.js';
    import { prepareSvgRoot } from './svg-prep.js';
    import type { ButtonAction, DeviceInfo, RatbagButton } from './types.js';

    interface LabelPos {
        readonly id: string;
        /** Plain button index (`buttonN`) — empty for non-button labels. */
        readonly buttonIndex: number | null;
        readonly text: string;
        readonly x: number;
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

    let container: HTMLDivElement | undefined = $state();

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
        if (svgContent.length === 0 || container === undefined) return;

        let observer: ResizeObserver | undefined;
        const target = container; // capture narrowed value before await
        void (async () => {
            await tick();
            measureLeaders();
            // After first paint, set up a resize observer so we
            // re-measure on layout changes.
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
        if (container === undefined) return;
        const svgRoot = container.querySelector('svg');
        if (svgRoot === null) return;

        // SVG sizing is a `prepareSvgRoot` helper so it can be tested
        // in isolation — getting it wrong (removing the upstream
        // width/height attributes or setting them to empty strings)
        // triggers WebKit's "Invalid value for <svg> attribute
        // width=" warning and the regression test in
        // `svg-prep.test.ts` catches it.
        prepareSvgRoot(svgRoot);

        const containerRect = container.getBoundingClientRect();
        const next: LabelPos[] = [];

        for (const leader of container.querySelectorAll<SVGElement>('[id$="-leader"]')) {
            const id = leader.id.slice(0, -'-leader'.length);
            // Skip empty / no-id sentinels.
            if (id.length === 0) continue;

            // getBoundingClientRect handles transforms (e.g. scale(-1) on
            // mirrored buttons) automatically.
            const rect = leader.getBoundingClientRect();
            if (rect.width === 0 && rect.height === 0) continue;

            const x = rect.left + rect.width / 2 - containerRect.left;
            const y = rect.top + rect.height / 2 - containerRect.top;

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
                x,
                y,
                side,
            });
        }
        labels = next;
    }

    /**
     * Live label text — falls back to the static "B0" / "LED 0" form
     * when we don't (yet) have a binding for that button, and shows
     * the human-readable action otherwise.
     */
    function liveLabelText(label: LabelPos): string {
        if (label.buttonIndex === null) return label.text;
        const binding = buttons.find((b) => b.index === label.buttonIndex);
        if (binding === undefined) return label.text;
        return formatAction(binding.action);
    }

    function handleLabelClick(label: LabelPos): void {
        if (label.buttonIndex === null) return;
        const binding = buttons.find((b) => b.index === label.buttonIndex);
        if (binding === undefined) {
            // Daemon hasn't returned this button yet (still loading or
            // an error blocked it). Don't open the editor against a
            // synthesised default; surface the error in the panel.
            return;
        }
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
            <div
                bind:this={container}
                class="mouse-stage"
            >
                <div class="mouse-stage-inner">
                    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
                    {@html svgContent}
                </div>
                {#each labels as label (label.id)}
                    <!-- svelte-ignore a11y_no_static_element_interactions -->
                    <!-- svelte-ignore a11y_click_events_have_key_events -->
                    <span
                        class="leader-label"
                        class:leader-label-clickable={label.buttonIndex !== null}
                        class:leader-label-active={
                            editingButton !== null && editingButton.index === label.buttonIndex
                        }
                        data-side={label.side}
                        style={`left: ${String(label.x)}px; top: ${String(label.y)}px;`}
                        title={
                            label.buttonIndex === null
                                ? label.id
                                : `Click to edit Button ${String(label.buttonIndex)}`
                        }
                        onclick={() => { handleLabelClick(label); }}
                    >
                        {liveLabelText(label)}
                    </span>
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
