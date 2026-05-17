<script lang="ts">
    import Icon from './Icon.svelte';
    import type { DeviceInfo } from './types.js';

    interface Props {
        devices: DeviceInfo[];
        error: string | null;
    }

    const { devices, error }: Props = $props();
</script>

<section class="panel panel-wide">
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
                        <th>Active profile</th>
                        <th>Profiles</th>
                        <th>Object path</th>
                    </tr>
                </thead>
                <tbody>
                    {#each devices as device (device.object_path)}
                        <tr>
                            <td>{device.name}</td>
                            <td class="font-mono text-sm">{device.model}</td>
                            <td class="text-center">{device.active_profile}</td>
                            <td class="text-center">{device.profile_count}</td>
                            <td class="font-mono text-xs muted">{device.object_path}</td>
                        </tr>
                    {/each}
                </tbody>
            </table>
        </div>
    {/if}
</section>
