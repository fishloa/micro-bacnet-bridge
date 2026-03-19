<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { SystemStatus, OtaConfig } from '$lib/api';

	let status: SystemStatus | null = $state(null);
	let otaConfig: OtaConfig | null = $state(null);
	let loading = $state(true);
	let error = $state('');
	let success = $state('');

	// OTA check state
	let checking = $state(false);
	let updateAvailable: { version: string; releaseNotes: string } | null = $state(null);

	// Manual upload state
	let otaFile: File | null = $state(null);
	let otaProgress = $state(0);
	let otaStatus: 'idle' | 'uploading' | 'success' | 'error' = $state('idle');
	let otaMessage = $state('');

	onMount(async () => {
		try {
			[status, otaConfig] = await Promise.all([api.getStatus(), api.getOtaConfig()]);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load firmware info.';
		} finally {
			loading = false;
		}
	});

	async function saveOtaConfig() {
		if (!otaConfig) return;
		try {
			await api.setOtaConfig(otaConfig);
			success = 'OTA settings saved.';
			setTimeout(() => { success = ''; }, 3000);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to save OTA config.';
		}
	}

	async function checkForUpdate() {
		checking = true;
		error = '';
		updateAvailable = null;
		try {
			const result = await api.checkOtaUpdate();
			if (result.available) {
				updateAvailable = { version: result.version ?? '?', releaseNotes: result.releaseNotes ?? '' };
			} else {
				success = 'Firmware is up to date.';
				setTimeout(() => { success = ''; }, 3000);
			}
		} catch (e) {
			error = e instanceof Error ? e.message : 'Update check failed.';
		} finally {
			checking = false;
		}
	}

	function onFileChange(e: Event) {
		otaFile = (e.target as HTMLInputElement).files?.[0] ?? null;
		otaStatus = 'idle';
		otaMessage = '';
		otaProgress = 0;
	}

	async function uploadFirmware() {
		if (!otaFile) return;
		const MAX_SIZE = 1_500_000;
		if (otaFile.size > MAX_SIZE) {
			otaStatus = 'error';
			otaMessage = `File is too large (${(otaFile.size / 1024).toFixed(0)} KB). Maximum: ${(MAX_SIZE / 1024).toFixed(0)} KB.`;
			return;
		}
		if (!confirm('This will overwrite the running firmware and reboot the device. Continue?')) return;

		otaStatus = 'uploading';
		otaProgress = 0;
		otaMessage = '';

		try {
			await new Promise<void>((resolve, reject) => {
				const xhr = new XMLHttpRequest();
				xhr.open('POST', '/api/v1/system/firmware');
				xhr.setRequestHeader('Content-Type', 'application/octet-stream');
				const token = localStorage.getItem('auth_token');
				if (token) xhr.setRequestHeader('Authorization', `Bearer ${token}`);

				xhr.upload.addEventListener('progress', (e) => {
					if (e.lengthComputable) otaProgress = Math.round((e.loaded / e.total) * 100);
				});
				xhr.addEventListener('load', () => {
					if (xhr.status === 200) { otaProgress = 100; resolve(); }
					else reject(new Error(`Server returned ${xhr.status}: ${xhr.responseText}`));
				});
				xhr.addEventListener('error', () => reject(new Error('Network error during upload')));
				xhr.timeout = 120_000;
				xhr.send(otaFile);
			});

			otaStatus = 'success';
			otaMessage = 'Firmware uploaded. Device is rebooting — this page will be available again in 10–30 seconds.';
		} catch (err: unknown) {
			otaStatus = 'error';
			otaMessage = err instanceof Error ? err.message : 'Unknown upload error';
		}
	}
</script>

<svelte:head>
	<title>Firmware — BACnet Bridge</title>
</svelte:head>

