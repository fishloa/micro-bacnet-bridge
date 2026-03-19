<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import PointsPanel from '$lib/PointsPanel.svelte';
	import DeviceSidebar from '$lib/DeviceSidebar.svelte';
	import { api, connectSSE, pointKey } from '$lib/api';
	import type { BacnetDevice, BacnetPoint, SerialStatus } from '$lib/api';
	import { points, deviceId } from '$lib/stores';

	let devices: BacnetDevice[] = $state([]);
	let selectedDeviceId: number | null = $state(null);
	let showAllDevices = $state(false);
	let disconnectSSE: (() => void) | null = null;
	let serial: SerialStatus = $state({ baud: 0, parity: '8N1', framesRx: 0, framesTx: 0, errorsRx: 0, busActive: false, detecting: false });

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
		disconnectSSE = connectSSE(
			(updates: Record<string, string | number | boolean>) => {
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
			},
			(s: SerialStatus) => { serial = s; },
		);
	});

	onDestroy(() => {
		disconnectSSE?.();
	});

</script>

<svelte:head>
	<title>BACnet Bridge</title>
</svelte:head>

<div class="dashboard-page">
	<h1 class="vui-page-title">Dashboard</h1>

	<div class="serial-status-bar">
		<span class="serial-indicator" class:active={serial.busActive} class:detecting={serial.detecting}></span>
		{#if serial.detecting}
			<span class="serial-label">Auto-detecting baud rate...</span>
		{:else if serial.baud === 0}
			<span class="serial-label">Serial: not configured</span>
		{:else}
			<span class="serial-label">Serial: {serial.baud} {serial.parity}</span>
			<span class="serial-stat">RX: {serial.framesRx}</span>
			<span class="serial-stat">TX: {serial.framesTx}</span>
			{#if serial.errorsRx > 0}
				<span class="serial-stat serial-error">Errors: {serial.errorsRx}</span>
			{/if}
		{/if}
	</div>

	<div class="dashboard-body">
	<DeviceSidebar
		{devices}
		{selectedDeviceId}
		showAllOption={true}
		{showAllDevices}
		onSelect={selectDevice}
	/>

	<!-- Right panel: points for selected device -->
	<div class="points-area">
		<PointsPanel showDeviceColumn={showAllDevices} />
	</div>
	</div>
</div>

<style>
	.dashboard-page {
		display: flex;
		flex-direction: column;
		height: 100%;
		overflow: hidden;
		padding: var(--vui-space-lg) var(--vui-space-lg) 0;
	}
	.dashboard-body {
		display: flex;
		flex: 1;
		overflow: hidden;
	}
	.points-area {
		flex: 1;
		overflow: hidden;
	}
	.serial-status-bar {
		display: flex;
		align-items: center;
		gap: var(--vui-space-sm);
		padding: var(--vui-space-xs) var(--vui-space-sm);
		font-size: var(--vui-font-size-sm);
		color: var(--vui-text-secondary);
		border-bottom: 1px solid var(--vui-border);
	}
	.serial-indicator {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: var(--vui-text-muted);
	}
	.serial-indicator.active {
		background: var(--vui-success);
	}
	.serial-indicator.detecting {
		background: var(--vui-warning);
		animation: pulse 1s infinite;
	}
	@keyframes pulse {
		0%, 100% { opacity: 1; }
		50% { opacity: 0.3; }
	}
	.serial-label {
		font-weight: 500;
	}
	.serial-stat {
		color: var(--vui-text-muted);
	}
	.serial-error {
		color: var(--vui-danger);
		font-weight: 500;
	}
</style>
