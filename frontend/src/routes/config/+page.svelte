<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { NetworkConfig, BacnetConfig } from '$lib/api';

	let net: NetworkConfig = $state({
		dhcp: true, ip: '', subnet: '', gateway: '', dns: '', hostname: ''
	});
	let bacnet: BacnetConfig = $state({
		deviceId: 0, deviceName: '', vendor: '', mstpMac: 0, mstpBaud: 9600, maxMaster: 127
	});
	let savingNetwork = $state(false);
	let savingBacnet = $state(false);
	let savedMsg = $state('');
	let loaded = $state(false);

	onMount(async () => {
		net = await api.getNetworkConfig();
		bacnet = await api.getBacnetConfig();
		loaded = true;
	});

	async function saveNetwork() {
		savingNetwork = true;
		try {
			await api.setNetworkConfig(net);
			savedMsg = 'Network config saved';
		} catch (e) {
			savedMsg = 'Failed to save network config';
		} finally {
			savingNetwork = false;
			setTimeout(() => savedMsg = '', 3000);
		}
	}

	async function saveBacnet() {
		savingBacnet = true;
		try {
			await api.setBacnetConfig(bacnet);
			savedMsg = 'BACnet config saved';
		} catch (e) {
			savedMsg = 'Failed to save BACnet config';
		} finally {
			savingBacnet = false;
			setTimeout(() => savedMsg = '', 3000);
		}
	}
</script>

<svelte:head>
	<title>Config — BACnet Bridge</title>
</svelte:head>

<div class="config-page">
	<div class="page-header">
		<h1>Configuration</h1>
		{#if savedMsg}
			<div class="vui-alert vui-alert-success vui-animate-fade-in" style="padding: 8px 16px; font-size: var(--vui-text-sm);">
				{savedMsg}
			</div>
		{/if}
	</div>

	<div class="config-grid">
		<div class="vui-card vui-animate-fade-in">
			<div class="vui-section-header">Network</div>
			<table class="form-table">
				<tbody>
					<tr>
						<td class="field-label">DHCP</td>
						<td><input type="checkbox" bind:checked={net.dhcp} style="width:18px;height:18px;accent-color:var(--vui-accent)" /></td>
					</tr>
					<tr>
						<td class="field-label">Hostname</td>
						<td><input class="vui-input" bind:value={net.hostname} placeholder="bacnet-bridge" /></td>
					</tr>
					{#if !net.dhcp}
						<tr>
							<td class="field-label">IP Address</td>
							<td><input class="vui-input mono" bind:value={net.ip} placeholder="192.168.1.100" /></td>
						</tr>
						<tr>
							<td class="field-label">Subnet Mask</td>
							<td><input class="vui-input mono" bind:value={net.subnet} placeholder="255.255.255.0" /></td>
						</tr>
						<tr>
							<td class="field-label">Gateway</td>
							<td><input class="vui-input mono" bind:value={net.gateway} placeholder="192.168.1.1" /></td>
						</tr>
						<tr>
							<td class="field-label">DNS</td>
							<td><input class="vui-input mono" bind:value={net.dns} placeholder="192.168.1.1" /></td>
						</tr>
					{/if}
				</tbody>
			</table>
			<div class="card-actions">
				<button class="vui-btn vui-btn-primary" onclick={saveNetwork} disabled={savingNetwork || !loaded}>
					{savingNetwork ? 'Saving...' : 'Save Network'}
				</button>
			</div>
		</div>

		<div class="vui-card vui-animate-fade-in">
			<div class="vui-section-header">BACnet / MS/TP</div>
			<table class="form-table">
				<tbody>
					<tr>
						<td class="field-label">Device ID</td>
						<td><input class="vui-input mono" type="number" bind:value={bacnet.deviceId} /></td>
					</tr>
					<tr>
						<td class="field-label">Device Name</td>
						<td><input class="vui-input" bind:value={bacnet.deviceName} /></td>
					</tr>
					<tr>
						<td class="field-label">Vendor</td>
						<td><input class="vui-input" bind:value={bacnet.vendor} disabled /></td>
					</tr>
					<tr>
						<td class="field-label">MS/TP MAC (0–127)</td>
						<td><input class="vui-input mono" type="number" min="0" max="127" bind:value={bacnet.mstpMac} /></td>
					</tr>
					<tr>
						<td class="field-label">MS/TP Baud Rate</td>
						<td>
							<select class="vui-input" bind:value={bacnet.mstpBaud}>
								<option value={9600}>9600</option>
								<option value={19200}>19200</option>
								<option value={38400}>38400</option>
								<option value={76800}>76800</option>
							</select>
						</td>
					</tr>
					<tr>
						<td class="field-label">Max Master</td>
						<td><input class="vui-input mono" type="number" min="1" max="127" bind:value={bacnet.maxMaster} /></td>
					</tr>
				</tbody>
			</table>
			<div class="card-actions">
				<button class="vui-btn vui-btn-primary" onclick={saveBacnet} disabled={savingBacnet || !loaded}>
					{savingBacnet ? 'Saving...' : 'Save BACnet'}
				</button>
			</div>
		</div>
	</div>
</div>

<style>
	.config-page {
		padding: var(--vui-space-lg);
		height: 100%;
		overflow-y: auto;
	}
	.config-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(380px, 1fr));
		gap: var(--vui-space-lg);
	}
	.form-table {
		width: 100%;
		border-collapse: collapse;
		margin: var(--vui-space-md) 0;
	}
	.form-table td {
		padding: var(--vui-space-sm) 0;
		vertical-align: middle;
		text-align: left;
	}
	.form-table .field-label {
		width: 160px;
		font-size: var(--vui-text-sm);
		color: var(--vui-text-sub);
		font-weight: var(--vui-font-medium);
		white-space: nowrap;
		padding-right: var(--vui-space-md);
	}
	.form-table .vui-input {
		width: 100%;
	}
	.card-actions {
		padding-top: var(--vui-space-md);
		border-top: 1px solid var(--vui-border);
		margin-top: var(--vui-space-sm);
	}
</style>
