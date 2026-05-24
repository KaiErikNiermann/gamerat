<!--
    Themed dropdown — replacement for `<select>` so the open popup
    inherits the app's font + theme tokens (native `<select>` popups
    on WebKitGTK are GTK-owned and can't be skinned). Generic on the
    value type so call sites can bind enums/numerics directly.
-->
<script lang="ts" generics="T extends string | number">
    import ChevronDown from '@lucide/svelte/icons/chevron-down';
    import { tick } from 'svelte';

    /** Move `node` into <body> for as long as it's mounted. Without
     *  this, the popover stays a child of the trigger and therefore
     *  of the app's scroll container; on WebKitGTK that container's
     *  overlay scrollbar composites above the fixed popover. As a
     *  body child the popover lives outside the scroll layer's paint
     *  tree entirely, so the scrollbar can't draw over it. */
    function portalToBody(node: HTMLElement): { destroy: () => void } {
        document.body.append(node);
        return {
            destroy: () => {
                node.remove();
            },
        };
    }

    interface Option {
        readonly value: T;
        readonly label: string;
        readonly disabled?: boolean;
    }

    interface Props {
        value: T;
        options: readonly Option[];
        onchange?: (next: T) => void;
        /** Shown in the trigger when `value` doesn't match any option
         *  (typical for "" sentinel = "no selection"). */
        placeholder?: string;
        disabled?: boolean;
        /** Extra class on the trigger button — used for site-specific
         *  layout overrides (e.g. flex-1, width caps). */
        className?: string;
        ariaLabel?: string;
        title?: string;
        required?: boolean;
    }

    // Single `let` destructure: Svelte 5 needs `let` (not const) so
    // the bindable `value` can be reassigned by callers binding to
    // it. eslint's prefer-const can't see through the $props macro.
    // The `= $bindable()` on a required prop is the Svelte 5 marker
    // for "this prop participates in bind:" — not a literal default.
    /* eslint-disable prefer-const, @typescript-eslint/no-useless-default-assignment */
    let {
        value = $bindable(),
        options,
        onchange,
        placeholder,
        disabled = false,
        className = '',
        ariaLabel,
        title,
        required = false,
    }: Props = $props();
    /* eslint-enable prefer-const, @typescript-eslint/no-useless-default-assignment */

    let open = $state(false);
    let highlightedIndex = $state(-1);
    let triggerEl = $state<HTMLButtonElement | null>(null);
    let listEl = $state<HTMLUListElement | null>(null);
    /** Viewport-relative coordinates for the popover. Recomputed
     *  every time the menu opens and on every scroll/resize while
     *  it's open. Using `position: fixed` here is what lets the menu
     *  escape clipping ancestors (`overflow: hidden`/`auto` panels)
     *  and render above their scrollbars. */
    let menuTop = $state(0);
    let menuLeft = $state(0);
    let menuMinWidth = $state(0);
    /** Typeahead buffer — accumulates chars within a short window so
     *  typing "ag" jumps to "agnostic" not just the latest letter. */
    let typeahead = $state('');
    let typeaheadTimer: ReturnType<typeof setTimeout> | null = null;

    const currentIndex = $derived(
        options.findIndex((o) => o.value === value),
    );
    const currentLabel = $derived.by((): string => {
        // `currentIndex` comes from `options.findIndex(...)` above —
        // it's either a valid index or -1, both safe. No
        // attacker-controlled key path.
        // eslint-disable-next-line security/detect-object-injection
        const cur = options[currentIndex];
        if (cur !== undefined) return cur.label;
        return placeholder ?? '';
    });

    function recomputeMenuPosition(): void {
        if (triggerEl === null) return;
        const r = triggerEl.getBoundingClientRect();
        menuTop = r.bottom + 4;
        menuLeft = r.left;
        menuMinWidth = r.width;
    }

    function openMenu(initialIndex?: number): void {
        if (disabled) return;
        recomputeMenuPosition();
        open = true;
        highlightedIndex = initialIndex ?? Math.max(currentIndex, 0);
        // Wait for the listbox to mount, then scroll the highlighted
        // option into view + give it focus so screen readers and
        // keyboard users land on it immediately.
        void tick().then(() => {
            scrollHighlightedIntoView();
        });
    }

    /** While the menu is open, follow the trigger if anything moves
     *  it — page scroll, window resize, layout shifts. `capture: true`
     *  on the scroll listener catches scrolls inside nested
     *  scrollable ancestors (the trigger's container scroll, sidebars,
     *  modal backdrops). */
    $effect(() => {
        if (!open) return;
        globalThis.addEventListener('scroll', recomputeMenuPosition, {
            passive: true,
            capture: true,
        });
        globalThis.addEventListener('resize', recomputeMenuPosition);
        return () => {
            globalThis.removeEventListener('scroll', recomputeMenuPosition, {
                capture: true,
            });
            globalThis.removeEventListener('resize', recomputeMenuPosition);
        };
    });

    function closeMenu(returnFocus = true): void {
        open = false;
        highlightedIndex = -1;
        if (returnFocus) triggerEl?.focus();
    }

    function commit(index: number): void {
        // `index` comes from internal navigation state (highlight /
        // typeahead / click handlers), all bounded by `options.length`.
        // eslint-disable-next-line security/detect-object-injection
        const opt = options[index];
        if (opt === undefined || opt.disabled === true) return;
        const wasDifferent = opt.value !== value;
        value = opt.value;
        if (wasDifferent) onchange?.(opt.value);
        closeMenu();
    }

    function moveHighlight(delta: number): void {
        if (options.length === 0) return;
        let next = highlightedIndex;
        // Skip past disabled options so keyboard nav stays useful.
        // for-of doesn't apply: we need the index modulo arithmetic
        // to wrap around the start/end of the list.
        // eslint-disable-next-line @typescript-eslint/prefer-for-of
        for (let i = 0; i < options.length; i += 1) {
            next = (next + delta + options.length) % options.length;
            // `next` is modulo `options.length` — always in-bounds. No
            // attacker-controlled key path.
            // eslint-disable-next-line security/detect-object-injection
            if (options[next]?.disabled !== true) {
                highlightedIndex = next;
                scrollHighlightedIntoView();
                return;
            }
        }
    }

    function jumpHighlight(toFirst: boolean): void {
        const dir = toFirst ? 1 : -1;
        let i = toFirst ? 0 : options.length - 1;
        while (i >= 0 && i < options.length) {
            // `i` is bounded by the while condition. No
            // attacker-controlled key path.
            // eslint-disable-next-line security/detect-object-injection
            if (options[i]?.disabled !== true) {
                highlightedIndex = i;
                scrollHighlightedIntoView();
                return;
            }
            i += dir;
        }
    }

    function scrollHighlightedIntoView(): void {
        if (listEl === null || highlightedIndex < 0) return;
        // `highlightedIndex` is always a valid options-array index by
        // construction (guarded above + only set inside this file).
        // HTMLCollection's bracket access is the standard DOM idiom.
        // eslint-disable-next-line security/detect-object-injection
        const el = listEl.children[highlightedIndex] as HTMLElement | undefined;
        el?.scrollIntoView({ block: 'nearest' });
    }

    function pushTypeahead(ch: string): void {
        typeahead += ch.toLowerCase();
        if (typeaheadTimer !== null) clearTimeout(typeaheadTimer);
        typeaheadTimer = setTimeout(() => {
            typeahead = '';
        }, 600);
        // Find the first option whose label starts with the buffer.
        // Falls back to a `contains` match so partial words also work.
        const buf = typeahead;
        const startMatch = options.findIndex(
            (o) =>
                o.disabled !== true && o.label.toLowerCase().startsWith(buf),
        );
        const idx
            = startMatch === -1
                ? options.findIndex(
                    (o) =>
                        o.disabled !== true
                        && o.label.toLowerCase().includes(buf),
                )
                : startMatch;
        if (idx !== -1) {
            highlightedIndex = idx;
            scrollHighlightedIntoView();
        }
    }

    function onTriggerKeydown(e: KeyboardEvent): void {
        if (disabled) return;
        switch (e.key) {
            case 'ArrowDown':
            case 'ArrowUp':
            case 'Enter':
            case ' ': {
                e.preventDefault();
                openMenu();
                break;
            }
            case 'Home': {
                e.preventDefault();
                openMenu();
                jumpHighlight(true);
                break;
            }
            case 'End': {
                e.preventDefault();
                openMenu();
                jumpHighlight(false);
                break;
            }
            default: {
                // Single-character keydown on the closed trigger opens
                // + typeahead-jumps in one go (mirrors native <select>).
                if (e.key.length === 1) {
                    openMenu();
                    pushTypeahead(e.key);
                }
            }
        }
    }

    function onListKeydown(e: KeyboardEvent): void {
        switch (e.key) {
            case 'ArrowDown': {
                e.preventDefault();
                moveHighlight(1);
                break;
            }
            case 'ArrowUp': {
                e.preventDefault();
                moveHighlight(-1);
                break;
            }
            case 'Home': {
                e.preventDefault();
                jumpHighlight(true);
                break;
            }
            case 'End': {
                e.preventDefault();
                jumpHighlight(false);
                break;
            }
            case 'Enter':
            case ' ': {
                e.preventDefault();
                if (highlightedIndex >= 0) commit(highlightedIndex);
                break;
            }
            case 'Escape':
            case 'Tab': {
                e.preventDefault();
                closeMenu();
                break;
            }
            default: {
                if (e.key.length === 1) pushTypeahead(e.key);
            }
        }
    }

    function onDocumentMouseDown(e: MouseEvent): void {
        if (!open) return;
        const target = e.target as Node | null;
        if (
            target !== null
            && (triggerEl?.contains(target) === true
                || listEl?.contains(target) === true)
        ) {
            return;
        }
        closeMenu(false);
    }
