<script lang="ts">
	import { onMount } from 'svelte';
	import { api, ENGINEERING_UNITS, MOCK_CONVERTORS } from '$lib/api';
	import type { Convertor, ProcessorDef } from '$lib/api';

	// ---------------------------------------------------------------------------
	// State
	// ---------------------------------------------------------------------------

	let convertors: Convertor[] = $state([]);
	let loading = $state(true);
	let saveStatus: 'idle' | 'saving' | 'success' | 'error' = $state('idle');
	let saveMessage = $state('');

	// Form state
	let editingId: string | null = $state(null); // null = new, string = existing
	let formName = $state('');
	let formId = $state('');
	let formIdManual = $state(false); // true once user manually edits the ID
	let formProcessors: ProcessorDef[] = $state([]);
	let showForm = $state(false);

	// ---------------------------------------------------------------------------
	// Load
	// ---------------------------------------------------------------------------

	onMount(async () => {
		loading = true;
		try {
			convertors = await api.getConvertors();
		} finally {
			loading = false;
		}
	});

	// ---------------------------------------------------------------------------
	// ID auto-generation from name
	// ---------------------------------------------------------------------------

	function nameToId(name: string): string {
		return name
			.toLowerCase()
			.replace(/[°→]/g, '')
			.replace(/\s+/g, '-')
			.replace(/[^a-z0-9-]/g, '')
			.replace(/-+/g, '-')
			.replace(/^-|-$/g, '')
			.slice(0, 16);
	}

	function onNameInput(e: Event) {
		formName = (e.target as HTMLInputElement).value;
		if (!formIdManual) {
			formId = nameToId(formName);
		}
	}

	function onIdInput(e: Event) {
		formId = (e.target as HTMLInputElement).value;
		formIdManual = true;
	}

	// ---------------------------------------------------------------------------
	// Form open/close
	// ---------------------------------------------------------------------------

	function openNew() {
		editingId = null;
		formName = '';
		formId = '';
		formIdManual = false;
		formProcessors = [];
		showForm = true;
	}

	function openEdit(c: Convertor) {
		editingId = c.id;
		formName = c.name;
		formId = c.id;
		formIdManual = true;
		formProcessors = c.processors.map(p => ({ ...p } as ProcessorDef));
		showForm = true;
	}

	function closeForm() {
		showForm = false;
		editingId = null;
	}

	// ---------------------------------------------------------------------------
	// Processor management
	// ---------------------------------------------------------------------------

	function addProcessor(type: ProcessorDef['type']) {
		if (type === 'set_unit') {
			formProcessors = [...formProcessors, { type: 'set_unit', unit: 95 }];
		} else if (type === 'scale') {
			formProcessors = [...formProcessors, { type: 'scale', factor: 1.0, offset: 0.0 }];
		} else {
			formProcessors = [...formProcessors, { type: 'map_states', labels: [] }];
		}
	}

	function removeProcessor(idx: number) {
		formProcessors = formProcessors.filter((_, i) => i !== idx);
	}

	function moveUp(idx: number) {
		if (idx === 0) return;
		const arr = [...formProcessors];
		[arr[idx - 1], arr[idx]] = [arr[idx], arr[idx - 1]];
		formProcessors = arr;
	}

	function moveDown(idx: number) {
		if (idx === formProcessors.length - 1) return;
		const arr = [...formProcessors];
		[arr[idx], arr[idx + 1]] = [arr[idx + 1], arr[idx]];
		formProcessors = arr;
	}

	function updateProcessor(idx: number, patch: Partial<ProcessorDef>) {
		formProcessors = formProcessors.map((p, i) =>
			i === idx ? { ...p, ...patch } as ProcessorDef : p
		);
	}

	function onLabelsInput(idx: number, e: Event) {
		const raw = (e.target as HTMLInputElement).value;
		const labels = raw.split(',').map(s => s.trim()).filter(s => s.length > 0);
		updateProcessor(idx, { type: 'map_states', labels } as ProcessorDef);
	}

	// ---------------------------------------------------------------------------
	// Save convertor
	// ---------------------------------------------------------------------------

	async function saveConvertor() {
		if (!formId || !formName) return;
		const convertor: Convertor = {
			id: formId,
			name: formName,
			processors: formProcessors,
		};

		const newList =
			editingId === null
				? [...convertors, convertor]
				: convertors.map(c => (c.id === editingId ? convertor : c));

		saveStatus = 'saving';
		try {
			await api.setConvertors(newList);
			convertors = newList;
			saveStatus = 'success';
			saveMessage = editingId === null ? 'Convertor created.' : 'Convertor updated.';
			closeForm();
		} catch (err) {
			saveStatus = 'error';
			saveMessage = `Save failed: ${err instanceof Error ? err.message : String(err)}`;
		}
		setTimeout(() => { saveStatus = 'idle'; saveMessage = ''; }, 3000);
	}

	// ---------------------------------------------------------------------------
	// Delete convertor
	// ---------------------------------------------------------------------------

	async function deleteConvertor(id: string) {
		const newList = convertors.filter(c => c.id !== id);
		saveStatus = 'saving';
		try {
			await api.setConvertors(newList);
			convertors = newList;
			saveStatus = 'success';
			saveMessage = 'Convertor deleted.';
		} catch (err) {
			saveStatus = 'error';
			saveMessage = `Delete failed: ${err instanceof Error ? err.message : String(err)}`;
		}
		setTimeout(() => { saveStatus = 'idle'; saveMessage = ''; }, 3000);
	}

	// ---------------------------------------------------------------------------
	// Preview helper
	// ---------------------------------------------------------------------------

	function processorSummary(p: ProcessorDef): string {
		if (p.type === 'set_unit') {
			const label = ENGINEERING_UNITS.find(u => u.code === p.unit)?.label ?? String(p.unit);
			return `Unit: ${label}`;
		}
		if (p.type === 'scale') {
			if (p.offset === 0) return `× ${p.factor}`;
			return `× ${p.factor} ${p.offset >= 0 ? '+' : ''}${p.offset}`;
		}
		if (p.type === 'map_states') {
			return `States: ${p.labels.join(', ') || '(none)'}`;
		}
		return '';
	}

	function convertorPreview(c: Convertor): string {
		if (c.processors.length === 0) return 'No processors (identity)';
		return c.processors.map(processorSummary).join(' → ');
	}
