<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { SystemStatus } from '$lib/api';

	let status: SystemStatus | null = $state(null);

	// ---- OTA firmware update state ----
	let otaFile: File | null = $state(null);
	let otaProgress: number = $state(0); // 0–100
	let otaStatus: 'idle' | 'uploading' | 'success' | 'error' = $state('idle');
	let otaMessage: string = $state('');

	onMount(async () => {
		status = await api.getStatus();
	});

	async function reboot() {
		if (!confirm('Reboot device?')) return;
		try {
			await fetch('/api/v1/system/reboot', { method: 'POST' });
		} catch { /* device will go offline */ }
	}

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

	function onFileChange(e: Event) {
		const input = e.target as HTMLInputElement;
		otaFile = input.files?.[0] ?? null;
		otaStatus = 'idle';
		otaMessage = '';
		otaProgress = 0;
	}

	async function uploadFirmware() {
		if (!otaFile) return;

		const MAX_SIZE = 1_500_000;
		if (otaFile.size > MAX_SIZE) {
			otaStatus = 'error';
			otaMessage = `File is too large (${(otaFile.size / 1024).toFixed(0)} KB). Maximum allowed size is ${(MAX_SIZE / 1024).toFixed(0)} KB.`;
			return;
		}

		if (!confirm('This will overwrite the running firmware and reboot the device. Continue?')) {
			return;
		}

		otaStatus = 'uploading';
		otaProgress = 0;
		otaMessage = '';

		try {
			// Use XMLHttpRequest for upload progress events.
			await new Promise<void>((resolve, reject) => {
				const xhr = new XMLHttpRequest();
				xhr.open('POST', '/api/v1/system/firmware');
				xhr.setRequestHeader('Content-Type', 'application/octet-stream');

				xhr.upload.addEventListener('progress', (e) => {
					if (e.lengthComputable) {
						otaProgress = Math.round((e.loaded / e.total) * 100);
					}
				});

				xhr.addEventListener('load', () => {
					if (xhr.status === 200) {
						otaProgress = 100;
						resolve();
					} else {
						reject(new Error(`Server returned ${xhr.status}: ${xhr.responseText}`));
					}
				});

				xhr.addEventListener('error', () => reject(new Error('Network error during upload')));
				xhr.addEventListener('timeout', () => reject(new Error('Upload timed out')));
				xhr.timeout = 120_000; // 2 minutes

				xhr.send(otaFile);
			});

			otaStatus = 'success';
			otaMessage = 'Firmware uploaded successfully. The device is rebooting — this page will become available again in 10–30 seconds.';
		} catch (err: unknown) {
			otaStatus = 'error';
			otaMessage = err instanceof Error ? err.message : 'Unknown upload error';
		}
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
			<button class="vui-btn vui-btn-danger" onclick={reboot}>
				Reboot Device
			</button>
		</div>

		<!-- OTA Firmware Update -->
		<div class="vui-card vui-animate-fade-in" style="margin-top: var(--vui-space-lg);">
			<div class="vui-section-header">Firmware Update</div>
			<p class="text-sub" style="margin-bottom: var(--vui-space-md);">
				Upload a raw ARM binary (<code>.bin</code>) to update the firmware.
				The device will reboot automatically after the upload completes.
			</p>

			{#if otaStatus === 'success'}
				<div class="vui-alert vui-alert-success">
					{otaMessage}
				</div>
			{:else if otaStatus === 'error'}
				<div class="vui-alert vui-alert-danger">
					{otaMessage}
				</div>
			{/if}

			<div class="ota-row">
				<label class="vui-btn vui-btn-secondary ota-file-label">
					{otaFile ? otaFile.name : 'Choose firmware .bin…'}
					<input
						type="file"
						accept=".bin"
						class="ota-file-input"
						onchange={onFileChange}
						disabled={otaStatus === 'uploading'}
					/>
				</label>

				<button
					class="vui-btn vui-btn-primary"
					onclick={uploadFirmware}
					disabled={!otaFile || otaStatus === 'uploading'}
				>
					{otaStatus === 'uploading' ? 'Uploading…' : 'Upload & Flash'}
				</button>
			</div>

			{#if otaStatus === 'uploading'}
				<div class="ota-progress-wrap">
					<div class="ota-progress-bar" style="width: {otaProgress}%;"></div>
				</div>
				<p class="text-sub ota-progress-label">{otaProgress}%</p>
			{/if}

			<p class="ota-warning">
				Warning: do not power off the device during a firmware update.
				A power loss mid-write may leave the device unbootable and require
				manual reflash via USB.
			</p>
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

	/* ---- OTA firmware update ---- */
	.ota-row {
		display: flex;
		gap: var(--vui-space-sm);
		align-items: center;
		flex-wrap: wrap;
		margin-bottom: var(--vui-space-md);
	}
	.ota-file-label {
		cursor: pointer;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		max-width: 340px;
	}
	.ota-file-input {
		display: none;
	}
	.ota-progress-wrap {
		height: 6px;
		background: var(--vui-border);
		border-radius: 3px;
		overflow: hidden;
		margin-bottom: 4px;
	}
	.ota-progress-bar {
		height: 100%;
		background: var(--vui-color-primary, #3b82f6);
		border-radius: 3px;
		transition: width 0.2s ease;
	}
	.ota-progress-label {
		text-align: right;
		font-size: var(--vui-text-xs);
		margin-bottom: var(--vui-space-sm);
	}
	.ota-warning {
		font-size: var(--vui-text-xs);
		color: var(--vui-text-muted);
		margin-top: var(--vui-space-md);
		padding-top: var(--vui-space-sm);
		border-top: 1px solid var(--vui-border);
	}
</style>
