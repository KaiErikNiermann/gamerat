<script lang="ts">
    import RotateCcw from '@lucide/svelte/icons/rotate-ccw';
    import { SvelteMap } from 'svelte/reactivity';
    import { defaultBindingsFor } from './device-defaults.js';
    import Icon from './Icon.svelte';
    import {
        fetchButtons,
        fetchSlotMap,
        wipeGameratState,
        writeSlotContent,
    } from './ipc.js';
    import Modal from './Modal.svelte';
    import { m } from './paraglide/messages.js';
    import type { DeviceInfo, SlotInfo } from './types.js';

    interface Props {
        devices: DeviceInfo[];
        error: string | null;
        /** Bumped by App.svelte whenever a profile-switched event
         *  fires or the user runs an Apply — re-fetches the slot
         *  map so the table stays in sync without a polling loop. */
        slotMapRevision: number;
        /** Fired after a successful "Purge & reset device" flow.
         *  Lets the parent re-fetch profiles + slot map so the
         *  Profiles panel reflects the auto-reimported entries
         *  immediately (otherwise both stay stale until reload). */
        onpurgecomplete?: () => void;
    }

    const {
        devices,
        error,
        slotMapRevision,
        onpurgecomplete,
    }: Props = $props();

    /** Keyed by device object path. SvelteMap so updates are
     *  reactive without copying. */
    const slotMaps = new SvelteMap<string, SlotInfo[]>();
    let slotMapError = $state<string | null>(null);

    async function refreshSlotMap(path: string): Promise<void> {
        try {
            const slots = await fetchSlotMap(path);
            slotMaps.set(path, slots);
            slotMapError = null;
        } catch (error_) {
            slotMapError = String(error_);
        }
    }

    // Refetch whenever the revision changes or the set of device paths
    // changes. The Svelte 5 proxy machinery has been observed to
    // invalidate this effect even when its observable inputs are
    // identical (e.g. on parent re-renders during reloadAll's parallel
    // state writes), which spammed fetchSlotMap → loggedInvoke →
    // dev-log appends until effect_update_depth_exceeded fired. The
    // string-keyed dedupe makes the body idempotent so spurious
    // re-runs are a no-op.
    let lastFetchedKey = '';
    $effect(() => {
        const revision = slotMapRevision;
        const paths = devices.map((d) => d.object_path);
        const key = `${String(revision)}|${paths.join(',')}`;
        if (key === lastFetchedKey) return;
        lastFetchedKey = key;
        for (const path of paths) {
            void refreshSlotMap(path);
        }
    });

    // ───────────────────────────────────────────────────────────────
    // Purge & reset device. Two-step:
    //   1. Rewrite every hardware slot (including Desktop) with the
    //      canonical default profile from `device-defaults.ts` —
    //      reusing the same table that powers MouseView's "Reset to
    //      defaults" button.
    //   2. Wipe gamerat-side state (profiles.toml + slot-cache) so
    //      the next focus event re-imports the fresh slot content.
    // ───────────────────────────────────────────────────────────────

    let purgeConfirmFor = $state<DeviceInfo | null>(null);
    let purging = $state(false);
    let purgeError = $state<string | null>(null);

    async function executePurge(device: DeviceInfo): Promise<void> {
        purging = true;
        purgeError = null;
        try {
            for (let slot = 0; slot < device.profile_count; slot += 1) {
                // We need this slot's actual button indices to build a
                // self-contained default — `defaultBindingsFor` returns
                // one ProfileButton per index it's told about. The
                // device-wide button list isn't enough since libratbag
                // exposes per-profile button arrays.
                const buttons = await fetchButtons(device.object_path, slot);
                const indices = buttons.map((b) => b.index);
                const defaults = defaultBindingsFor(device.model, indices);
                // LEDs aren't part of the canonical default in
                // device-defaults.ts (the existing Reset-to-defaults
                // button leaves them alone), so we pass an empty
                // slice — apply_profile_complete skips the LED phase
                // when leds is empty.
                await writeSlotContent(
                    device.object_path,
                    slot,
                    [800],
                    0,
                    defaults,
                    [],
                );
            }
            await wipeGameratState();
            purgeConfirmFor = null;
            onpurgecomplete?.();
        } catch (error_) {
            purgeError = String(error_);
        } finally {
            purging = false;
        }
    }
</script>