</script>

<div class="page-root">
	<div class="page-header">
		<div class="header-left">
			<h1 class="vui-page-title">Convertors</h1>
			<span class="text-sm text-sub">{convertors.length} convertor{convertors.length !== 1 ? 's' : ''} defined</span>
		</div>
		<div class="header-right">
			{#if saveStatus === 'success'}
				<span class="vui-badge vui-badge-success">{saveMessage}</span>
			{:else if saveStatus === 'error'}
				<span class="vui-badge vui-badge-danger">{saveMessage}</span>
			{/if}
			<button class="vui-btn vui-btn-primary" onclick={openNew}>+ New Convertor</button>
		</div>
	</div>

	{#if loading}
		<div class="empty-state"><span class="text-sub">Loading…</span></div>
	{:else if convertors.length === 0 && !showForm}
		<div class="empty-state">
			<span class="text-sub">No convertors yet. Create one to start transforming point values.</span>
		</div>
	{:else}
		<div class="table-wrap vui-card">
			<table>
				<thead>
					<tr>
						<th>Name</th>
						<th>ID</th>
						<th>Processors</th>
						<th style="text-align:right">Actions</th>
					</tr>
				</thead>
				<tbody>
					{#each convertors as c (c.id)}
						<tr>
							<td class="cell-name">{c.name}</td>
							<td class="cell-id">{c.id}</td>
							<td class="cell-preview">{convertorPreview(c)}</td>
							<td class="cell-actions">
								<button class="vui-btn vui-btn-sm vui-btn-primary" onclick={() => openEdit(c)}>Edit</button>
								<button
									class="vui-btn vui-btn-sm vui-btn-danger"
									onclick={() => deleteConvertor(c.id)}
								>Delete</button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}

	<!-- Inline form -->
	{#if showForm}
		<div class="form-panel vui-card">
			<h2 class="form-title">{editingId === null ? 'New Convertor' : 'Edit Convertor'}</h2>

			<div class="form-row">
				<label class="form-label" for="conv-name">Name</label>
				<input
					id="conv-name"
					class="vui-input form-input"
					type="text"
					placeholder="e.g. Temperature (°C)"
					value={formName}
					oninput={onNameInput}
				/>
			</div>

			<div class="form-row">
				<label class="form-label" for="conv-id">ID</label>
				<input
					id="conv-id"
					class="vui-input form-input"
					type="text"
					placeholder="e.g. temp-c"
					maxlength="16"
					value={formId}
					oninput={onIdInput}
				/>
				<span class="form-hint text-xs text-sub">Max 16 chars, auto-generated from name</span>
			</div>

			<!-- Processor list -->
			<div class="proc-section">
				<div class="proc-header">
					<span class="text-sm" style="font-weight: var(--vui-font-semibold)">Processors</span>
					<div class="proc-add-row">
						<span class="text-xs text-sub">Add:</span>
						<button class="vui-btn vui-btn-sm" onclick={() => addProcessor('set_unit')}>Set Unit</button>
						<button class="vui-btn vui-btn-sm" onclick={() => addProcessor('scale')}>Scale</button>
						<button class="vui-btn vui-btn-sm" onclick={() => addProcessor('map_states')}>Map States</button>
					</div>
				</div>

				{#if formProcessors.length === 0}
					<p class="text-xs text-sub proc-empty">No processors — value passes through unchanged.</p>
				{:else}
					<div class="proc-list">
						{#each formProcessors as proc, idx (idx)}
							<div class="proc-item">
								<div class="proc-order">
									<button
										class="vui-btn vui-btn-xs"
										onclick={() => moveUp(idx)}
										disabled={idx === 0}
										title="Move up">↑</button>
									<button
										class="vui-btn vui-btn-xs"
										onclick={() => moveDown(idx)}
										disabled={idx === formProcessors.length - 1}
										title="Move down">↓</button>
								</div>

								<div class="proc-body">
									{#if proc.type === 'set_unit'}
										<span class="proc-type-badge">Set Unit</span>
										<select
											class="vui-input proc-unit-select"
											value={proc.unit}
											onchange={(e) => updateProcessor(idx, { type: 'set_unit', unit: parseInt((e.target as HTMLSelectElement).value, 10) })}
										>
											{#each ENGINEERING_UNITS as u}
												<option value={u.code}>{u.label}</option>
											{/each}
										</select>
									{:else if proc.type === 'scale'}
										<span class="proc-type-badge">Scale</span>
										<label class="proc-field-label text-xs">×</label>
										<input
											class="vui-input proc-num"
											type="number"
											step="0.0001"
											value={proc.factor}
											oninput={(e) => updateProcessor(idx, { type: 'scale', factor: parseFloat((e.target as HTMLInputElement).value) || 1, offset: proc.offset })}
										/>
										<label class="proc-field-label text-xs">+</label>
										<input
											class="vui-input proc-num"
											type="number"
											step="0.01"
											value={proc.offset}
											oninput={(e) => updateProcessor(idx, { type: 'scale', factor: proc.factor, offset: parseFloat((e.target as HTMLInputElement).value) || 0 })}
										/>
									{:else if proc.type === 'map_states'}
										<span class="proc-type-badge">Map States</span>
										<input
											class="vui-input proc-states"
											type="text"
											placeholder="Off, Heat, Cool, Auto"
											value={proc.labels.join(', ')}
											oninput={(e) => onLabelsInput(idx, e)}
										/>
										<span class="text-xs text-sub">comma-separated, 1-based</span>
									{/if}
								</div>

								<button
									class="vui-btn vui-btn-xs vui-btn-danger"
									onclick={() => removeProcessor(idx)}
									title="Remove">✕</button>
							</div>
						{/each}
					</div>
				{/if}
			</div>

			<!-- Preview -->
			{#if formProcessors.length > 0}
				<div class="preview-row text-xs text-sub">
					Preview: {formProcessors.map(processorSummary).join(' → ')}
				</div>
			{/if}

			<div class="form-actions">
				<button class="vui-btn" onclick={closeForm}>Cancel</button>
				<button
					class="vui-btn vui-btn-primary"
					onclick={saveConvertor}
					disabled={!formId || !formName || saveStatus === 'saving'}
				>
					{saveStatus === 'saving' ? 'Saving…' : 'Save Convertor'}
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
		overflow: auto;
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

	.empty-state {
		display: flex;
		align-items: center;
		justify-content: center;
		flex: 1;
		min-height: 120px;
	}

	/* Table */
	.table-wrap {
		padding: 0;
		border-radius: var(--vui-radius);
		overflow-x: auto;
	}

	table {
		width: 100%;
		border-collapse: collapse;
	}

	/* thead th styled by design system */

	td {
		padding: 8px var(--vui-space-md);
		border-bottom: 1px solid var(--vui-border);
		font-size: var(--vui-text-sm);
		vertical-align: middle;
	}

	tr:last-child td { border-bottom: none; }
	tr:hover td { background: var(--vui-surface-hover); }

	.cell-name { font-weight: var(--vui-font-medium); min-width: 160px; }
	.cell-id { min-width: 120px; }
	.cell-preview { }
	.cell-actions { text-align: right; white-space: nowrap; }
	.cell-actions .vui-btn { margin-left: 6px; }

	/* Form panel */
	.form-panel {
		padding: var(--vui-space-lg);
		display: flex;
		flex-direction: column;
		gap: var(--vui-space-md);
		border-radius: var(--vui-radius);
	}

	.form-title {
		font-size: var(--vui-text-lg);
		font-weight: var(--vui-font-semibold);
		margin: 0;
	}

	.form-row {
		display: flex;
		align-items: center;
		gap: var(--vui-space-sm);
	}

	.form-label {
		font-size: var(--vui-text-sm);
		font-weight: var(--vui-font-medium);
		min-width: 60px;
		color: var(--vui-text-sub);
	}

	.form-input {
		width: 280px;
		font-size: var(--vui-text-sm);
	}

	.form-hint {
		margin-left: 4px;
	}

	/* Processors section */
	.proc-section {
		display: flex;
		flex-direction: column;
		gap: var(--vui-space-sm);
		border: 1px solid var(--vui-border);
		border-radius: var(--vui-radius-md);
		padding: var(--vui-space-md);
	}

	.proc-header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: var(--vui-space-md);
	}

	.proc-add-row {
		display: flex;
		align-items: center;
		gap: var(--vui-space-xs);
	}

	.proc-empty {
		margin: var(--vui-space-sm) 0 0;
		font-style: italic;
	}

	.proc-list {
		display: flex;
		flex-direction: column;
		gap: 6px;
		margin-top: 4px;
	}

	.proc-item {
		display: flex;
		align-items: center;
		gap: var(--vui-space-sm);
		background: var(--vui-surface-hover);
		border-radius: var(--vui-radius-sm);
		padding: 6px 8px;
	}

	.proc-order {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.proc-body {
		display: flex;
		align-items: center;
		gap: var(--vui-space-xs);
		flex: 1;
		flex-wrap: wrap;
	}

	.proc-type-badge {
		font-size: var(--vui-text-xs);
		font-weight: var(--vui-font-semibold);
		background: var(--vui-accent-dim);
		color: var(--vui-accent);
		border-radius: 4px;
		padding: 2px 8px;
		white-space: nowrap;
	}

	.proc-unit-select {
		width: 110px;
		padding: 4px 6px;
		font-size: var(--vui-text-sm);
	}

	.proc-num {
		width: 80px;
		padding: 4px 6px;
		font-size: var(--vui-text-sm);
		text-align: right;
	}

	.proc-states {
		width: 220px;
		padding: 4px 6px;
		font-size: var(--vui-text-sm);
	}

	.proc-field-label {
		color: var(--vui-text-muted);
	}

	.preview-row {
		padding: 6px 10px;
		background: var(--vui-surface-hover);
		border-radius: var(--vui-radius-sm);
		font-style: italic;
	}

	.form-actions {
		display: flex;
		gap: var(--vui-space-sm);
		justify-content: flex-end;
		padding-top: var(--vui-space-sm);
		border-top: 1px solid var(--vui-border);
	}

	:global(.vui-btn-xs) {
		padding: 2px 6px;
		font-size: var(--vui-text-xs);
		min-width: 24px;
	}
	:global(.vui-btn-danger) {
		background: color-mix(in srgb, var(--vui-danger, #dc2626) 15%, transparent);
		color: var(--vui-danger, #dc2626);
		border-color: color-mix(in srgb, var(--vui-danger, #dc2626) 40%, transparent);
	}
	:global(.vui-btn-danger:hover) {
		background: color-mix(in srgb, var(--vui-danger, #dc2626) 25%, transparent);
	}
</style>
