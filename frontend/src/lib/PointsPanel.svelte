<script lang="ts">
	import { filteredPoints, filterText, activeTab, tabs, points, deviceId } from './stores';
	import { OBJECT_TYPE_INFO, api, pointKey } from './api';
	import type { BacnetPoint } from './api';

	/** When true, show a "Device" column (used in the "All Devices" view). */
	let { showDeviceColumn = false }: { showDeviceColumn?: boolean } = $props();

	let editingPoint: BacnetPoint | null = $state(null);
	let editValue: string = $state('');
	let changedKeys: Set<string> = $state(new Set());
	let prevValues: Map<string, string | number | boolean> = new Map();

	// Track which values just changed for flash effect
	$effect(() => {
		const pts = $filteredPoints;
		const newChanged = new Set<string>();
		const currentKeys = new Set<string>();
		for (const p of pts) {
			const key = pointKey(p);
			currentKeys.add(key);
			const prev = prevValues.get(key);
			if (prev !== undefined && prev !== p.presentValue) {
				newChanged.add(key);
			}
			prevValues.set(key, p.presentValue);
		}
		// Prune entries for points no longer visible to prevent unbounded growth
		for (const key of prevValues.keys()) {
			if (!currentKeys.has(key)) prevValues.delete(key);
		}
		if (newChanged.size > 0) {
			changedKeys = newChanged;
			setTimeout(() => { changedKeys = new Set(); }, 800);
		}
	});

	function badgeClass(objectType: string): string {
		const info = OBJECT_TYPE_INFO[objectType];
		if (!info) return 'vui-badge';
		return `vui-badge vui-badge-${info.color}`;
	}

	function badgeLabel(objectType: string): string {
		return OBJECT_TYPE_INFO[objectType]?.label ?? objectType;
	}

	function formatValue(p: BacnetPoint): string {
		if (typeof p.presentValue === 'boolean') return p.presentValue ? 'Active' : 'Inactive';
		if (typeof p.presentValue === 'number') {
			if (p.units) return `${p.presentValue} ${p.units}`;
			return String(p.presentValue);
		}
		return String(p.presentValue);
	}

	function startEdit(p: BacnetPoint) {
		editingPoint = p;
		editValue = String(p.presentValue);
	}

	async function submitEdit() {
		if (!editingPoint) return;
		let val: string | number | boolean = editValue;
		if (editingPoint.objectType.startsWith('binary')) {
			val = editValue === 'true' || editValue === 'Active' || editValue === '1';
		} else if (!isNaN(Number(editValue))) {
			val = Number(editValue);
		}
		await api.writePoint($deviceId, editingPoint.objectType, editingPoint.objectInstance, val);
		// Update local state
		const idx = $points.findIndex(
			pp => pp.objectType === editingPoint!.objectType && pp.objectInstance === editingPoint!.objectInstance
		);
		if (idx >= 0) {
			$points[idx] = { ...$points[idx], presentValue: val };
			$points = $points;
		}
		editingPoint = null;
	}

	function cancelEdit() {
		editingPoint = null;
	}
</script>

