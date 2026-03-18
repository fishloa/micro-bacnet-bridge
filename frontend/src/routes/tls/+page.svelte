<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { TlsStatus } from '$lib/api';

	let tlsStatus: TlsStatus | null = $state(null);
	let loading = $state(true);
	let error = $state('');

	// CSR generation
	let csrPem = $state('');
	let csrLoading = $state(false);

	// Certificate upload
	let certFile: File | null = $state(null);
	let certLoading = $state(false);

	// Key + Cert upload
	let keyFile: File | null = $state(null);
	let keyFile2: File | null = $state(null);
	let keyLoading = $state(false);

	// Self-signed
	let selfSignedLoading = $state(false);

	// Disable TLS
	let disableLoading = $state(false);

	let successMsg = $state('');

	onMount(async () => {
		try {
			tlsStatus = await api.getTlsStatus();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load TLS status.';
		} finally {
			loading = false;
		}
	});

	async function generateCsr() {
		csrLoading = true;
		error = '';
		try {
			const result = await api.tlsCsr();
			csrPem = result.csr;
		} catch (e) {
			error = e instanceof Error ? e.message : 'CSR generation failed.';
		} finally {
			csrLoading = false;
		}
	}

	async function uploadCert() {
		if (!certFile) return;
		certLoading = true;
		error = '';
		try {
			const text = await certFile.text();
			await api.tlsUploadCert(text);
			successMsg = 'Certificate uploaded successfully.';
			tlsStatus = await api.getTlsStatus();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Certificate upload failed.';
		} finally {
			certLoading = false;
		}
	}

	async function uploadKeyAndCert() {
		if (!keyFile || !keyFile2) return;
		keyLoading = true;
		error = '';
		try {
			const keyPem = await keyFile.text();
			const certPem = await keyFile2.text();
			await api.tlsUploadKey(keyPem);
			await api.tlsUploadCert(certPem);
			successMsg = 'Private key and certificate uploaded successfully.';
			tlsStatus = await api.getTlsStatus();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Upload failed.';
		} finally {
			keyLoading = false;
		}
	}

	async function generateSelfSigned() {
		selfSignedLoading = true;
		error = '';
		try {
			await api.tlsSelfSigned();
			successMsg = 'Self-signed certificate generated. TLS enabled.';
			tlsStatus = await api.getTlsStatus();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Self-signed generation failed.';
		} finally {
			selfSignedLoading = false;
		}
	}

	async function disableTls() {
		if (!confirm('This will disable HTTPS and remove the stored certificate. Continue?')) return;
		disableLoading = true;
		error = '';
		try {
			await api.tlsDisable();
			successMsg = 'TLS disabled.';
			tlsStatus = await api.getTlsStatus();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to disable TLS.';
		} finally {
			disableLoading = false;
		}
	}

	function copyCsr() {
		navigator.clipboard.writeText(csrPem).catch(() => {});
	}
</script>

<svelte:head>
	<title>TLS — BACnet Bridge</title>
</svelte:head>

