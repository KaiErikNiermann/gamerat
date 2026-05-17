<script lang="ts">
    import { onMount } from 'svelte';
    import { SvelteMap } from 'svelte/reactivity';
    import Icon from './Icon.svelte';
    import { fetchSlotMap } from './ipc.js';
    import type { DeviceInfo, SlotInfo } from './types.js';

    interface Props {
        devices: DeviceInfo[];
        error: string | null;
        /** Bumped by App.svelte whenever a profile-switched event
         *  fires or the user runs an Apply — re-fetches the slot
         *  map so the table stays in sync without a polling loop. */
        slotMapRevision: number;
    }

    const { devices, error, slotMapRevision }: Props = $props();

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

    onMount(() => {
        for (const d of devices) {
            void refreshSlotMap(d.object_path);
        }
    });

    // Re-fetch on revision bumps (profile-switched / manual apply)
    // or when the device list changes. Both reads need to happen
    // INSIDE the effect for Svelte to track them — pulling them into
    // locals does the trick without the `void` operator gymnastics.
    $effect(() => {
        const _revision = slotMapRevision;
        const list = devices;
        if (_revision < 0) return; // satisfies "use the read"
        for (const d of list) {
            void refreshSlotMap(d.object_path);
        }
    });
</script>

<section class="panel">
    <h2 class="panel-title"><Icon name="mouse" /> Devices</h2>

    {#if error}
        <p class="error-text">{error}</p>
    {:else if devices.length === 0}
        <p class="muted">No devices found.</p>
    {:else}
        <div class="table-wrap">
            <table class="data-table">
                <thead>
                    <tr>
                        <th>Name</th>
                        <th>Model</th>
                        <th>Active</th>
                        <th>Profiles</th>
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
            {@const slots = slotMaps.get(device.object_path) ?? []}
            <h3 class="panel-subtitle">Profiles in slots — {device.name}</h3>
            {#if slots.length === 0}
                <p class="muted text-xs">Loading slot map…</p>
            {:else}
                <ul class="slot-map">
                    {#each slots as slot (slot.index)}
                        <li
                            class="slot-row"
                            class:slot-row-active={slot.is_active}
                            class:slot-row-empty={slot.profile_id.length === 0 && !slot.is_desktop}
                        >
                            <span class="slot-row-index font-mono">Slot {slot.index}</span>
                            <span class="slot-row-name">
                                {#if slot.is_desktop}
                                    Desktop (reserved baseline)
                                {:else if slot.profile_id.length === 0}
                                    (empty)
                                {:else}
                                    {slot.profile_name.length > 0 ? slot.profile_name : slot.profile_id}
                                    <small class="muted font-mono">{slot.profile_id}</small>
                                {/if}
                            </span>
                            {#if slot.is_active}
                                <span class="slot-row-badge">active</span>
                            {/if}
                        </li>
                    {/each}
                </ul>
            {/if}
        {/each}

        {#if slotMapError !== null}
            <p class="error-text text-xs">slot map: {slotMapError}</p>
        {/if}
    {/if}
</section>
