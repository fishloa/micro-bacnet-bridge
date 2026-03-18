<script lang="ts">
	import { onMount } from 'svelte';
	import {
		api,
		OBJECT_TYPE_INFO,
		ENGINEERING_UNITS,
		isNumericType,
		pointKey,
	} from '$lib/api';
	import type { BacnetPoint, BacnetDevice, PointConfig } from '$lib/api';

	// --- State ---
	let devices: BacnetDevice[] = $state([]);
	let allPoints: BacnetPoint[] = $state([]);
	let configs: Map<string, PointConfig> = $state(new Map());
	let dirty: Set<string> = $state(new Set());
	let filterText: string = $state('');
	let loading: boolean = $state(true);
	let saveStatus: 'idle' | 'saving' | 'success' | 'error' = $state('idle');
	let saveMessage: string = $state('');

	// --- Derived: merged rows ---
	type Row = {
		point: BacnetPoint;
		config: PointConfig;
		key: string;
	};

	let filteredRows: Row[] = $derived.by(() => {
		let rows: Row[] = allPoints.map(p => {
			const key = pointKey(p);
			const cfg = configs.get(key) ?? defaultConfig(p);
			return { point: p, config: cfg, key };
		});

		if (filterText) {
			try {
				const re = new RegExp(filterText, 'i');
				rows = rows.filter(r =>
					re.test(r.point.objectName) ||
					re.test(r.point.description) ||
					re.test(r.point.objectType)
				);
			} catch {
				const lower = filterText.toLowerCase();
				rows = rows.filter(r =>
					r.point.objectName.toLowerCase().includes(lower) ||
					r.point.description.toLowerCase().includes(lower)
				);
			}
		}

		return rows;
	});

	function defaultConfig(p: BacnetPoint): PointConfig {
		return {
			objectType: p.objectType,
			objectInstance: p.objectInstance,
			scale: 1.0,
			offset: 0.0,
			engineeringUnit: 95,
			bridgeToBacnetIp: true,
			bridgeToMqtt: true,
		};
	}

	// --- Load data ---
	onMount(async () => {
		loading = true;
		try {
			const devList = await api.getDevices();
			devices = devList;
			// Load points from first online device for the config page
			// (config page shows ALL points across ALL devices — or just device 100 for now)
			const targetDevice = devList.find(d => d.online && d.id === 100) ?? devList.find(d => d.online);
			if (targetDevice) {
				allPoints = await api.getPoints(targetDevice.id);
			}
			const cfgList = await api.getPointConfigs();
			const map = new Map<string, PointConfig>();
			for (const cfg of cfgList) {
				map.set(`${cfg.objectType}:${cfg.objectInstance}`, cfg);
			}
			configs = map;
		} finally {
			loading = false;
		}
	});

	// --- Helpers ---
	function badgeClass(objectType: string): string {
		const info = OBJECT_TYPE_INFO[objectType];
		if (!info) return 'vui-badge';
		return `vui-badge vui-badge-${info.color}`;
	}

	function badgeLabel(objectType: string): string {
		return OBJECT_TYPE_INFO[objectType]?.label ?? objectType;
	}

	function unitLabel(code: number): string {
		return ENGINEERING_UNITS.find(u => u.code === code)?.label ?? String(code);
	}

	function convertedValue(p: BacnetPoint, cfg: PointConfig): string {
		if (!isNumericType(p.objectType)) {
			if (typeof p.presentValue === 'boolean') return p.presentValue ? 'Active' : 'Inactive';
			return String(p.presentValue);
		}
		const raw = typeof p.presentValue === 'number' ? p.presentValue : parseFloat(String(p.presentValue));
		if (isNaN(raw)) return String(p.presentValue);
		const converted = raw * cfg.scale + cfg.offset;
		const label = unitLabel(cfg.engineeringUnit);
		if (label === 'No Units') return converted.toFixed(3).replace(/\.?0+$/, '') || '0';
		return `${converted.toFixed(3).replace(/\.?0+$/, '') || '0'} ${label}`;
	}

	// --- Mutation helpers ---
	function getOrDefault(p: BacnetPoint): PointConfig {
		const key = pointKey(p);
		return configs.get(key) ?? defaultConfig(p);
	}

	function updateConfig(p: BacnetPoint, patch: Partial<PointConfig>) {
		const key = pointKey(p);
		const existing = configs.get(key) ?? defaultConfig(p);
		const updated = { ...existing, ...patch };
		const newMap = new Map(configs);
		newMap.set(key, updated);
		configs = newMap;
		const newDirty = new Set(dirty);
		newDirty.add(key);
		dirty = newDirty;
	}

	function onScaleInput(p: BacnetPoint, e: Event) {
		const val = parseFloat((e.target as HTMLInputElement).value);
		if (!isNaN(val)) updateConfig(p, { scale: val });
	}

	function onOffsetInput(p: BacnetPoint, e: Event) {
		const val = parseFloat((e.target as HTMLInputElement).value);
		if (!isNaN(val)) updateConfig(p, { offset: val });
	}

	function onUnitChange(p: BacnetPoint, e: Event) {
		const code = parseInt((e.target as HTMLSelectElement).value, 10);
		updateConfig(p, { engineeringUnit: code });
	}

	function onBacnetIpChange(p: BacnetPoint, e: Event) {
		updateConfig(p, { bridgeToBacnetIp: (e.target as HTMLInputElement).checked });
	}

	function onMqttChange(p: BacnetPoint, e: Event) {
		updateConfig(p, { bridgeToMqtt: (e.target as HTMLInputElement).checked });
	}

	// --- Save ---
	async function saveAll() {
		if (dirty.size === 0) return;
		saveStatus = 'saving';
		saveMessage = '';
		try {
			const dirtyConfigs: PointConfig[] = [];
			for (const key of dirty) {
				const cfg = configs.get(key);
				if (cfg) dirtyConfigs.push(cfg);
			}
			// Save each dirty config individually
			for (const cfg of dirtyConfigs) {
				await api.setPointConfig(cfg.objectType, cfg.objectInstance, cfg);
			}
			dirty = new Set();
			saveStatus = 'success';
			saveMessage = `Saved ${dirtyConfigs.length} point config${dirtyConfigs.length !== 1 ? 's' : ''}.`;
		} catch (err) {
			saveStatus = 'error';
			saveMessage = `Save failed: ${err instanceof Error ? err.message : String(err)}`;
		}
		setTimeout(() => { saveStatus = 'idle'; saveMessage = ''; }, 3000);
	}
