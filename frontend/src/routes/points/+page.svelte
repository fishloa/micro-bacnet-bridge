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
	import { exposureConfig } from '$lib/stores';

	// --- State ---
	let devices: BacnetDevice[] = $state([]);
	let allPoints: BacnetPoint[] = $state([]);
	let configs: Map<string, PointConfig> = $state(new Map());
	let dirty: Set<string> = $state(new Set());
	let filterText: string = $state('');
	let loading: boolean = $state(true);
	let saveStatus: 'idle' | 'saving' | 'success' | 'error' = $state('idle');
	let saveMessage: string = $state('');

	// --- Pagination ---
	const PAGE_SIZE_OPTIONS = [25, 50, 100, 0]; // 0 = All
	let pageSize: number = $state(50);
	let currentPage: number = $state(1);

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

	let totalPages: number = $derived(
		pageSize === 0 ? 1 : Math.max(1, Math.ceil(filteredRows.length / pageSize))
	);

	let pagedRows: Row[] = $derived.by(() => {
		if (pageSize === 0) return filteredRows;
		const start = (currentPage - 1) * pageSize;
		return filteredRows.slice(start, start + pageSize);
	});

	// Reset to page 1 when filter changes
	$effect(() => {
		// access filterText to establish the reactive dependency
		filterText;
		currentPage = 1;
	});

	// Clamp current page when total pages shrink
	$effect(() => {
		if (currentPage > totalPages) currentPage = totalPages;
	});

	function defaultConfig(p: BacnetPoint): PointConfig {
		// Default conversion unit: use discoveredUnit if available and > 0
		const engineeringUnit = (p.discoveredUnit > 0) ? p.discoveredUnit : 95;
		return {
			objectType: p.objectType,
			objectInstance: p.objectInstance,
			scale: 1.0,
			offset: 0.0,
			engineeringUnit,
			bridgeToBacnetIp: true,
			bridgeToMqtt: true,
			showOnDashboard: true,
			exposeInApi: true,
			stateText: [],
		};
	}

	// --- Load data ---
	onMount(async () => {
		loading = true;
		try {
			const devList = await api.getDevices();
			devices = devList;
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
	const MULTI_STATE_TYPES = new Set(['multi-state-input', 'multi-state-output', 'multi-state-value']);

	function isMultiState(objectType: string): boolean {
		return MULTI_STATE_TYPES.has(objectType);
	}

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

	/** Raw BACnet value as a string for display. */
	function rawValue(p: BacnetPoint): string {
		if (typeof p.presentValue === 'boolean') return p.presentValue ? 'Active' : 'Inactive';
		return String(p.presentValue);
	}

	/** Resolved multi-state label, or null if not applicable. */
	function resolvedStateLabel(p: BacnetPoint, cfg: PointConfig): string | null {
		if (!isMultiState(p.objectType)) return null;
		if (!cfg.stateText || cfg.stateText.length === 0) return null;
		const stateNum = typeof p.presentValue === 'number'
			? p.presentValue
			: parseFloat(String(p.presentValue));
		if (isNaN(stateNum) || stateNum < 1) return null;
		const label = cfg.stateText[Math.round(stateNum) - 1];
		return label ?? null;
	}

	/** Computed (scaled) value for numeric non-multistate points when scale/offset is applied. */
	function computedValue(p: BacnetPoint, cfg: PointConfig): string | null {
		if (!isNumericType(p.objectType)) return null;
		if (isMultiState(p.objectType)) return null;
		if (cfg.scale === 1 && cfg.offset === 0) return null;
		const raw = typeof p.presentValue === 'number' ? p.presentValue : parseFloat(String(p.presentValue));
		if (isNaN(raw)) return null;
		const converted = raw * cfg.scale + cfg.offset;
		const label = unitLabel(cfg.engineeringUnit);
		if (label === 'No Units') return converted.toFixed(3).replace(/\.?0+$/, '') || '0';
		return `${converted.toFixed(3).replace(/\.?0+$/, '') || '0'} ${label}`;
	}

	// --- Mutation helpers ---
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

	function onDashChange(p: BacnetPoint, e: Event) {
		updateConfig(p, { showOnDashboard: (e.target as HTMLInputElement).checked });
	}

	function onApiChange(p: BacnetPoint, e: Event) {
		updateConfig(p, { exposeInApi: (e.target as HTMLInputElement).checked });
	}

	function onStateTextInput(p: BacnetPoint, e: Event) {
		const raw = (e.target as HTMLInputElement).value;
		const labels = raw.split(',').map(s => s.trim()).filter(s => s.length > 0);
		updateConfig(p, { stateText: labels });
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
				<colgroup>
					<!-- BACnet MS/TP: Type, Name, Value, Unit (4 cols) -->
					<col style="width: 52px" />
					<col />
					<col style="width: 120px" />
					<col style="width: 90px" />
					<!-- Conversion: Scale, Offset, Unit, Mapped Value (4 cols) -->
					<col style="width: 80px" />
					<col style="width: 80px" />
					<col style="width: 100px" />
					<col style="width: 140px" />
					<!-- Exposure: Dash, B/IP, MQTT, API (4 cols) -->
					<col style="width: 45px" />
					<col style="width: 45px" />
					<col style="width: 45px" />
					<col style="width: 45px" />
				</colgroup>
				<thead>
					<tr class="super-header">
						<th colspan="4" class="group-bacnet">BACnet MS/TP</th>
						<th colspan="4" class="group-conversion">Conversion</th>
						<th colspan="4" class="group-exposure">Exposure</th>
					</tr>
					<tr class="sub-header">
						<!-- BACnet MS/TP group -->
						<th class="border-group-start">Type</th>
						<th>Name</th>
						<th>Value</th>
						<th>Unit</th>
						<!-- Conversion group -->
						<th class="border-group-start">Scale</th>
						<th>Offset</th>
						<th>Unit</th>
						<th>Mapped</th>
						<!-- Exposure group -->
						<th class="border-group-start" style="text-align: center">Dash</th>
						<th style="text-align: center">B/IP</th>
						<th style="text-align: center">MQTT</th>
						<th style="text-align: center">API</th>
					</tr>
				</thead>
				<tbody>
					{#each pagedRows as row (row.key)}
						{@const numeric = isNumericType(row.point.objectType)}
						{@const multiState = isMultiState(row.point.objectType)}
						{@const isDirty = dirty.has(row.key)}
						{@const stateLabel = resolvedStateLabel(row.point, row.config)}
						{@const mapped = computedValue(row.point, row.config)}
						{@const dUnit = row.point.discoveredUnit}
						<tr class:row-dirty={isDirty}>
							<!-- Type badge -->
							<td class="border-group-start">
								<span class={badgeClass(row.point.objectType)}>{badgeLabel(row.point.objectType)}</span>
							</td>
							<!-- Name + description -->
							<td class="cell-name">
								<span class="point-name">{row.point.objectName}</span>
								{#if row.point.description}
									<span class="point-desc text-sub text-xs">{row.point.description}</span>
								{/if}
							</td>
							<!-- BACnet raw value -->
							<td class="cell-value mono">
								<span class="raw-value">{rawValue(row.point)}</span>
								{#if multiState}
									<div class="state-text-input-wrap">
										<input
											class="vui-input compact-state-text"
											type="text"
											placeholder="Off,Heat,Cool,…"
											value={row.config.stateText.join(', ')}
											oninput={(e) => onStateTextInput(row.point, e)}
											title="Comma-separated state labels (1-based)"
										/>
									</div>
								{/if}
							</td>
							<!-- Discovered unit (read-only) -->
							<td class="cell-discovered-unit">
								{#if dUnit > 0 && dUnit !== 95}
									<span class="discovered-unit-badge" title="Reported by device">{unitLabel(dUnit)}</span>
								{:else}
									<span class="text-sub text-sm">—</span>
								{/if}
							</td>
							<!-- Scale -->
							<td class="cell-input border-group-start">
								{#if numeric && !multiState}
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
							<!-- Offset -->
							<td class="cell-input">
								{#if numeric && !multiState}
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
							<!-- Conversion unit -->
							<td class="cell-input">
								{#if numeric && !multiState}
									<select
										class="vui-input compact-select"
										value={row.config.engineeringUnit}
										onchange={(e) => onUnitChange(row.point, e)}
									>
										{#each ENGINEERING_UNITS as unit}
											<option value={unit.code}>{unit.label}</option>
										{/each}
									</select>
								{:else if multiState && stateLabel !== null}
									<span class="computed-value">{stateLabel}</span>
								{:else}
									<span class="text-sub text-sm">—</span>
								{/if}
							</td>
							<!-- Mapped value -->
							<td class="cell-mapped mono">
								{#if multiState && stateLabel !== null}
									<span class="computed-value">{stateLabel}</span>
								{:else if !multiState && mapped !== null}
									<span class="computed-value">{mapped}</span>
								{:else}
									<span class="text-sub text-sm">—</span>
								{/if}
							</td>
							<!-- Dash -->
							<td class="col-check border-group-start">
								<input
									type="checkbox"
									class="vui-checkbox"
									checked={row.config.showOnDashboard}
									onchange={(e) => onDashChange(row.point, e)}
								/>
							</td>
							<!-- B/IP -->
							<td class="col-check">
								<input
									type="checkbox"
									class="vui-checkbox"
									checked={$exposureConfig.bacnetIpEnabled && row.config.bridgeToBacnetIp}
									disabled={!$exposureConfig.bacnetIpEnabled}
									onchange={(e) => onBacnetIpChange(row.point, e)}
								/>
							</td>
							<!-- MQTT -->
							<td class="col-check">
								<input
									type="checkbox"
									class="vui-checkbox"
									checked={$exposureConfig.mqttEnabled && row.config.bridgeToMqtt}
									disabled={!$exposureConfig.mqttEnabled}
									onchange={(e) => onMqttChange(row.point, e)}
								/>
							</td>
							<!-- API -->
							<td class="col-check">
								<input
									type="checkbox"
									class="vui-checkbox"
									checked={$exposureConfig.apiEnabled && row.config.exposeInApi}
									disabled={!$exposureConfig.apiEnabled}
									onchange={(e) => onApiChange(row.point, e)}
								/>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>

		<!-- Pagination -->
		<div class="pagination-bar">
			<div class="pagination-left">
				<label class="text-sm text-sub" for="page-size-select">Rows:</label>
				<select
					id="page-size-select"
					class="vui-input page-size-select"
					value={pageSize}
					onchange={(e) => {
						pageSize = parseInt((e.target as HTMLSelectElement).value, 10);
						currentPage = 1;
					}}
				>
					{#each PAGE_SIZE_OPTIONS as size}
						<option value={size}>{size === 0 ? 'All' : size}</option>
					{/each}
				</select>
			</div>
			<div class="pagination-center">
				{#if pageSize !== 0}
					<span class="text-sm text-sub">
						Page {currentPage} of {totalPages}
						({filteredRows.length} total)
					</span>
				{:else}
					<span class="text-sm text-sub">
						All {filteredRows.length} rows
					</span>
				{/if}
			</div>
			<div class="pagination-right">
				<button
					class="vui-btn vui-btn-primary"
					onclick={() => { if (currentPage > 1) currentPage -= 1; }}
					disabled={currentPage <= 1 || pageSize === 0}
				>
					&laquo; Prev
				</button>
				<button
					class="vui-btn vui-btn-primary"
					onclick={() => { if (currentPage < totalPages) currentPage += 1; }}
					disabled={currentPage >= totalPages || pageSize === 0}
				>
					Next &raquo;
				</button>
			</div>
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

	/* Super-header row */
	.super-header th {
		text-align: left;
		padding: 4px var(--vui-space-sm);
		font-size: var(--vui-text-xs);
		font-weight: var(--vui-font-medium);
		color: var(--vui-text-muted);
		letter-spacing: 0.03em;
		position: sticky;
		top: 0;
		background: var(--vui-surface, var(--vui-bg));
		z-index: 2;
		white-space: nowrap;
		border-bottom: none;
	}

	.group-bacnet {
		border-left: 2px solid var(--vui-accent, #16a34a);
		padding-left: 8px;
	}

	.group-conversion {
		border-left: 2px solid var(--vui-border-accent, #7c3aed);
		padding-left: 8px;
		color: var(--vui-text-muted);
	}

	.group-exposure {
		border-left: 2px solid var(--vui-border, #e5e7eb);
		padding-left: 8px;
		color: var(--vui-text-muted);
	}

	/* Sub-header row (column labels) */
	.sub-header th {
		text-align: left;
		padding: var(--vui-space-sm) var(--vui-space-sm);
		font-size: var(--vui-text-xs);
		font-weight: var(--vui-font-semibold);
		color: var(--vui-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.05em;
		border-bottom: 1px solid var(--vui-border);
		position: sticky;
		top: 25px; /* height of super-header row */
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

	/* Group divider — left border on the first column of each group */
	.border-group-start {
		border-left: 1px solid color-mix(in srgb, var(--vui-border) 60%, transparent);
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
		vertical-align: top;
	}

	.cell-mapped {
		font-family: var(--vui-font-mono, monospace);
		font-size: var(--vui-text-sm);
	}

	/* Discovered unit cell — read-only, muted badge style */
	.cell-discovered-unit {
		font-size: var(--vui-text-xs);
	}

	.discovered-unit-badge {
		display: inline-block;
		padding: 1px 6px;
		border-radius: 4px;
		background: color-mix(in srgb, var(--vui-border) 40%, transparent);
		color: var(--vui-text-muted);
		font-size: var(--vui-text-xs);
		font-family: var(--vui-font-mono, monospace);
	}

	.raw-value {
		display: block;
	}

	.computed-value {
		color: var(--vui-accent, #16a34a);
		font-weight: var(--vui-font-medium);
	}

	.state-text-input-wrap {
		margin-top: 3px;
	}

	.compact-state-text {
		width: 110px;
		padding: 3px 6px;
		font-size: var(--vui-text-xs);
		font-family: var(--vui-font-mono, monospace);
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

	.col-check {
		text-align: center;
		padding: 4px 0;
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

	/* Pagination bar */
	.pagination-bar {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--vui-space-md);
		flex-shrink: 0;
		padding: var(--vui-space-xs) 0;
	}

	.pagination-left {
		display: flex;
		align-items: center;
		gap: var(--vui-space-xs);
	}

	.pagination-center {
		flex: 1;
		text-align: center;
	}

	.pagination-right {
		display: flex;
		align-items: center;
		gap: var(--vui-space-xs);
	}

	.page-size-select {
		width: 70px;
		padding: 4px 6px;
		font-size: var(--vui-text-sm);
	}
</style>