<div class="points-panel">
		<div class="panel-header">
			<div>
				<span class="vui-page-title">Dashboard</span>
				<span class="text-sm text-sub">
					{$filteredPoints.length} of {$points.length} objects
				</span>
			</div>
		</div>

		<div class="toolbar">
			<div class="tab-bar">
				{#each tabs as tab}
					<button
						class="tab-btn vui-transition"
						class:active={$activeTab === tab.key}
						onclick={() => $activeTab = tab.key}
					>{tab.label}</button>
				{/each}
			</div>
			<div class="filter-input">
				<input
					class="vui-input"
					type="text"
					placeholder="Filter (supports regex)..."
					bind:value={$filterText}
				/>
			</div>
		</div>

		<div class="points-table">
			<table>
				<thead>
					<tr>
						<th style="width: 52px">Type</th>
						{#if showDeviceColumn}
							<th style="width: 80px">Device</th>
						{/if}
						<th>Name</th>
						<th>Description</th>
						<th style="width: 140px">Value</th>
						<th style="width: 44px"></th>
					</tr>
				</thead>
				<tbody>
					{#each $filteredPoints as point (pointKey(point))}
						<tr class="vui-transition">
							<td><span class={badgeClass(point.objectType)}>{badgeLabel(point.objectType)}</span></td>
							{#if showDeviceColumn}
								<td class="text-sub text-sm mono">{(point as BacnetPoint & { _deviceId?: number })._deviceId ?? $deviceId}</td>
							{/if}
							<td class="point-name">{point.objectName}</td>
							<td>{point.description}</td>
							<td class="point-value mono" class:value-changed={changedKeys.has(pointKey(point))}>
								{#if editingPoint?.objectType === point.objectType && editingPoint?.objectInstance === point.objectInstance}
									<form class="edit-form" onsubmit={(e) => { e.preventDefault(); submitEdit(); }}>
										<input class="vui-input edit-input" bind:value={editValue} />
										<button type="submit" class="vui-btn vui-btn-sm vui-btn-primary">✓</button>
										<button type="button" class="vui-btn vui-btn-sm vui-btn-ghost" onclick={cancelEdit}>✕</button>
									</form>
								{:else}
									<span class:bool-active={point.presentValue === true} class:bool-inactive={point.presentValue === false}>
										{formatValue(point)}
									</span>
								{/if}
							</td>
							<td>
								{#if point.writable && !(editingPoint?.objectType === point.objectType && editingPoint?.objectInstance === point.objectInstance)}
									<button class="vui-btn vui-btn-sm vui-btn-ghost write-btn" onclick={() => startEdit(point)} title="Write value">✎</button>
								{/if}
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
</div>

<style>
	.points-panel {
		display: flex;
		flex-direction: column;
		height: 100%;
		overflow: hidden;
	}
	.panel-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: var(--vui-space-md) var(--vui-space-lg);
		border-bottom: 1px solid var(--vui-border);
	}
	.panel-header h2 {
		font-size: var(--vui-text-lg);
		font-weight: var(--vui-font-semibold);
		margin-bottom: 2px;
	}
	.toolbar {
		display: flex;
		align-items: center;
		gap: var(--vui-space-md);
		padding: var(--vui-space-sm) var(--vui-space-lg);
		border-bottom: 1px solid var(--vui-border);
	}
	.tab-bar {
		display: flex;
		gap: 2px;
	}
	.tab-btn {
		padding: 8px 14px;
		border: none;
		background: none;
		color: var(--vui-text-sub);
		font-size: var(--vui-text-sm);
		font-weight: var(--vui-font-semibold);
		text-transform: uppercase;
		letter-spacing: 0.04em;
		border-radius: var(--vui-radius-sm);
		cursor: pointer;
	}
	.tab-btn:hover {
		color: var(--vui-accent);
		background: var(--vui-surface);
	}
	.tab-btn.active {
		color: var(--vui-accent);
		background: var(--vui-accent-dim);
	}
	.filter-input {
		flex: 1;
		max-width: 300px;
		margin-left: auto;
	}
	.filter-input .vui-input {
		width: 100%;
		font-size: var(--vui-text-sm);
		padding: 6px 12px;
	}
	.points-table {
		flex: 1;
		overflow-y: auto;
		padding: 0 var(--vui-space-lg);
	}
	table {
		width: 100%;
		border-collapse: collapse;
	}
	th {
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
		background: var(--vui-bg);
		z-index: 1;
	}
	td {
		padding: var(--vui-space-sm);
		border-bottom: 1px solid var(--vui-border);
		font-size: var(--vui-text-sm);
		vertical-align: middle;
	}
	tr:hover td {
		background: var(--vui-surface-hover);
	}
	.point-name {
		font-weight: var(--vui-font-medium);
	}
	.point-value {
		font-size: var(--vui-text-sm);
	}
	.value-changed {
		animation: flash 0.8s ease-out;
	}
	@keyframes flash {
		0% { background: var(--vui-accent-dim); color: var(--vui-accent); }
		100% { background: transparent; }
	}
	.bool-active {
		color: var(--vui-text);
	}
	.bool-inactive {
		color: var(--vui-text);
	}
	.write-btn {
		opacity: 0.4;
		font-size: 14px;
	}
	tr:hover .write-btn {
		opacity: 1;
	}
	.edit-form {
		display: flex;
		align-items: center;
		gap: 4px;
	}
	.edit-input {
		width: 80px;
		padding: 3px 6px;
		font-size: var(--vui-text-sm);
		font-family: var(--vui-font-mono);
	}
</style>
