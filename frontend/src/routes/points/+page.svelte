<script lang="ts">
	import { onMount } from 'svelte';
	import {
		api,
		OBJECT_TYPE_INFO,
		pointKey,
		connectSSE,
	} from '$lib/api';
	import type { BacnetPoint, BacnetDevice, PointConfig, Convertor, ProcessorDef } from '$lib/api';

	// ---------------------------------------------------------------------------
	// State
	// ---------------------------------------------------------------------------

	let devices: BacnetDevice[] = $state([]);
	let selectedDeviceId: number | null = $state(null);
	let allPoints: BacnetPoint[] = $state([]);
	let configs: Map<string, PointConfig> = $state(new Map());
	let convertors: Convertor[] = $state([]);
	let dirty: Set<string> = $state(new Set());
	let filterText: string = $state('');
	let loading: boolean = $state(true);
	let saveStatus: 'idle' | 'saving' | 'success' | 'error' = $state('idle');
	let saveMessage: string = $state('');

	/** Live BACnet values keyed by `{deviceId}:{objectType}:{objectInstance}` */
	let liveValues: Map<string, string | number | boolean> = $state(new Map());

	let selectedDevice = $derived(devices.find(d => d.id === selectedDeviceId) ?? null);

	async function selectDevice(deviceId: number) {
		selectedDeviceId = deviceId;
		allPoints = await api.getPoints(deviceId);
		currentPage = 1;
	}

	// ---------------------------------------------------------------------------
	// Pagination
	// ---------------------------------------------------------------------------

	const PAGE_SIZE_OPTIONS = [25, 50, 100, 0]; // 0 = All
	let pageSize: number = $state(50);
	let currentPage: number = $state(1);

	// ---------------------------------------------------------------------------
	// Derived: merged rows
	// ---------------------------------------------------------------------------

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
		filterText;
		currentPage = 1;
	});

	// Clamp current page when total pages shrink
	$effect(() => {
		if (currentPage > totalPages) currentPage = totalPages;
	});

	// ---------------------------------------------------------------------------
	// Defaults
	// ---------------------------------------------------------------------------

	function defaultConfig(p: BacnetPoint): PointConfig {
		return {
			objectType: p.objectType,
			objectInstance: p.objectInstance,
			mode: 'passthrough',
			convertorId: '',
		};
	}

	// ---------------------------------------------------------------------------
	// Load data & SSE
	// ---------------------------------------------------------------------------

	let disconnectSSE: (() => void) | null = null;

	onMount(async () => {
		loading = true;
		try {
			const [devList, cfgList, convList] = await Promise.all([
				api.getDevices(),
				api.getPointConfigs(),
				api.getConvertors(),
			]);

			devices = devList;
			convertors = convList;

			const map = new Map<string, PointConfig>();
			for (const cfg of cfgList) {
				map.set(`${cfg.objectType}:${cfg.objectInstance}`, cfg);
			}
			configs = map;

			const targetDevice = devList.find(d => d.online) ?? devList[0];
			if (targetDevice) {
				selectedDeviceId = targetDevice.id;
				allPoints = await api.getPoints(targetDevice.id);
			}
		} finally {
			loading = false;
		}

		// Connect SSE for live updates
		disconnectSSE = connectSSE((updates) => {
			const newMap = new Map(liveValues);
			for (const [k, v] of Object.entries(updates)) {
				newMap.set(k, v as string | number | boolean);
			}
			liveValues = newMap;
		});

		return () => disconnectSSE?.();
	});

	// ---------------------------------------------------------------------------
	// Helpers
	// ---------------------------------------------------------------------------

	function badgeClass(objectType: string): string {
		const info = OBJECT_TYPE_INFO[objectType];
		if (!info) return 'vui-badge';
		return `vui-badge vui-badge-${info.color}`;
	}

	function badgeLabel(objectType: string): string {
		return OBJECT_TYPE_INFO[objectType]?.label ?? objectType;
	}

	/** Live or static present value as a display string. */
	function liveValue(p: BacnetPoint): string {
		const liveKey = `${selectedDeviceId}:${p.objectType}:${p.objectInstance}`;
		const live = liveValues.get(liveKey);
		const raw = live !== undefined ? live : p.presentValue;
		if (typeof raw === 'boolean') return raw ? 'Active' : 'Inactive';
		return String(raw);
	}

	/** Apply convertor processors to a raw value string for the "Converted Value" column. */
	function convertedValue(rawStr: string, cfg: PointConfig): string | null {
		if (cfg.mode !== 'convert' || !cfg.convertorId) return null;
		const conv = convertors.find(c => c.id === cfg.convertorId);
		if (!conv || conv.processors.length === 0) return null;

		// Parse raw value
		const raw = parseFloat(rawStr);
		if (isNaN(raw) && rawStr !== 'Active' && rawStr !== 'Inactive') return null;

		let value: number | string | boolean =
			rawStr === 'Active' ? true : rawStr === 'Inactive' ? false : raw;

		// Apply processors forward
		for (const proc of conv.processors) {
			value = applyProcessorFwd(value, proc);
		}

		if (typeof value === 'boolean') return value ? 'Active' : 'Inactive';
		if (typeof value === 'number') return value.toFixed(3).replace(/\.?0+$/, '') || '0';
		return String(value);
	}

	function applyProcessorFwd(value: number | string | boolean, proc: ProcessorDef): number | string | boolean {
		if (proc.type === 'set_unit') return value; // metadata only
		if (proc.type === 'scale') {
			if (typeof value !== 'number') return value;
			return value * proc.factor + proc.offset;
		}
		if (proc.type === 'map_states') {
			if (typeof value !== 'number') return value;
			const idx = Math.round(value) - 1;
			return proc.labels[idx] ?? value;
		}
		return value;
	}

	// ---------------------------------------------------------------------------
	// Mutation helpers
	// ---------------------------------------------------------------------------

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

	function onModeChange(p: BacnetPoint, e: Event) {
		const mode = (e.target as HTMLSelectElement).value as PointConfig['mode'];
		updateConfig(p, { mode });
	}

	function onConvertorChange(p: BacnetPoint, e: Event) {
		const convertorId = (e.target as HTMLSelectElement).value;
		updateConfig(p, { convertorId });
	}

	// ---------------------------------------------------------------------------
	// Save
	// ---------------------------------------------------------------------------

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
	<!-- Device selector tabs -->
	{#if devices.length > 1}
		<div class="device-bar">
			{#each devices as device (device.id)}
				<button
					class="device-tab vui-transition"
					class:active={selectedDeviceId === device.id}
					onclick={() => selectDevice(device.id)}
				>
					<span class="device-tab-status" class:online={device.online} class:offline={!device.online}></span>
					<span class="device-tab-name">{device.name}</span>
					<span class="device-tab-id">ID {device.id}</span>
				</button>
			{/each}
		</div>
	{/if}

	<div class="page-header">
		<div class="header-left">
			<h1 class="vui-page-title">
				{#if selectedDevice}
					{selectedDevice.name}
					<span class="text-sub text-sm" style="font-weight: normal; margin-left: 8px;">Device {selectedDevice.id}</span>
				{:else}
					Points Configuration
				{/if}
			</h1>
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
					<col style="width: 52px" />    <!-- Type -->
					<col />                         <!-- Name -->
					<col style="width: 130px" />   <!-- BACnet Value -->
					<col style="width: 110px" />   <!-- Mode -->
					<col style="width: 160px" />   <!-- Convertor -->
					<col style="width: 130px" />   <!-- Converted Value -->
				</colgroup>
				<thead>
					<tr>
						<th>Type</th>
						<th>Name</th>
						<th>BACnet Value</th>
						<th>Mode</th>
						<th>Convertor</th>
						<th>Converted Value</th>
					</tr>
				</thead>
				<tbody>
					{#each pagedRows as row (row.key)}
						{@const isDirty = dirty.has(row.key)}
						{@const raw = liveValue(row.point)}
						{@const converted = convertedValue(raw, row.config)}
						{@const isConvert = row.config.mode === 'convert'}
						{@const isIgnored = row.config.mode === 'ignore'}
						<tr class:row-dirty={isDirty} class:row-ignored={isIgnored}>
							<!-- Type badge -->
							<td>
								<span class={badgeClass(row.point.objectType)}>{badgeLabel(row.point.objectType)}</span>
							</td>
							<!-- Name + description -->
							<td class="cell-name">
								<span class="point-name" class:text-muted={isIgnored}>{row.point.objectName}</span>
								{#if row.point.description}
									<span class="point-desc text-sub text-xs">{row.point.description}</span>
								{/if}
							</td>
							<!-- BACnet raw value (live) -->
							<td class="cell-value mono" class:text-muted={isIgnored}>
								{raw}
							</td>
							<!-- Mode dropdown -->
							<td class="cell-input">
								<select
									class="vui-input compact-select"
									value={row.config.mode}
									onchange={(e) => onModeChange(row.point, e)}
								>
									<option value="passthrough">Passthrough</option>
									<option value="convert">Convert</option>
									<option value="ignore">Ignore</option>
								</select>
							</td>
							<!-- Convertor dropdown (only when mode=convert) -->
							<td class="cell-input">
								{#if isConvert}
									<select
										class="vui-input compact-select-wide"
										value={row.config.convertorId}
										onchange={(e) => onConvertorChange(row.point, e)}
									>
										<option value="">— none —</option>
										{#each convertors as c (c.id)}
											<option value={c.id}>{c.name}</option>
										{/each}
									</select>
								{:else}
									<span class="text-sub text-sm">—</span>
								{/if}
							</td>
							<!-- Converted value (live computed) -->
							<td class="cell-converted mono">
								{#if isConvert && converted !== null}
									<span class="converted-value">{converted}</span>
								{:else if isIgnored}
									<span class="text-sub text-xs">ignored</span>
								{:else}
									<span class="text-sub text-sm">—</span>
								{/if}
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
					<span class="text-sm text-sub">All {filteredRows.length} rows</span>
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
	.device-bar {
		display: flex;
		gap: 4px;
		overflow-x: auto;
		flex-shrink: 0;
		padding-bottom: 2px;
	}

	.device-tab {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 10px 16px;
		border: 1px solid var(--vui-border);
		border-radius: var(--vui-radius-md);
		background: none;
		color: var(--vui-text);
		font-size: var(--vui-text-base);
		cursor: pointer;
		white-space: nowrap;
	}

	.device-tab:hover {
		background: var(--vui-surface-hover);
		color: var(--vui-text);
	}

	.device-tab.active {
		background: var(--vui-accent-dim);
		border-color: var(--vui-accent-border);
		color: var(--vui-accent);
	}

	.device-tab-status {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		flex-shrink: 0;
	}

	.device-tab-status.online {
		background: var(--vui-accent);
		box-shadow: 0 0 4px var(--vui-accent-glow);
	}

	.device-tab-status.offline {
		background: var(--vui-text-dim);
	}

	.device-tab-name { font-weight: var(--vui-font-medium); }

	.device-tab-id {
		font-size: var(--vui-text-xs);
		color: var(--vui-text-muted);
		font-family: var(--vui-font-mono, monospace);
	}

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

	.save-msg { font-size: var(--vui-text-xs); }

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

	thead th {
		text-align: left;
		padding: var(--vui-space-sm) var(--vui-space-sm);
		font-size: var(--vui-text-sm);
		font-weight: var(--vui-font-semibold);
		color: var(--vui-accent);
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

	tr:hover td { background: var(--vui-surface-hover); }

	tr.row-dirty td {
		background: color-mix(in srgb, var(--vui-accent-dim, #1e40af22) 60%, transparent);
	}

	tr.row-dirty:hover td {
		background: color-mix(in srgb, var(--vui-accent-dim, #1e40af22) 80%, var(--vui-surface-hover));
	}

	tr.row-ignored td {
		opacity: 0.5;
	}

	.cell-name {
		display: flex;
		flex-direction: column;
		gap: 1px;
	}

	.point-name { font-weight: var(--vui-font-medium); }

	.point-desc {
		color: var(--vui-text-muted);
		font-size: var(--vui-text-xs);
	}

	.cell-value {
		font-family: var(--vui-font-mono, monospace);
		font-size: var(--vui-text-sm);
		color: var(--vui-text);
	}

	.cell-converted {
		font-family: var(--vui-font-mono, monospace);
		font-size: var(--vui-text-sm);
	}

	.converted-value {
		color: var(--vui-text);
	}

	.cell-input { padding: 4px var(--vui-space-sm); }

	.compact-select {
		width: 100px;
		padding: 4px 6px;
		font-size: var(--vui-text-sm);
	}

	.compact-select-wide {
		width: 150px;
		padding: 4px 6px;
		font-size: var(--vui-text-sm);
	}

	.mono { font-family: var(--vui-font-mono, monospace); }

	.text-muted { color: var(--vui-text-muted); }

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
