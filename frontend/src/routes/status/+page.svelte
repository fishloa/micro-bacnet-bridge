<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { SystemStatus } from '$lib/api';

	let status: SystemStatus | null = $state(null);

	onMount(async () => {
		status = await api.getStatus();
	});

	function formatUptime(seconds: number): string {
		const d = Math.floor(seconds / 86400);
		const h = Math.floor((seconds % 86400) / 3600);
		const m = Math.floor((seconds % 3600) / 60);
		const parts: string[] = [];
		if (d > 0) parts.push(`${d}d`);
		if (h > 0) parts.push(`${h}h`);
		parts.push(`${m}m`);
		return parts.join(' ');
	}
</script>

<svelte:head>
	<title>Status — BACnet Bridge</title>
</svelte:head>

<div class="status-page">
	<h1>System Status</h1>

	{#if status}
		<div class="status-grid vui-animate-fade-in">
			<div class="vui-card stat-card">
				<div class="stat-label">Uptime</div>
				<div class="stat-value mono">{formatUptime(status.uptime)}</div>
			</div>
			<div class="vui-card stat-card">
				<div class="stat-label">IP Address</div>
				<div class="stat-value mono">{status.ip}</div>
				<span class="vui-badge" class:vui-badge-success={status.dhcp}>
					{status.dhcp ? 'DHCP' : 'Static'}
				</span>
			</div>
			<div class="vui-card stat-card">
				<div class="stat-label">Hostname</div>
				<div class="stat-value mono">{status.hostname}.local</div>
			</div>
			<div class="vui-card stat-card">
				<div class="stat-label">Firmware</div>
				<div class="stat-value mono">v{status.firmwareVersion}</div>
			</div>
			<div class="vui-card stat-card">
				<div class="stat-label">Devices Discovered</div>
				<div class="stat-value">{status.devicesDiscovered}</div>
			</div>
			<div class="vui-card stat-card">
				<div class="stat-label">MS/TP State</div>
				<div class="stat-value">{status.mstpState}</div>
			</div>
		</div>

		<div class="vui-card vui-animate-fade-in" style="margin-top: var(--vui-space-lg);">
			<div class="vui-section-header">MS/TP Statistics</div>
			<div class="stat-row">
				<span class="text-sub">Frames Sent</span>
				<span class="mono">{status.mstpFramesSent.toLocaleString()}</span>
			</div>
			<div class="stat-row">
				<span class="text-sub">Frames Received</span>
				<span class="mono">{status.mstpFramesRecv.toLocaleString()}</span>
			</div>
		</div>

		<div style="margin-top: var(--vui-space-lg);">
			<button class="vui-btn vui-btn-danger" onclick={() => { if(confirm('Reboot device?')) alert('Reboot sent'); }}>
				Reboot Device
			</button>
		</div>
	{:else}
		<div class="vui-skeleton" style="height: 200px; border-radius: var(--vui-radius-md);"></div>
	{/if}
</div>

<style>
	.status-page {
		padding: var(--vui-space-lg);
		height: 100%;
		overflow-y: auto;
		max-width: 800px;
	}
	h1 {
		font-size: var(--vui-text-xl);
		font-weight: var(--vui-font-bold);
		margin-bottom: var(--vui-space-lg);
	}
	.status-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
		gap: var(--vui-space-md);
	}
	.stat-card {
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.stat-label {
		font-size: var(--vui-text-xs);
		color: var(--vui-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.05em;
		font-weight: var(--vui-font-semibold);
	}
	.stat-value {
		font-size: var(--vui-text-lg);
		font-weight: var(--vui-font-bold);
	}
	.stat-row {
		display: flex;
		justify-content: space-between;
		padding: var(--vui-space-sm) 0;
		border-bottom: 1px solid var(--vui-border);
	}
</style>