<section class="panel">
    <h2 class="panel-title"><Icon name="mouse" /> {m.devices_title()}</h2>

    {#if error}
        <p class="error-text">{error}</p>
    {:else if devices.length === 0}
        <p class="muted">{m.devices_none()}</p>
    {:else}
        <div class="table-wrap">
            <table class="data-table">
                <thead>
                    <tr>
                        <th>{m.devices_th_name()}</th>
                        <th>{m.devices_th_model()}</th>
                        <th>{m.devices_th_active()}</th>
                        <th>{m.devices_th_profiles()}</th>
                    </tr>
                </thead>
                <tbody>
                    {#each devices as device (device.object_path)}
                        <tr>
                            <td>{device.name}</td>
                            <td class="font-mono text-sm">{device.model}</td>
                            <td class="text-center">{device.active_profile}</td>
                            <td class="text-center">{device.profile_count}</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>

        <!-- Slot map sub-table: which gamerat profile (if any) is in
             each hardware slot. Reflects the daemon's allocator
             state via GetSlotMap. Updated automatically on
             ProfileSwitched events. -->
        {#each devices as device (device.object_path)}
            {@const loaded = slotMaps.has(device.object_path)}
            {@const slots = slotMaps.get(device.object_path) ?? []}
            <h3 class="panel-subtitle">{m.devices_slots_heading({ name: device.name })}</h3>
            {#if !loaded}
                <p class="muted text-xs">{m.devices_slots_loading()}</p>
            {:else if slots.length === 0}
                <p class="muted text-xs">{m.devices_slots_empty()}</p>
            {:else}
                <ul class="slot-map">
                    {#each slots as slot (slot.index)}
                        <li
                            class="slot-row"
                            class:slot-row-active={slot.is_active}
                            class:slot-row-empty={slot.profile_id.length === 0 && !slot.is_desktop}
                        >
                            <span class="slot-row-index font-mono">{m.devices_slot_n({ index: slot.index })}</span>
                            <span class="slot-row-name">
                                {#if slot.is_desktop}
                                    {m.devices_slot_desktop()}
                                {:else if slot.profile_id.length === 0}
                                    {m.devices_slot_empty()}
                                {:else}
                                    {slot.profile_name.length > 0 ? slot.profile_name : slot.profile_id}
                                {/if}
                            </span>
                            {#if slot.is_active}
                                <span class="slot-row-badge">{m.devices_slot_active()}</span>
                            {/if}
                        </li>
                    {/each}
                </ul>
            {/if}

            <div class="device-purge-row">
                <!-- Icon-only destructive action: the full label and
                     consequences are spelled out in the confirm modal
                     that opens on click, and the hover tooltip below
                     surfaces the short version inline. Text-as-button
                     would have made the row visually heavy for an
                     edge-case affordance most users never touch. -->
                <span class="device-purge-tooltip-wrap">
                    <button
                        class="btn-danger-sm device-purge-button"
                        type="button"
                        onclick={() => { purgeConfirmFor = device; purgeError = null; }}
                        aria-label={m.devices_purge_aria({ name: device.name })}
                    >
                        <RotateCcw size={14} />
                    </button>
                    <span class="device-purge-tooltip" role="tooltip">
                        {m.devices_purge_tooltip()}
                    </span>
                </span>
            </div>
        {/each}

        {#if slotMapError !== null}
            <p class="error-text text-xs">{m.devices_slot_map_error({ error: slotMapError })}</p>
        {/if}
    {/if}
</section>

{#if purgeConfirmFor !== null}
    {@const target = purgeConfirmFor}
    <Modal
        label={m.devices_purge_confirm_label()}
        onclose={() => {
            // Don't allow closing while the purge is in flight — the
            // confirmation is the only place that surfaces the
            // operation's progress + outcome.
            if (!purging) purgeConfirmFor = null;
        }}
    >
        <div class="binding-editor-card">
            <header class="binding-editor-head">
                <h3 class="binding-editor-title">{m.devices_purge_confirm_title({ name: target.name })}</h3>
            </header>
            <p>{m.devices_purge_body({ count: target.profile_count, model: target.model })}</p>
            <p class="muted text-xs">{m.devices_purge_caveat()}</p>
            {#if purgeError !== null}
                <p class="error-text">{purgeError}</p>
            {/if}
            <footer class="binding-editor-actions">
                <button
                    class="btn-ghost"
                    type="button"
                    onclick={() => { purgeConfirmFor = null; }}
                    disabled={purging}
                >
                    {m.common_cancel()}
                </button>
                <button
                    class="btn-danger-sm"
                    type="button"
                    onclick={() => { void executePurge(target); }}
                    disabled={purging}
                >
                    {purging ? m.devices_purge_purging() : m.devices_purge_confirm_btn()}
                </button>
            </footer>
        </div>
    </Modal>
{/if}
