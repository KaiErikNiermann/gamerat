<script lang="ts">
    import { tick } from 'svelte';
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

        // Constrain the SVG to the panel width but preserve aspect ratio.
        // The SVG itself comes from upstream with fixed width/height attrs;
        // we override here for responsive sizing.
        svgRoot.setAttribute('width', '100%');
        svgRoot.setAttribute('height', 'auto');
        svgRoot.style.maxHeight = '480px';
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
</script>

<section class="panel mouse-view-panel">
    <h2 class="panel-title">🖱️ Mouse</h2>

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
            <div bind:this={container} class="mouse-stage">
                <!-- eslint-disable-next-line svelte/no-at-html-tags -->
                {@html svgContent}
                {#each labels as label (label.id)}
                    <span
                        class="leader-label"
                        data-side={label.side}
                        style={`left: ${String(label.x)}px; top: ${String(label.y)}px;`}
                    >
                        {label.text}
                    </span>
                {/each}
            </div>
        {/if}
    {/if}
</section>
