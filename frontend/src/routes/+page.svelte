<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import PointsPanel from '$lib/PointsPanel.svelte';
	import { api, connectSSE } from '$lib/api';
	import { points } from '$lib/stores';

	let disconnectSSE: (() => void) | null = null;

	onMount(async () => {
		const allDevices = await api.getDevices();
		const allPoints = await Promise.all(allDevices.map(d => api.getPoints(d.id)));
		$points = allPoints.flat();

		// Subscribe to live value updates
		disconnectSSE = connectSSE((updates) => {
			let changed = false;
			const updated = $points.map(p => {
				const key = `${p.objectType}:${p.objectInstance}`;
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
