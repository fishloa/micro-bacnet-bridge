<script lang="ts">
	import { onMount, onDestroy } from 'svelte';
	import { api } from '$lib/api';
	import type { NetworkConfig, BacnetConfig, NtpConfig, SyslogConfig, MqttConfig, SnmpConfig } from '$lib/api';
	import { exposureConfig } from '$lib/stores';

	function updateExposure() {
		$exposureConfig = {
			bacnetIpEnabled: bacnet.bacnetIpEnabled,
			mqttEnabled: mqtt.enabled,
			apiEnabled: true,
		};
	}

	let net: NetworkConfig = $state({
		dhcp: true, ip: '', subnet: '', gateway: '', dns: '', hostname: ''
	});
	let bacnet: BacnetConfig = $state({
		deviceId: 0, deviceName: '', vendor: '', mstpMac: 0, mstpBaud: 9600, maxMaster: 127, bacnetIpEnabled: true
	});
	let ntp: NtpConfig = $state({
		enabled: true, use_dhcp_servers: true, servers: ['pool.ntp.org', '', ''], sync_interval_secs: 3600
	});
	let syslog: SyslogConfig = $state({
		enabled: false, server: '', port: 514
	});
	let mqtt: MqttConfig = $state({
		enabled: false, broker: '', port: 1883, client_id: 'bacnet-bridge',
		username: '', password: '', topic_prefix: 'bacnet',
		ha_discovery_enabled: false, ha_discovery_prefix: 'homeassistant',
		publish_points: [], tls_enabled: false
	});
	let snmp: SnmpConfig = $state({
		enabled: false, community: 'public'
	});

	let savedMsg = $state('');
	let loaded = $state(false);

	function showMsg(msg: string) {
		savedMsg = msg;
		setTimeout(() => savedMsg = '', 3000);
	}

	/** Generic config card controller: tracks dirty state, saving flag, save action. */
	function createCard<T>(getData: () => T, saveFn: (data: T) => Promise<unknown>, label: string, afterSave?: () => void) {
		let snapshot = $state('');
		let saving = $state(false);

		return {
			get saving() { return saving; },
			get dirty() { return loaded && JSON.stringify(getData()) !== snapshot; },
			get disabled() { return saving || !loaded || !this.dirty; },
			snap() { snapshot = JSON.stringify(getData()); },
			async save() {
				saving = true;
				try {
					await saveFn(getData());
					snapshot = JSON.stringify(getData());
					afterSave?.();
					showMsg(`${label} saved`);
				} catch {
					showMsg(`Failed to save ${label}`);
				} finally {
					saving = false;
				}
			},
		};
	}

	const cards = {
		net: createCard(() => net, d => api.setNetworkConfig(d), 'Network'),
		bacnet: createCard(() => bacnet, d => api.setBacnetConfig(d), 'BACnet', updateExposure),
		ntp: createCard(
			() => ntp,
			d => api.setNtpConfig({ ...d, servers: d.servers.filter(s => s.trim() !== '') }),
			'NTP',
		),
		syslog: createCard(() => syslog, d => api.setSyslogConfig(d), 'Syslog'),
		mqtt: createCard(() => mqtt, d => api.setMqttConfig(d), 'MQTT', updateExposure),
		snmp: createCard(() => snmp, d => api.setSnmpConfig(d), 'SNMP'),
	};

	onMount(async () => {
		[net, bacnet, ntp, syslog, mqtt, snmp] = await Promise.all([
			api.getNetworkConfig(),
			api.getBacnetConfig(),
			api.getNtpConfig(),
			api.getSyslogConfig(),
			api.getMqttConfig(),
			api.getSnmpConfig(),
		]);
		while (ntp.servers.length < 3) ntp.servers = [...ntp.servers, ''];
		loaded = true;
		Object.values(cards).forEach(c => c.snap());
		updateExposure();
	});
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

	<!-- SYSTEM SECTION -->
	<div class="section-row">
		<div class="section-sidebar">
			<span class="section-sidebar-label">System</span>
		</div>
		<div class="section-grid-3">
		<!-- Network Card -->
		<div class="vui-card vui-animate-fade-in">
			<div class="card-title-row">
				<div class="vui-section-header">Network</div>
				<button class="vui-btn vui-btn-primary" onclick={cards.net.save} disabled={cards.net.disabled}>
					{cards.net.saving ? 'Saving...' : 'Save'}
				</button>
			</div>
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
		</div>

		<!-- BACnet / MS/TP Card -->
		<div class="vui-card vui-animate-fade-in">
			<div class="card-title-row">
				<div class="vui-section-header">BACnet / MS/TP</div>
				<button class="vui-btn vui-btn-primary" onclick={cards.bacnet.save} disabled={cards.bacnet.disabled}>
					{cards.bacnet.saving ? 'Saving...' : 'Save'}
				</button>
			</div>
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
		</div>

		<!-- NTP / Time Sync Card -->
		<div class="vui-card vui-animate-fade-in">
			<div class="card-title-row">
				<div class="vui-section-header">NTP / Time Sync</div>
				<button class="vui-btn vui-btn-primary" onclick={cards.ntp.save} disabled={cards.ntp.disabled}>
					{cards.ntp.saving ? 'Saving...' : 'Save'}
				</button>
			</div>
			<table class="form-table">
				<tbody>
					<tr>
						<td class="field-label">Enabled</td>
						<td><input type="checkbox" bind:checked={ntp.enabled} style="width:18px;height:18px;accent-color:var(--vui-accent)" /></td>
					</tr>
					{#if ntp.enabled}
						<tr>
							<td class="field-label">Use DHCP Servers</td>
							<td><input type="checkbox" bind:checked={ntp.use_dhcp_servers} style="width:18px;height:18px;accent-color:var(--vui-accent)" /></td>
						</tr>
						{#if !ntp.use_dhcp_servers}
							<tr>
								<td class="field-label">Server 1</td>
								<td><input class="vui-input mono" bind:value={ntp.servers[0]} placeholder="pool.ntp.org" /></td>
							</tr>
							<tr>
								<td class="field-label">Server 2</td>
								<td><input class="vui-input mono" bind:value={ntp.servers[1]} placeholder="time.cloudflare.com" /></td>
							</tr>
							<tr>
								<td class="field-label">Server 3</td>
								<td><input class="vui-input mono" bind:value={ntp.servers[2]} placeholder="time.google.com" /></td>
							</tr>
						{/if}
						<tr>
							<td class="field-label">Sync Interval (s)</td>
							<td><input class="vui-input mono" type="number" min="60" bind:value={ntp.sync_interval_secs} /></td>
						</tr>
					{/if}
				</tbody>
			</table>
		</div>
		</div>
	</div>

	<!-- EXPOSURES SECTION -->
	<div class="section-row">
		<div class="section-sidebar">
			<span class="section-sidebar-label">Exposures</span>
		</div>
		<div class="section-grid-2">
		<!-- BACnet/IP Card -->
		<div class="vui-card vui-animate-fade-in">
			<div class="card-title-row">
				<div class="vui-section-header">BACnet/IP</div>
				<button class="vui-btn vui-btn-primary" onclick={cards.bacnet.save} disabled={cards.bacnet.disabled}>
					{cards.bacnet.saving ? 'Saving...' : 'Save'}
				</button>
			</div>
			<table class="form-table">
				<tbody>
					<tr>
						<td class="field-label">Enabled</td>
						<td><input type="checkbox" bind:checked={bacnet.bacnetIpEnabled} style="width:18px;height:18px;accent-color:var(--vui-accent)" /></td>
					</tr>
				</tbody>
			</table>
		</div>

		<!-- MQTT Card -->
		<div class="vui-card vui-animate-fade-in">
			<div class="card-title-row">
				<div class="vui-section-header">MQTT</div>
				<button class="vui-btn vui-btn-primary" onclick={cards.mqtt.save} disabled={cards.mqtt.disabled}>
					{cards.mqtt.saving ? 'Saving...' : 'Save'}
				</button>
			</div>
			<div class="mqtt-config">
				<table class="form-table">
					<tbody>
						<tr>
							<td class="field-label">Enabled</td>
							<td><input type="checkbox" bind:checked={mqtt.enabled} style="width:18px;height:18px;accent-color:var(--vui-accent)" /></td>
						</tr>
						{#if mqtt.enabled}
							<tr>
								<td class="field-label">Broker</td>
								<td><input class="vui-input mono" bind:value={mqtt.broker} placeholder="mqtt.example.com" /></td>
							</tr>
							<tr>
								<td class="field-label">Port</td>
								<td><input class="vui-input mono" type="number" min="1" max="65535" bind:value={mqtt.port} /></td>
							</tr>
							<tr>
								<td class="field-label">TLS (port 8883)</td>
								<td><input type="checkbox" bind:checked={mqtt.tls_enabled} style="width:18px;height:18px;accent-color:var(--vui-accent)" /></td>
							</tr>
							<tr>
								<td class="field-label">Client ID</td>
								<td><input class="vui-input mono" bind:value={mqtt.client_id} placeholder="bacnet-bridge" /></td>
							</tr>
							<tr>
								<td class="field-label">Username</td>
								<td><input class="vui-input" bind:value={mqtt.username} placeholder="(optional)" /></td>
							</tr>
							<tr>
								<td class="field-label">Password</td>
								<td><input class="vui-input" type="password" bind:value={mqtt.password} placeholder="(optional)" /></td>
							</tr>
							<tr>
								<td class="field-label">Topic Prefix</td>
								<td><input class="vui-input mono" bind:value={mqtt.topic_prefix} placeholder="bacnet" /></td>
							</tr>
							<tr>
								<td class="field-label">HA Auto-Discovery</td>
								<td><input type="checkbox" bind:checked={mqtt.ha_discovery_enabled} style="width:18px;height:18px;accent-color:var(--vui-accent)" /></td>
							</tr>
							{#if mqtt.ha_discovery_enabled}
								<tr>
									<td class="field-label">HA Discovery Prefix</td>
									<td><input class="vui-input mono" bind:value={mqtt.ha_discovery_prefix} placeholder="homeassistant" /></td>
								</tr>
							{/if}
						{/if}
					</tbody>
				</table>
				{#if mqtt.enabled}
					<div class="mqtt-points-info">
						<p style="font-size: var(--vui-text-xs); color: var(--vui-text-sub); line-height: 1.6;">
							Per-point MQTT publish control has moved to the
							<a href="/points" style="color: var(--vui-accent); text-decoration: none; font-weight: var(--vui-font-medium);">Points Config</a>
							page, where you can also set scale, offset, and engineering units for each point.
						</p>
					</div>
				{/if}
			</div>
		</div>
		</div>
	</div>

	<!-- OPERATIONS SECTION -->
	<div class="section-row">
		<div class="section-sidebar">
			<span class="section-sidebar-label">Operations</span>
		</div>
		<div class="section-grid-2">
		<!-- Syslog Card -->
		<div class="vui-card vui-animate-fade-in">
			<div class="card-title-row">
				<div class="vui-section-header">Syslog</div>
				<button class="vui-btn vui-btn-primary" onclick={cards.syslog.save} disabled={cards.syslog.disabled}>
					{cards.syslog.saving ? 'Saving...' : 'Save'}
				</button>
			</div>
			<table class="form-table">
				<tbody>
					<tr>
						<td class="field-label">Enabled</td>
						<td><input type="checkbox" bind:checked={syslog.enabled} style="width:18px;height:18px;accent-color:var(--vui-accent)" /></td>
					</tr>
					{#if syslog.enabled}
						<tr>
							<td class="field-label">Server</td>
							<td><input class="vui-input mono" bind:value={syslog.server} placeholder="syslog.example.com" /></td>
						</tr>
						<tr>
							<td class="field-label">Port</td>
							<td><input class="vui-input mono" type="number" min="1" max="65535" bind:value={syslog.port} /></td>
						</tr>
					{/if}
				</tbody>
			</table>
		</div>

		<!-- SNMP Card -->
		<div class="vui-card vui-animate-fade-in">
			<div class="card-title-row">
				<div class="vui-section-header">SNMP</div>
				<button class="vui-btn vui-btn-primary" onclick={cards.snmp.save} disabled={cards.snmp.disabled}>
					{cards.snmp.saving ? 'Saving...' : 'Save'}
				</button>
			</div>
			<table class="form-table">
				<tbody>
					<tr>
						<td class="field-label">Enabled</td>
						<td><input type="checkbox" bind:checked={snmp.enabled} style="width:18px;height:18px;accent-color:var(--vui-accent)" /></td>
					</tr>
					{#if snmp.enabled}
						<tr>
							<td class="field-label">Community String</td>
							<td><input class="vui-input mono" bind:value={snmp.community} placeholder="public" /></td>
						</tr>
					{/if}
				</tbody>
			</table>
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

	.section-row {
		display: flex;
		margin-bottom: var(--vui-space-lg);
	}
	.section-sidebar {
		display: flex;
		align-items: center;
		justify-content: center;
		width: 36px;
		min-width: 36px;
		border-right: 3px solid var(--vui-accent);
		margin-right: var(--vui-space-md);
	}
	.section-sidebar-label {
		writing-mode: vertical-rl;
		text-orientation: mixed;
		transform: rotate(180deg);
		font-size: var(--vui-text-sm);
		font-weight: var(--vui-font-bold);
		color: var(--vui-accent);
		text-transform: uppercase;
		letter-spacing: 0.1em;
		white-space: nowrap;
	}
	.section-grid-3 {
		display: grid;
		grid-template-columns: repeat(3, 1fr);
		gap: var(--vui-space-lg);
		flex: 1;
	}
	.section-grid-2 {
		display: grid;
		grid-template-columns: repeat(2, 1fr);
		gap: var(--vui-space-lg);
		flex: 1;
		max-width: 66%;
	}
	@media (max-width: 1000px) {
		.section-grid-3,
		.section-grid-2 {
			grid-template-columns: 1fr;
			max-width: 100%;
		}
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
		font-weight: var(--vui-font-medium);
		white-space: nowrap;
		padding-right: var(--vui-space-md);
	}

	.form-table .vui-input {
		width: 100%;
	}

	.card-title-row {
		display: flex;
		align-items: center;
		justify-content: space-between;
	}

	.mqtt-config {
		display: flex;
		flex-direction: column;
	}

	.mqtt-points-info {
		margin-top: var(--vui-space-md);
		padding-top: var(--vui-space-md);
		border-top: 1px solid var(--vui-border);
	}
</style>
