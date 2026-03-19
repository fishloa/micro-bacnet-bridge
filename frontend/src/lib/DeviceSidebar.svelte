<script lang="ts">
	import type { BacnetDevice } from './api';

	let {
		devices,
		selectedDeviceId,
		showAllOption = false,
		showAllDevices = false,
		onSelect,
	}: {
		devices: BacnetDevice[];
		selectedDeviceId: number | null;
		showAllOption?: boolean;
		showAllDevices?: boolean;
		onSelect: (id: number | null) => void;
	} = $props();
</script>

<aside class="device-sidebar">
	{#if showAllOption}
		<button
			class="device-item"
			class:active={showAllDevices}
			onclick={() => onSelect(null)}
		>
			<span class="device-icon">⊞</span>
			<span class="device-name">All Devices</span>
		</button>
	{/if}

	{#each devices as dev (dev.id)}
		<button
			class="device-item"
			class:active={selectedDeviceId === dev.id && !showAllDevices}
			onclick={() => onSelect(dev.id)}
		>
			<span class="device-status-dot" class:online={dev.online}></span>
			<span class="device-info">
				<span class="device-name">{dev.name}</span>
				<span class="device-meta">
					<span class="vui-badge vui-badge-info">ID {dev.id}</span>
					{#if dev.mac}
						<span class="vui-badge vui-badge-info">MAC {dev.mac}</span>
					{/if}
				</span>
			</span>
		</button>
	{:else}
		<div class="no-devices text-sub text-sm">No devices discovered</div>
	{/each}
</aside>

<style>
	.device-sidebar {
		width: 220px;
		flex-shrink: 0;
		display: flex;
		flex-direction: column;
		border-right: 1px solid var(--vui-border);
		overflow-y: auto;
	}

	.device-item {
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 10px 12px;
		cursor: pointer;
		border: 1px solid transparent;
		background: transparent;
		color: var(--vui-text);
		text-align: left;
		width: 100%;
		font-size: var(--vui-text-base);
		border-bottom: 1px solid var(--vui-border);
		border-radius: var(--vui-radius-md);
		transition: background 0.12s;
	}

	.device-item:hover {
		background: var(--vui-surface-hover);
	}

	.device-item.active {
		background: var(--vui-accent-dim);
		border: 1px solid var(--vui-accent-border);
		color: var(--vui-accent);
	}

	.device-icon {
		font-size: 14px;
		opacity: 0.6;
	}

	.device-status-dot {
		width: 7px;
		height: 7px;
		border-radius: 50%;
		flex-shrink: 0;
		background: var(--vui-danger, #ef4444);
	}

	.device-status-dot.online {
		background: var(--vui-accent);
	}

	.device-info {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
	}

	.device-name {
		font-weight: var(--vui-font-semibold);
		color: var(--vui-text);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.device-meta {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.no-devices {
		padding: var(--vui-space-md);
		text-align: center;
	}
</style>