</script>

<div class="page-root">
	<div class="page-header">
		<div class="header-left">
			<h1 class="page-title">Points Configuration</h1>
			<span class="text-sm text-sub">
				{allPoints.length} point{allPoints.length !== 1 ? 's' : ''} · {filteredRows.length} shown
			</span>
		</div>
		<div class="header-right">
			{#if saveStatus === 'success'}
				<span class="vui-badge vui-badge-success save-msg">{saveMessage}</span>
			{:else if saveStatus === 'error'}
				<span class="vui-badge vui-badge-danger save-msg">{saveMessage}</span>
			{:else if dirty.size > 0}
				<span class="text-sm text-sub">{dirty.size} unsaved change{dirty.size !== 1 ? 's' : ''}</span>
			{/if}
			<input
				class="vui-input filter-input"
				type="text"
				placeholder="Filter points (regex)…"
				bind:value={filterText}
			/>
			<button
				class="vui-btn vui-btn-primary"
				onclick={saveAll}
				disabled={dirty.size === 0 || saveStatus === 'saving'}
			>
				{saveStatus === 'saving' ? 'Saving…' : 'Save All'}
			</button>
		</div>
	</div>

	{#if loading}
		<div class="loading-state">
			<span class="text-sub">Loading points…</span>
		</div>
	{:else if allPoints.length === 0}
		<div class="loading-state">
			<span class="text-sub">No points available. Ensure a device is online.</span>
		</div>
	{:else}
		<div class="table-wrap vui-card">
			<table>
				<thead>
					<tr>
						<th style="width: 52px">Type</th>
						<th>Name</th>
						<th style="width: 160px">Value</th>
						<th style="width: 80px">Scale</th>
						<th style="width: 80px">Offset</th>
						<th style="width: 110px">Unit</th>
						<th style="width: 70px; text-align: center">BACnet/IP</th>
						<th style="width: 55px; text-align: center">MQTT</th>
					</tr>
				</thead>
				<tbody>
					{#each filteredRows as row (row.key)}
						{@const numeric = isNumericType(row.point.objectType)}
						{@const isDirty = dirty.has(row.key)}
						<tr class:row-dirty={isDirty}>
							<td>
								<span class={badgeClass(row.point.objectType)}>{badgeLabel(row.point.objectType)}</span>
							</td>
							<td class="cell-name">
								<span class="point-name">{row.point.objectName}</span>
								{#if row.point.description}
									<span class="point-desc text-sub text-xs">{row.point.description}</span>
								{/if}
							</td>
							<td class="cell-value mono">
								{#if numeric && (row.config.scale !== 1 || row.config.offset !== 0)}
									<span class="raw-value text-muted">{row.point.presentValue}</span>
									<span class="converted-arrow text-muted">&rarr;</span>
									<span class="computed-value">{convertedValue(row.point, row.config)}</span>
								{:else}
									{convertedValue(row.point, row.config)}
								{/if}
							</td>
							<td class="cell-input">
								{#if numeric}
									<input
										class="vui-input compact-num"
										type="number"
										step="0.001"
										value={row.config.scale}
										oninput={(e) => onScaleInput(row.point, e)}
									/>
								{:else}
									<span class="text-sub text-sm">—</span>
								{/if}
							</td>
							<td class="cell-input">
								{#if numeric}
									<input
										class="vui-input compact-num"
										type="number"
										step="0.1"
										value={row.config.offset}
										oninput={(e) => onOffsetInput(row.point, e)}
									/>
								{:else}
									<span class="text-sub text-sm">—</span>
								{/if}
							</td>
							<td class="cell-input">
								{#if numeric}
									<select
										class="vui-input compact-select"
										value={row.config.engineeringUnit}
										onchange={(e) => onUnitChange(row.point, e)}
									>
										{#each ENGINEERING_UNITS as unit}
											<option value={unit.code}>{unit.label}</option>
										{/each}
									</select>
								{:else}
									<span class="text-sub text-sm">—</span>
								{/if}
							</td>
							<td style="text-align: center">
								<input
									type="checkbox"
									class="vui-checkbox"
									checked={row.config.bridgeToBacnetIp}
									onchange={(e) => onBacnetIpChange(row.point, e)}
								/>
							</td>
							<td style="text-align: center">
								<input
									type="checkbox"
									class="vui-checkbox"
									checked={row.config.bridgeToMqtt}
									onchange={(e) => onMqttChange(row.point, e)}
								/>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>

<style>
	.page-root {
		display: flex;
		flex-direction: column;
		height: 100%;
		overflow: hidden;
		padding: var(--vui-space-lg);
		gap: var(--vui-space-md);
		box-sizing: border-box;
	}

	.page-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--vui-space-md);
		flex-shrink: 0;
	}

	.header-left {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.page-title {
		font-size: var(--vui-text-xl);
		font-weight: var(--vui-font-semibold);
		margin: 0;
	}

	.header-right {
		display: flex;
		align-items: center;
		gap: var(--vui-space-sm);
	}

	.filter-input {
		width: 220px;
		font-size: var(--vui-text-sm);
		padding: 6px 12px;
	}

	.save-msg {
		font-size: var(--vui-text-xs);
	}

	.loading-state {
		display: flex;
		align-items: center;
		justify-content: center;
		flex: 1;
	}

	/* Table */
	.table-wrap {
		flex: 1;
		overflow-y: auto;
		overflow-x: auto;
		padding: 0;
		border-radius: var(--vui-radius);
	}

	table {
		width: 100%;
		border-collapse: collapse;
	}

	th {
		text-align: left;
		padding: var(--vui-space-sm) var(--vui-space-sm);
		font-size: var(--vui-text-xs);
		font-weight: var(--vui-font-semibold);
		color: var(--vui-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.05em;
		border-bottom: 1px solid var(--vui-border);
		position: sticky;
		top: 0;
		background: var(--vui-surface, var(--vui-bg));
		z-index: 1;
		white-space: nowrap;
	}

	td {
		padding: 6px var(--vui-space-sm);
		border-bottom: 1px solid var(--vui-border);
		font-size: var(--vui-text-sm);
		vertical-align: middle;
	}

	tr:hover td {
		background: var(--vui-surface-hover);
	}

	tr.row-dirty td {
		background: color-mix(in srgb, var(--vui-accent-dim, #1e40af22) 60%, transparent);
	}

	tr.row-dirty:hover td {
		background: color-mix(in srgb, var(--vui-accent-dim, #1e40af22) 80%, var(--vui-surface-hover));
	}

	.cell-name {
		display: flex;
		flex-direction: column;
		gap: 1px;
	}

	.point-name {
		font-weight: var(--vui-font-medium);
	}

	.point-desc {
		color: var(--vui-text-muted);
	}

	.text-xs {
		font-size: var(--vui-text-xs);
	}

	.cell-value {
		font-family: var(--vui-font-mono, monospace);
		font-size: var(--vui-text-sm);
		color: var(--vui-text);
	}
	.raw-value {
		font-size: var(--vui-text-xs);
	}
	.converted-arrow {
		font-size: var(--vui-text-xs);
		margin: 0 2px;
	}
	.computed-value {
		color: var(--vui-accent);
	}

	.cell-input {
		padding: 4px var(--vui-space-sm);
	}

	.compact-num {
		width: 70px;
		padding: 4px 6px;
		font-size: var(--vui-text-sm);
		font-family: var(--vui-font-mono, monospace);
		text-align: right;
	}

	.compact-select {
		width: 90px;
		padding: 4px 6px;
		font-size: var(--vui-text-sm);
	}

	.vui-checkbox {
		cursor: pointer;
		width: 15px;
		height: 15px;
		accent-color: var(--vui-accent);
	}

	.mono {
		font-family: var(--vui-font-mono, monospace);
	}
</style>