<div class="tls-page">
	<div class="page-header">
		<h1>TLS / HTTPS</h1>
	</div>

	{#if error}
		<div class="vui-alert vui-alert-danger">{error}</div>
	{/if}
	{#if successMsg}
		<div class="vui-alert vui-alert-success">{successMsg}</div>
	{/if}

	<!-- TLS status card -->
	{#if loading}
		<div class="vui-skeleton" style="height: 80px; border-radius: var(--vui-radius-md);"></div>
	{:else if tlsStatus}
		<div class="vui-card vui-animate-fade-in">
			<div class="vui-section-header">Status</div>
			<div class="status-row">
				<span class="text-sub">HTTPS Server</span>
				<span class="vui-badge" class:vui-badge-success={tlsStatus.enabled} class:vui-badge-danger={!tlsStatus.enabled}>
					{tlsStatus.enabled ? 'Enabled' : 'Disabled'}
				</span>
			</div>
			{#if tlsStatus.enabled}
				<div class="status-row">
					<span class="text-sub">Certificate CN</span>
					<span class="mono">{tlsStatus.subject}</span>
				</div>
				<div class="status-row">
					<span class="text-sub">Expires</span>
					<span class="mono">{tlsStatus.expiry}</span>
				</div>
			{/if}
		</div>
	{/if}

	<!-- Generate CSR -->
	<div class="vui-card">
		<div class="vui-section-header">Generate CSR</div>
		<p class="text-sub" style="margin-bottom: var(--vui-space-md); font-size: var(--vui-text-sm);">
			Generate a Certificate Signing Request. Submit the CSR to your CA, then upload the signed
			certificate below.
		</p>
		<button class="vui-btn vui-btn-secondary" onclick={generateCsr} disabled={csrLoading}>
			{csrLoading ? 'Generating…' : 'Generate CSR'}
		</button>
		{#if csrPem}
			<div class="pem-block">
				<textarea class="pem-textarea" readonly value={csrPem} rows="8"></textarea>
				<button class="vui-btn vui-btn-sm vui-btn-ghost" onclick={copyCsr}>Copy</button>
			</div>
		{/if}
	</div>

	<!-- Upload Certificate -->
	<div class="vui-card">
		<div class="vui-section-header">Upload Certificate</div>
		<p class="text-sub" style="margin-bottom: var(--vui-space-md); font-size: var(--vui-text-sm);">
			Upload a PEM-encoded certificate signed by your CA (requires a private key already stored
			on device, e.g. from a CSR).
		</p>
		<div class="upload-row">
			<label class="vui-btn vui-btn-secondary file-label">
				{certFile ? certFile.name : 'Choose certificate (.pem/.crt)'}
				<input type="file" accept=".pem,.crt,.cer" class="file-input" onchange={(e) => certFile = (e.target as HTMLInputElement).files?.[0] ?? null} />
			</label>
			<button class="vui-btn vui-btn-primary" onclick={uploadCert} disabled={!certFile || certLoading}>
				{certLoading ? 'Uploading…' : 'Upload Certificate'}
			</button>
		</div>
	</div>

	<!-- Upload Key + Certificate -->
	<div class="vui-card">
		<div class="vui-section-header">Upload Private Key + Certificate</div>
		<p class="text-sub" style="margin-bottom: var(--vui-space-md); font-size: var(--vui-text-sm);">
			Upload both the private key and the certificate (e.g. issued by your own CA).
		</p>
		<div class="upload-grid">
			<div>
				<span class="upload-label">Private Key (.pem)</span>
				<label class="vui-btn vui-btn-secondary file-label">
					{keyFile ? keyFile.name : 'Choose key file'}
					<input type="file" accept=".pem,.key" class="file-input" onchange={(e) => keyFile = (e.target as HTMLInputElement).files?.[0] ?? null} />
				</label>
			</div>
			<div>
				<span class="upload-label">Certificate (.pem/.crt)</span>
				<label class="vui-btn vui-btn-secondary file-label">
					{keyFile2 ? keyFile2.name : 'Choose cert file'}
					<input type="file" accept=".pem,.crt,.cer" class="file-input" onchange={(e) => keyFile2 = (e.target as HTMLInputElement).files?.[0] ?? null} />
				</label>
			</div>
		</div>
		<button class="vui-btn vui-btn-primary" onclick={uploadKeyAndCert} disabled={!keyFile || !keyFile2 || keyLoading} style="margin-top: var(--vui-space-md);">
			{keyLoading ? 'Uploading…' : 'Upload Both'}
		</button>
	</div>

	<!-- Self-signed -->
	<div class="vui-card">
		<div class="vui-section-header">Generate Self-Signed Certificate</div>
		<p class="text-sub" style="margin-bottom: var(--vui-space-md); font-size: var(--vui-text-sm);">
			Generate a self-signed certificate using the device's current hostname. Browsers will
			show a warning, but connections will be encrypted.
		</p>
		<button class="vui-btn vui-btn-secondary" onclick={generateSelfSigned} disabled={selfSignedLoading}>
			{selfSignedLoading ? 'Generating…' : 'Generate Self-Signed'}
		</button>
	</div>

	<!-- Disable TLS -->
	{#if tlsStatus?.enabled}
		<div class="vui-card">
			<div class="vui-section-header">Disable TLS</div>
			<p class="text-sub" style="margin-bottom: var(--vui-space-md); font-size: var(--vui-text-sm);">
				Disable the HTTPS server and remove the stored certificate and private key.
				The device will only be accessible over HTTP.
			</p>
			<button class="vui-btn vui-btn-danger" onclick={disableTls} disabled={disableLoading}>
				{disableLoading ? 'Disabling…' : 'Disable TLS'}
			</button>
		</div>
	{/if}
</div>

<style>
	.tls-page {
		padding: var(--vui-space-lg);
		max-width: 700px;
		height: 100%;
		overflow-y: auto;
		display: flex;
		flex-direction: column;
		gap: var(--vui-space-lg);
	}

	h1 {
		font-size: var(--vui-text-xl);
		font-weight: var(--vui-font-bold);
	}

	.page-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	.status-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: var(--vui-space-sm) 0;
		border-bottom: 1px solid var(--vui-border);
		font-size: var(--vui-text-sm);
	}

	.pem-block {
		margin-top: var(--vui-space-md);
	}

	.pem-textarea {
		width: 100%;
		font-family: var(--vui-font-mono, monospace);
		font-size: 12px;
		background: var(--vui-surface);
		color: var(--vui-text);
		border: 1px solid var(--vui-border);
		border-radius: var(--vui-radius-sm);
		padding: var(--vui-space-sm);
		resize: vertical;
		box-sizing: border-box;
		margin-bottom: 4px;
	}

	.upload-row {
		display: flex;
		gap: var(--vui-space-sm);
		align-items: center;
		flex-wrap: wrap;
	}

	.upload-grid {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: var(--vui-space-md);
	}

	.upload-label {
		display: block;
		font-size: var(--vui-text-sm);
		color: var(--vui-text-sub);
		margin-bottom: 4px;
	}

	.file-label {
		cursor: pointer;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		max-width: 260px;
	}

	.file-input {
		display: none;
	}
</style>