<div class="firmware-page">
	<h1 class="vui-page-title">Firmware</h1>

	{#if error}
		<div class="vui-alert vui-alert-danger">{error}</div>
	{/if}
	{#if success}
		<div class="vui-alert vui-alert-success">{success}</div>
	{/if}

	<!-- Current version -->
	{#if loading}
		<div class="vui-skeleton skeleton-sm"></div>
	{:else if status}
		<div class="vui-card vui-animate-fade-in">
			<div class="vui-section-header">Current Version</div>
			<div class="version-display">
				<span class="version-num">v{status.firmwareVersion}</span>
				<span class="vui-badge vui-badge-success">Running</span>
			</div>
		</div>
	{/if}

	<!-- OTA settings -->
	{#if otaConfig}
		<div class="vui-card vui-animate-fade-in">
			<div class="vui-section-header">Automatic Updates</div>
			<form onsubmit={(e) => { e.preventDefault(); saveOtaConfig(); }}>
				<div class="form-row">
					<label class="toggle-label">
						<input type="checkbox" bind:checked={otaConfig.auto_update} />
						<span>Enable automatic updates</span>
					</label>
				</div>
				<div class="vui-input-group">
					<label for="ota-channel">Update Channel</label>
					<select id="ota-channel" class="vui-input" bind:value={otaConfig.channel}>
						<option value="release">Release (stable)</option>
						<option value="beta">Beta</option>
					</select>
				</div>
				<div class="vui-input-group">
					<label for="manifest-url">Manifest URL</label>
					<input
						id="manifest-url"
						class="vui-input"
						type="url"
						bind:value={otaConfig.manifest_url}
						placeholder="https://example.com/firmware/manifest.json"
					/>
				</div>
				<div class="form-actions">
					<button type="submit" class="vui-btn vui-btn-primary">Save</button>
					<button type="button" class="vui-btn vui-btn-secondary" onclick={checkForUpdate} disabled={checking}>
						{checking ? 'Checking…' : 'Check Now'}
					</button>
				</div>
			</form>

			{#if updateAvailable}
				<div class="vui-alert vui-alert-info mt-md">
					<strong>Update available: v{updateAvailable.version}</strong>
					{#if updateAvailable.releaseNotes}
						<p class="release-notes">{updateAvailable.releaseNotes}</p>
					{/if}
				</div>
			{/if}
		</div>
	{/if}

	<!-- Manual upload -->
	<div class="vui-card">
		<div class="vui-section-header">Manual Upload</div>
		<p class="text-sub card-description">
			Upload a <code>.bin</code> or <code>.uf2</code> firmware file directly.
			The device will reboot automatically after the upload completes.
		</p>

		{#if otaStatus === 'success'}
			<div class="vui-alert vui-alert-success">{otaMessage}</div>
		{:else if otaStatus === 'error'}
			<div class="vui-alert vui-alert-danger">{otaMessage}</div>
		{/if}

		<div class="ota-row">
			<label class="vui-btn vui-btn-secondary ota-file-label">
				{otaFile ? otaFile.name : 'Choose firmware (.bin or .uf2)'}
				<input type="file" accept=".bin,.uf2" class="ota-file-input" onchange={onFileChange} disabled={otaStatus === 'uploading'} />
			</label>
			<button class="vui-btn vui-btn-primary" onclick={uploadFirmware} disabled={!otaFile || otaStatus === 'uploading'}>
				{otaStatus === 'uploading' ? 'Uploading…' : 'Upload & Flash'}
			</button>
		</div>

		{#if otaStatus === 'uploading'}
			<div class="vui-progress-wrap">
				<div class="vui-progress-bar" style="width: {otaProgress}%;"></div>
			</div>
			<p class="vui-progress-label">{otaProgress}%</p>
		{/if}

		<div class="vui-alert vui-alert-danger">
			&#9888; Warning: do not power off the device during a firmware update.
		</div>
	</div>
</div>

<style>
	.firmware-page {
		padding: var(--vui-space-lg);
		height: 100%;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: var(--vui-space-lg);
	}

	.skeleton-sm {
		height: 80px;
		border-radius: var(--vui-radius-md);
	}

	.card-description {
		margin-bottom: var(--vui-space-md);
		font-size: var(--vui-text-sm);
	}

	.vui-input-group select {
		max-width: 300px;
	}

	.release-notes {
		margin-top: 4px;
		font-size: var(--vui-text-sm);
	}

	.version-display {
		display: flex;
		align-items: center;
		gap: var(--vui-space-md);
		padding: var(--vui-space-sm) 0;
	}

	.version-num {
		font-size: var(--vui-text-xl);
		font-weight: var(--vui-font-bold);
	}

	.form-row {
		margin-bottom: var(--vui-space-md);
	}

	.toggle-label {
		display: flex;
		align-items: center;
		gap: var(--vui-space-sm);
		font-size: var(--vui-text-sm);
		cursor: pointer;
	}

	.form-actions {
		display: flex;
		gap: var(--vui-space-sm);
		margin-top: var(--vui-space-md);
	}

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



</style>