</script>

<svelte:document onmousedown={onDocumentMouseDown} />

<div class="select-root" class:select-open={open}>
    <button
        type="button"
        bind:this={triggerEl}
        class={`input-field select-trigger ${className}`}
        class:select-trigger-placeholder={currentIndex < 0}
        {disabled}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-label={ariaLabel}
        aria-required={required}
        {title}
        onclick={() => {
            if (open) closeMenu();
            else openMenu();
        }}
        onkeydown={onTriggerKeydown}
    >
        <span class="select-trigger-stack">
            <span class="select-trigger-label">{currentLabel}</span>
            <!-- Width sizer: every option label is rendered invisibly
                 in the same grid cell so the trigger's intrinsic width
                 grows to the widest option. Mirrors native <select>
                 sizing without JS measurement. -->
            <span class="select-trigger-sizer" aria-hidden="true">
                {#each options as opt (`${String(opt.value)}-sz`)}
                    <span>{opt.label}</span>
                {/each}
                {#if placeholder !== undefined}
                    <span>{placeholder}</span>
                {/if}
            </span>
        </span>
        <span class="select-trigger-chevron" aria-hidden="true">
            <ChevronDown size={12} strokeWidth={1.75} />
        </span>
    </button>

    {#if open}
        <ul
            bind:this={listEl}
            use:portalToBody
            class="select-menu"
            role="listbox"
            tabindex="-1"
            aria-label={ariaLabel}
            style:top="{String(menuTop)}px"
            style:left="{String(menuLeft)}px"
            style:min-width="{String(menuMinWidth)}px"
            onkeydown={onListKeydown}
        >
            {#each options as opt, i (`${String(opt.value)}-${String(i)}`)}
                <li
                    class="select-option"
                    class:select-option-active={i === highlightedIndex}
                    class:select-option-selected={opt.value === value}
                    class:select-option-disabled={opt.disabled === true}
                    role="option"
                    aria-selected={opt.value === value}
                    aria-disabled={opt.disabled === true}
                    onmouseenter={() => {
                        if (opt.disabled !== true) highlightedIndex = i;
                    }}
                    onmousedown={(e) => {
                        // mousedown not click — click would fire after the
                        // document-mousedown handler closes the menu via
                        // an outside-click race.
                        e.preventDefault();
                        commit(i);
                    }}
                >
                    {opt.label}
                </li>
            {/each}
        </ul>
    {/if}
</div>

<style>
    .select-root {
        position: relative;
        display: inline-flex;
        min-width: 0;
    }

    .select-trigger {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        padding-right: 1.7rem;
        text-align: left;
        cursor: pointer;
        position: relative;
        min-width: 0;
    }

    /* Grid overlay: the visible label and the (invisible) sizer share
       one grid cell. The cell sizes to max(label intrinsic width,
       sizer intrinsic width). Since the sizer claims the width of the
       widest option, the trigger ends up at least that wide — even
       when the current label is much shorter (placeholder, base, …).
       Mirrors native `<select>` behaviour without JS. */
    .select-trigger-stack {
        display: grid;
        grid-template-areas: "stack";
        flex: 1;
        min-width: 0;
        align-items: center;
    }

    .select-trigger-label,
    .select-trigger-sizer {
        grid-area: stack;
        min-width: 0;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    /* Each option label stacks as a grid row inside the sizer.
       `height: 0; overflow: hidden` zeroes their visual contribution
       but the intrinsic content width still feeds the parent cell's
       width — so the trigger expands horizontally without growing
       vertically. */
    .select-trigger-sizer {
        display: grid;
        visibility: hidden;
        pointer-events: none;
        user-select: none;
        height: 0;
    }

    .select-trigger-placeholder {
        color: var(--color-muted);
    }

    .select-trigger-chevron {
        position: absolute;
        right: 0.55rem;
        top: 50%;
        transform: translateY(-50%);
        display: inline-flex;
        color: var(--color-text);
        opacity: 0.7;
        transition: transform 0.12s ease;
        pointer-events: none;
    }

    .select-open .select-trigger-chevron {
        transform: translateY(-50%) rotate(180deg);
    }

    /* Fixed positioning so the popover escapes any clipping ancestor
       (overflow: hidden / auto panels, modal cards) and renders on
       the viewport layer — above the scrollbar of any inner scroll
       container. Coordinates are written inline from the trigger's
       getBoundingClientRect on open + scroll + resize. */
    .select-menu {
        position: fixed;
        max-height: 16rem;
        overflow-y: auto;
        margin: 0;
        padding: 0.25rem;
        background: var(--color-surface);
        border: 1px solid var(--color-border);
        border-radius: 0.4rem;
        box-shadow: 0 6px 18px rgba(0, 0, 0, 0.28);
        list-style: none;
        z-index: 1000;
        font-family: inherit;
        font-size: 0.85rem;
        color: var(--color-text);
    }

    .select-option {
        padding: 0.35rem 0.6rem;
        border-radius: 0.3rem;
        cursor: pointer;
        white-space: nowrap;
        transition: background-color 80ms linear, color 80ms linear;
    }

    .select-option-active:not(.select-option-disabled) {
        background-color: var(--color-row-hover);
    }

    .select-option-selected:not(.select-option-disabled) {
        background-color: var(--color-accent);
        color: var(--color-accent-fg);
    }

    .select-option-disabled {
        color: var(--color-muted);
        cursor: not-allowed;
        opacity: 0.6;
    }
</style>
