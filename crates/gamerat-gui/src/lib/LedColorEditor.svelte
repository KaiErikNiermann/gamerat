<script lang="ts">
    import { LED_COLOR_DEPTH, LED_MODE } from './types.js';
    import type { ProfileLed, RatbagLed } from './types.js';

    interface Props {
        /** Hardware-side LED snapshot — drives the supported_modes /
         *  color_depth gates. The current Color / Mode / Brightness in
         *  this snapshot are also used as the editor's initial values
         *  when no profile-side override is supplied. */
        led: RatbagLed;
        /** Optional profile-side override. When the user is editing in
         *  profile mode and has previously set a value for this LED,
         *  this is that recorded state; takes precedence over the live
         *  hardware values when seeding the form. Pass `null` to use
         *  the hardware snapshot alone (Base mode, or profile-mode
         *  with no prior override). */
        initial: ProfileLed | null;
        onsave: (next: Omit<ProfileLed, 'index'>) => Promise<void> | void;
        onclose: () => void;
    }

    const { led, initial, onsave, onclose }: Props = $props();

    /** Mode / color / brightness the form is currently producing. The
     *  initial values come from `initial` (profile-side override) when
     *  available, falling back to the hardware snapshot. */
    let mode = $state<number>(initial?.mode ?? led.mode);
    let hex = $state<string>(rgbToHex(initial?.color ?? led.color));
    let brightness = $state<number>(initial?.brightness ?? led.brightness);
    let saving = $state<boolean>(false);
    let error = $state<string | null>(null);

    /** Modes the firmware actually accepts on this LED. The Cycle/
     *  Breathing/Off options are disabled in the picker if missing. */
    const supportedModes = $derived<readonly number[]>(led.supported_modes);

    /** True when the device LED can render arbitrary RGB. False for
     *  monochrome LEDs (logo on / off only) — we hide the color picker
     *  to avoid promising something the firmware won't deliver. */
    const supportsColor = $derived(
        led.color_depth !== LED_COLOR_DEPTH.MONOCHROME,
    );

    /** Mode values where the current color contributes to the rendered
     *  output. Color picker only shows for these; for OFF/CYCLE it
     *  serves no purpose and is hidden to reduce noise. */
    const colorRelevant = $derived(
        mode === LED_MODE.ON || mode === LED_MODE.BREATHING,
    );

    /** Brightness slot only matters for color-driven modes on
     *  color-capable hardware. OFF mode hides the slider entirely;
     *  monochrome LEDs hide it too (brightness in the libratbag sense
     *  is "color intensity", not the same as an LED on/off bit). */
    const brightnessRelevant = $derived(mode !== LED_MODE.OFF && supportsColor);

    interface ModeOption {
        readonly value: number;
        readonly label: string;
    }

    const MODE_OPTIONS: readonly ModeOption[] = [
        { value: LED_MODE.OFF, label: 'Off' },
        { value: LED_MODE.ON, label: 'Solid' },
        { value: LED_MODE.BREATHING, label: 'Breathing' },
        { value: LED_MODE.CYCLE, label: 'Cycle' },
    ];

    function clamp8(v: number): number {
        return Math.max(0, Math.min(255, Math.round(v)));
    }

    function rgbToHex(rgb: readonly [number, number, number]): string {
        return (
            '#' +
            rgb.map((c) => clamp8(c).toString(16).padStart(2, '0')).join('')
        );
    }

    function hexToRgb(input: string): [number, number, number] | null {
        const trimmed = input.trim().replace(/^#/u, '');
        if (!/^[0-9a-f]{6}$/iu.test(trimmed)) return null;
        return [
            Number.parseInt(trimmed.slice(0, 2), 16),
            Number.parseInt(trimmed.slice(2, 4), 16),
            Number.parseInt(trimmed.slice(4, 6), 16),
        ];
    }

    interface BuildResult {
        readonly ok: boolean;
        readonly payload: Omit<ProfileLed, 'index'> | null;
        readonly error: string | null;
    }

    function buildPayload(): BuildResult {
        const rgb = hexToRgb(hex);
        if (rgb === null) {
            return { ok: false, payload: null, error: `Invalid hex color "${hex}" — use #rrggbb` };
        }
        return {
            ok: true,
            error: null,
            payload: {
                mode: mode as ProfileLed['mode'],
                color: rgb,
                brightness: clamp8(brightness),
            },
        };
    }

    async function handleSave(event: Event): Promise<void> {
        event.preventDefault();
        const result = buildPayload();
        if (!result.ok || result.payload === null) {
            error = result.error ?? 'Invalid LED state';
            return;
        }
        saving = true;
        error = null;
        try {
            await onsave(result.payload);
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
    aria-label={`Edit LED ${String(led.index)}`}
    onclick={(e) => {
        if (e.target === e.currentTarget) onclose();
    }}
    onkeydown={(e) => {
        if (e.key === 'Escape') onclose();
    }}
    tabindex="-1"
>
    <form class="binding-editor-card" onsubmit={handleSave}>
        <header class="binding-editor-head">
            <h3 class="binding-editor-title">LED {led.index}</h3>
            <button
                type="button"
                class="btn-ghost-sm"
                onclick={onclose}
                aria-label="Close LED editor"
            >
                close
            </button>
        </header>

        <div class="binding-editor-row">
            <span class="binding-editor-label">Mode</span>
            <div class="led-mode-row" role="radiogroup" aria-label="LED mode">
                {#each MODE_OPTIONS as opt (opt.value)}
                    {@const disabled = !supportedModes.includes(opt.value)}
                    <button
                        type="button"
                        class="led-mode-chip"
                        class:led-mode-chip-active={mode === opt.value}
                        {disabled}
                        title={disabled
                            ? `This device's LED ${String(led.index)} doesn't support ${opt.label.toLowerCase()} mode`
                            : opt.label}
                        onclick={() => {
                            mode = opt.value;
                        }}
                        aria-pressed={mode === opt.value}
                    >
                        {opt.label}
                    </button>
                {/each}
            </div>
        </div>

        {#if supportsColor && colorRelevant}
            <label class="binding-editor-row">
                <span class="binding-editor-label">Color</span>
                <div class="led-color-row">
                    <input
                        type="color"
                        class="led-color-swatch"
                        value={hex}
                        oninput={(e) => {
                            hex = (e.target as HTMLInputElement).value;
                        }}
                        aria-label="Pick LED color"
                    />
                    <input
                        type="text"
                        class="input-field led-color-hex"
                        bind:value={hex}
                        spellcheck="false"
                        autocomplete="off"
                        placeholder="#ff3344"
                    />
                </div>
            </label>
        {:else if !supportsColor}
            <p class="muted text-xs led-cap-hint">
                Monochrome LED — color is fixed by the firmware. Mode is
                the only adjustable axis.
            </p>
        {/if}

        {#if brightnessRelevant}
            <label class="binding-editor-row">
                <span class="binding-editor-label">
                    Brightness <span class="muted">({brightness})</span>
                </span>
                <input
                    type="range"
                    min="0"
                    max="255"
                    step="1"
                    class="led-brightness-slider"
                    value={brightness}
                    oninput={(e) => {
                        brightness = Number((e.target as HTMLInputElement).value);
                    }}
                />
            </label>
        {/if}

        {#if error !== null}
            <p class="error-text">{error}</p>
        {/if}

        <footer class="binding-editor-actions">
            <button class="btn-ghost" type="button" onclick={onclose}>Cancel</button>
            <button class="btn-primary" type="submit" disabled={saving}>
                {saving ? 'Saving…' : 'Apply'}
            </button>
        </footer>
    </form>
</div>

<style>
    .led-mode-row {
        display: flex;
        gap: 0.4rem;
        flex-wrap: wrap;
    }

    .led-mode-chip {
        padding: 0.35rem 0.75rem;
        border: 1px solid var(--color-border);
        background: var(--color-surface-2, transparent);
        color: inherit;
        border-radius: 0.4rem;
        font-size: 0.85rem;
        cursor: pointer;
        transition: background-color 80ms linear, border-color 80ms linear;
    }

    .led-mode-chip:hover:not(:disabled) {
        border-color: var(--color-accent, currentcolor);
    }

    .led-mode-chip-active {
        background: var(--color-accent, currentcolor);
        color: var(--color-on-accent, #fff);
        border-color: var(--color-accent, currentcolor);
    }

    .led-mode-chip:disabled {
        opacity: 0.4;
        cursor: not-allowed;
    }

    .led-color-row {
        display: flex;
        gap: 0.5rem;
        align-items: center;
    }

    .led-color-swatch {
        width: 2.6rem;
        height: 2rem;
        padding: 0;
        border: 1px solid var(--color-border);
        border-radius: 0.4rem;
        background: transparent;
        cursor: pointer;
    }

    .led-color-hex {
        font-family: var(--font-mono, ui-monospace, monospace);
        flex: 1;
        max-width: 9rem;
    }

    .led-brightness-slider {
        width: 100%;
    }

    .led-cap-hint {
        margin-top: 0.25rem;
    }
</style>
