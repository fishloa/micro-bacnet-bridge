<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import PointsPanel from '$lib/PointsPanel.svelte';
	import { api, connectSSE, pointKey } from '$lib/api';
	import type { BacnetDevice, BacnetPoint } from '$lib/api';
	import { points, deviceId } from '$lib/stores';

	let devices: BacnetDevice[] = $state([]);
	let selectedDeviceId: number | null = $state(null);
	let showAllDevices = $state(false);
	let disconnectSSE: (() => void) | null = null;

	// Load the point list for a single device or all devices.
	async function loadPoints(devId: number | null) {
		if (devId === null) {
			// "All Devices" — flatten all points with a deviceId annotation.
			const all: BacnetPoint[] = [];
			for (const dev of devices) {
				const pts = await api.getPoints(dev.id);
				for (const p of pts) {
					all.push({ ...p, _deviceId: dev.id } as BacnetPoint & { _deviceId: number });
				}
			}
			$points = all;
		} else {
			$deviceId = devId;
			$points = await api.getPoints(devId);
		}
	}

	async function selectDevice(id: number | null) {
		selectedDeviceId = id;
		showAllDevices = id === null;
		if (id !== null) {
			$deviceId = id;
		}
		await loadPoints(id);
	}

	onMount(async () => {
		devices = await api.getDevices();
		if (devices.length > 0) {
			// Auto-select the first device.
			await selectDevice(devices[0].id);
		}

		// Subscribe to live value updates.
		// SSE keys now include deviceId: `{deviceId}:{objectType}:{objectInstance}`
		disconnectSSE = connectSSE((updates: Record<string, string | number | boolean>) => {
			let changed = false;
			const updated = $points.map(p => {
				const devId = (p as BacnetPoint & { _deviceId?: number })._deviceId ?? $deviceId;
				const key = `${devId}:${pointKey(p)}`;
				if (key in updates) {
					changed = true;
					return { ...p, presentValue: updates[key] };
				}
				return p;
			});
			if (changed) $points = updated;
		});
	});

	onDestroy(() => {
		disconnectSSE?.();
	});

	function deviceOnlineClass(dev: BacnetDevice): string {
		return dev.online ? 'device-online' : 'device-offline';
	}
</script>

<svelte:head>
	<title>BACnet Bridge</title>
</svelte:head>

<div class="dashboard">
	<!-- Left sidebar: device list -->
	<aside class="device-sidebar">
		<div class="sidebar-header">
			<span class="sidebar-title">Devices</span>
			<span class="device-count">{devices.length}</span>
		</div>

		<!-- "All Devices" option -->
		<button
			class="device-item"
			class:active={showAllDevices}
			onclick={() => selectDevice(null)}
		>
			<span class="device-icon">⊞</span>
			<span class="device-name">All Devices</span>
		</button>

		{#each devices as dev (dev.id)}
			<button
				class="device-item {deviceOnlineClass(dev)}"
				class:active={selectedDeviceId === dev.id && !showAllDevices}
				onclick={() => selectDevice(dev.id)}
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
			<div class="no-devices">No devices discovered</div>
		{/each}
	</aside>

	<!-- Right panel: points for selected device -->
	<div class="points-area">
		<PointsPanel showDeviceColumn={showAllDevices} />
	</div>
</div>

<style>
	.dashboard {
		display: flex;
		height: 100%;
		overflow: hidden;
	}

	/* ---- Sidebar ---- */
	.device-sidebar {
		width: 220px;
		flex-shrink: 0;
		display: flex;
		flex-direction: column;
		border-right: 1px solid var(--vui-border);
		background: var(--vui-surface-sub, rgba(255,255,255,0.03));
		overflow-y: auto;
	}

	.sidebar-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 10px 12px 8px;
		font-size: var(--vui-text-xs);
		color: var(--vui-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.06em;
		font-weight: var(--vui-font-semibold);
		border-bottom: 1px solid var(--vui-border);
	}

	.sidebar-title {
		opacity: 0.7;
	}

	.device-count {
		background: var(--vui-accent-dim);
		border-radius: 10px;
		padding: 2px 8px;
		font-size: var(--vui-text-xs);
		color: var(--vui-accent);
		font-weight: var(--vui-font-semibold);
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
		background: var(--vui-color-danger, #ef4444);
	}

	.device-status-dot.online {
		background: var(--vui-color-success, #22c55e);
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

	.device-mac {
		font-family: var(--vui-font-mono);
		font-size: var(--vui-text-xs);
		color: var(--vui-text-sub);
	}

	.no-devices {
		padding: 16px 12px;
		font-size: var(--vui-text-sm);
		color: var(--vui-text-muted);
		text-align: center;
	}

	/* ---- Points area ---- */
	.points-area {
		flex: 1;
		overflow: hidden;
	}
</style>
