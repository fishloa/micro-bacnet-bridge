<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import PointsPanel from '$lib/PointsPanel.svelte';
	import { api, connectSSE, pointKey } from '$lib/api';
	import { points } from '$lib/stores';

	let disconnectSSE: (() => void) | null = null;

	onMount(async () => {
		// Load points from our bridge device (first/primary device)
		const devices = await api.getDevices();
		if (devices.length > 0) {
			$points = await api.getPoints(devices[0].id);
		}

		// Subscribe to live value updates
		disconnectSSE = connectSSE((updates) => {
			let changed = false;
			const updated = $points.map(p => {
				const key = pointKey(p);
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
</script>

<svelte:head>
	<title>BACnet Bridge</title>
</svelte:head>

<div class="dashboard">
	<PointsPanel />
</div>

<style>
	.dashboard {
		height: 100%;
		overflow: hidden;
	}
</style>
