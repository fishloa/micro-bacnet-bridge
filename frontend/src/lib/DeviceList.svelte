<script lang="ts">
	import { devices, selectedDeviceId } from './stores';
	import type { BacnetDevice } from './api';

	function selectDevice(device: BacnetDevice) {
		$selectedDeviceId = device.id;
	}
</script>

<div class="device-list">
	<div class="vui-section-header">Devices</div>
	<div class="device-items">
		{#each $devices as device (device.id)}
			<button
				class="device-item vui-transition"
				class:selected={$selectedDeviceId === device.id}
				onclick={() => selectDevice(device)}
			>
				<div class="device-status">
					<span class="status-dot" class:online={device.online} class:offline={!device.online}></span>
				</div>
				<div class="device-info">
					<div class="device-name">{device.name}</div>
					<div class="device-meta text-xs text-muted">
						ID {device.id} &middot; MAC {device.mac} &middot; {device.vendor}
					</div>
				</div>
			</button>
		{/each}
	</div>
</div>

<style>
	.device-list {
		display: flex;
		flex-direction: column;
		height: 100%;
		overflow: hidden;
	}
	.device-items {
		flex: 1;
		overflow-y: auto;
		padding: var(--vui-space-xs);
	}
	.device-item {
		display: flex;
		align-items: center;
		gap: var(--vui-space-sm);
		width: 100%;
		padding: var(--vui-space-sm) var(--vui-space-md);
		border: none;
		background: none;
		border-radius: var(--vui-radius-md);
		cursor: pointer;
		text-align: left;
		color: var(--vui-text);
	}
	.device-item:hover {
		background: var(--vui-surface-hover);
	}
	.device-item.selected {
		background: var(--vui-accent-dim);
		border: 1px solid var(--vui-accent-border);
	}
	.status-dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		flex-shrink: 0;
	}
	.status-dot.online {
		background: var(--vui-accent);
		box-shadow: 0 0 6px var(--vui-accent-glow);
	}
	.status-dot.offline {
		background: var(--vui-text-dim);
	}
	.device-name {
		font-size: var(--vui-text-sm);
		font-weight: var(--vui-font-medium);
	}
	.device-meta {
		margin-top: 2px;
	}
</style>
