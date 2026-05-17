<script lang="ts">
    import { tick } from 'svelte';
    import Icon from './Icon.svelte';
    import { lookupMouseSvg } from './svg-lookup.js';
    import type { DeviceInfo } from './types.js';

    interface LabelPos {
        readonly id: string;
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
    /** Currently-selected button id (e.g. "button3"). Drives the inspector. */
    let selectedButton = $state<string | null>(null);

    let container: HTMLDivElement | undefined = $state();

    // Re-fetch whenever the connected device changes.
    $effect(() => {
        const model = device?.model ?? '';
        if (model.length === 0) {
            svgContent = '';
            svgError = null;
            return;
        }
        void loadSvgForModel(model);
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
            const res = await fetch(`/mice/${filename}`);
            if (!res.ok) {
                throw new Error(`fetch ${filename}: ${String(res.status)}`);
            }
            const text = await res.text();
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

        // Upstream SVGs declare fixed width/height attrs. We re-size
        // for responsive layout but the *critical* bit is letting
        // overflow render: leader marker rects often sit just outside
        // the canonical viewBox (Piper's convention for label anchor
        // points). Without overflow=visible they'd be clipped and
        // getBoundingClientRect would return 0×0, so the label
        // wouldn't be placed at all — which was the historical "mouse
        // is half rendered" symptom.
        svgRoot.setAttribute('overflow', 'visible');
        svgRoot.removeAttribute('width');
        svgRoot.removeAttribute('height');
        svgRoot.style.width = '100%';
        svgRoot.style.height = 'auto';
        svgRoot.style.maxHeight = '420px';
        svgRoot.style.display = 'block';

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

            next.push({ id, text: labelTextFor(id), x, y, side });
        }
        labels = next;
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

    /**
     * Walk up the click target's ancestors looking for the nearest
     * element whose id is `buttonN`. Returns the id or `null` if the
     * click landed on the chassis / a non-interactive surface.
     */
    function buttonAtTarget(target: EventTarget | null): string | null {
        let node = target instanceof Element ? target : null;
        // sonarjs's narrowing of `container` here is fooled by Svelte
        // 5's $state(undefined) — at runtime container can be a real
        // HTMLDivElement, but the type system says HTMLDivElement |
        // undefined and SonarJS reasons that `node !== container` is
        // always true when container is undefined. We DO want the
        // identity check, so compare against `container as Element |
        // undefined`.
        const stop: Element | undefined = container;
        while (node !== null && node !== stop) {
            if (/^button\d+$/u.test(node.id)) return node.id;
            node = node.parentElement;
        }
        return null;
    }

    function handleStageClick(event: MouseEvent): void {
        const id = buttonAtTarget(event.target);
        selectedButton = id; // null clears the inspector
    }

    // ratbagd / Piper convention: button0 is left click, 1 right,
    // 2 middle, 3 back, 4 forward. Past that the assignment is
    // hardware-specific; we just label them by index.
    const WELL_KNOWN_BUTTONS = new Map<string, string>([
        ['0', 'Left click'],
        ['1', 'Right click'],
        ['2', 'Middle click'],
        ['3', 'Back'],
        ['4', 'Forward'],
    ]);

    /** Pretty index for the inspector ("B3", "Left click", etc). */
    function buttonHumanName(id: string): string {
        const m = /^button(\d+)$/u.exec(id);
        const n = m === null ? '?' : (m[1] ?? '?');
        const name = WELL_KNOWN_BUTTONS.get(n) ?? `Button ${n}`;
        return `${name} (B${n})`;
    }
</script>

<section class="panel mouse-view-panel">
    <h2 class="panel-title"><Icon name="mouse" /> Mouse</h2>

    {#if device === null}
        <p class="muted">No device connected.</p>
    {:else}
        <p class="muted mouse-meta">
            {device.name} — <span class="font-mono">{device.model}</span>
            {#if svgFilename.length > 0}
                <span class="mouse-meta-sep">·</span>
                <span class="font-mono">{svgFilename}</span>
            {/if}
        </p>

        {#if svgError !== null}
            <p class="error-text">{svgError}</p>
        {:else if svgContent.length === 0}
            <p class="muted">loading SVG…</p>
        {:else}
            <!-- Click delegation: the SVG itself is rendered via {@html}
                 so we can't attach per-button listeners declaratively.
                 The container catches all clicks and `buttonAtTarget`
                 walks up to find the originating button id. -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
                bind:this={container}
                class="mouse-stage"
                class:mouse-stage-selecting={selectedButton !== null}
                data-selected-button={selectedButton ?? ''}
                onclick={handleStageClick}
            >
                <div class="mouse-stage-inner">
                    <!-- eslint-disable-next-line svelte/no-at-html-tags -->
                    {@html svgContent}
                </div>
                {#each labels as label (label.id)}
                    <span
                        class="leader-label"
                        class:leader-label-active={label.id === selectedButton}
                        data-side={label.side}
                        style={`left: ${String(label.x)}px; top: ${String(label.y)}px;`}
                    >
                        {label.text}
                    </span>
                {/each}
            </div>

            <!-- Button inspector. Empty until the user clicks a button. -->
            {#if selectedButton !== null}
                <div class="button-inspector">
                    <div class="button-inspector-head">
                        <span class="button-inspector-name">
                            {buttonHumanName(selectedButton)}
                        </span>
                        <button
                            class="btn-ghost-sm"
                            type="button"
                            onclick={() => { selectedButton = null; }}
                            aria-label="Close button inspector"
                        >
                            close
                        </button>
                    </div>
                    <p class="muted text-xs">
                        Button bindings are coming in a later release — the daemon
                        doesn't expose ratbagd's button-mapping surface yet. For now
                        you can see the canonical id; macro editing will live here
                        once <code>Profile.Buttons</code> is wired through.
                    </p>
                </div>
            {:else}
                <p class="muted text-xs mouse-hint">
                    Click any button on the diagram to inspect it.
                </p>
            {/if}
        {/if}
    {/if}
</section>
