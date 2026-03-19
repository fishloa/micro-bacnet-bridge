<script lang="ts">
	import { onMount } from 'svelte';
	import { api } from '$lib/api';
	import type { ApiToken } from '$lib/api';

	let tokens: ApiToken[] = $state([]);
	let loading = $state(true);
	let error = $state('');

	// Create token form
	let showCreate = $state(false);
	let newName = $state('');
	let newRole: 'admin' | 'operator' | 'viewer' = $state('viewer');
	let createLoading = $state(false);
	let newPlainToken = $state(''); // shown once after creation

	onMount(async () => {
		try {
			tokens = await api.getTokens();
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to load tokens.';
		} finally {
			loading = false;
		}
	});

	async function createToken() {
		if (!newName) return;
		createLoading = true;
		error = '';
		newPlainToken = '';
		try {
			const result = await api.createToken(newName, newRole);
			if (result.ok && result.token) {
				newPlainToken = result.token;
			}
			tokens = await api.getTokens();
			newName = '';
			newRole = 'viewer';
			showCreate = false;
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to create token.';
		} finally {
			createLoading = false;
		}
	}

	async function revokeToken(id: string) {
		if (!confirm('Revoke this token? This cannot be undone.')) return;
		try {
			await api.revokeToken(id);
			tokens = tokens.filter(t => t.id !== id);
		} catch (e) {
			error = e instanceof Error ? e.message : 'Failed to revoke token.';
		}
	}

	function copyToken() {
		navigator.clipboard.writeText(newPlainToken).catch(() => {});
	}
</script>

<svelte:head>
	<title>API Tokens — BACnet Bridge</title>
</svelte:head>

<div class="tokens-page">
	<div class="page-header">
		<h1 class="vui-page-title">API Tokens</h1>
		<button class="vui-btn vui-btn-primary vui-btn-sm" onclick={() => { showCreate = !showCreate; newPlainToken = ''; }}>
			{showCreate ? 'Cancel' : '+ New Token'}
		</button>
	</div>

	{#if error}
		<div class="vui-alert vui-alert-danger">{error}</div>
	{/if}

	<!-- Newly created token — shown once -->
	{#if newPlainToken}
		<div class="vui-alert vui-alert-success token-reveal vui-animate-fade-in">
			<strong>Token created.</strong> Copy it now — it will not be shown again.
			<div class="token-value">
				<code>{newPlainToken}</code>
				<button class="vui-btn vui-btn-sm vui-btn-ghost" onclick={copyToken}>Copy</button>
			</div>
		</div>
	{/if}

	<!-- Create token form -->
	{#if showCreate}
		<div class="vui-card vui-animate-fade-in">
			<div class="vui-section-header">New API Token</div>
			<form class="create-form" onsubmit={(e) => { e.preventDefault(); createToken(); }}>
				<div class="vui-input-group">
					<label for="token-name">Name</label>
					<input
						id="token-name"
						class="vui-input"
						bind:value={newName}
						placeholder="e.g. CI/CD pipeline"
						required
					/>
				</div>
				<div class="vui-input-group">
					<label for="token-role">Role</label>
					<select id="token-role" class="vui-input" bind:value={newRole}>
						<option value="viewer">Viewer</option>
						<option value="operator">Operator</option>
						<option value="admin">Admin</option>
					</select>
				</div>
				<button type="submit" class="vui-btn vui-btn-primary" disabled={createLoading}>
					{createLoading ? 'Creating…' : 'Create Token'}
				</button>
			</form>
		</div>
	{/if}

	<!-- Tokens table -->
	<div class="vui-card">
		{#if loading}
			<div class="vui-skeleton" style="height: 80px;"></div>
		{:else}
			<table>
				<thead>
					<tr>
						<th>Name</th>
						<th>Role</th>
						<th>Created By</th>
						<th style="width: 80px"></th>
					</tr>
				</thead>
				<tbody>
					{#each tokens as tok (tok.id)}
						<tr>
							<td>{tok.name}</td>
							<td>
								<span
									class="vui-badge"
									class:vui-badge-danger={tok.role === 'admin'}
									class:vui-badge-info={tok.role === 'operator'}
									class:vui-badge-success={tok.role === 'viewer'}
								>{tok.role}</span>
							</td>
							<td class="text-sub">{tok.createdBy}</td>
							<td>
								<button class="vui-btn vui-btn-sm vui-btn-danger" onclick={() => revokeToken(tok.id)}>
									Revoke
								</button>
							</td>
						</tr>
					{:else}
						<tr>
							<td colspan="4" class="text-sub" style="text-align: center; padding: var(--vui-space-lg);">
								No API tokens. Create one to enable programmatic access.
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		{/if}
	</div>
</div>

<style>
	.tokens-page {
		padding: var(--vui-space-lg);
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

	.token-reveal {
		display: flex;
		flex-direction: column;
		gap: var(--vui-space-sm);
	}

	.token-value {
		display: flex;
		align-items: center;
		gap: var(--vui-space-sm);
		background: var(--vui-surface);
		border: 1px solid var(--vui-border);
		border-radius: var(--vui-radius-sm);
		padding: var(--vui-space-sm) var(--vui-space-md);
	}

	.token-value code {
		flex: 1;
		font-size: var(--vui-text-sm);
		word-break: break-all;
	}

	.create-form {
		display: flex;
		flex-direction: column;
		gap: var(--vui-space-md);
		padding: var(--vui-space-sm) 0;
	}

	.create-form label {
		display: block;
		font-size: var(--vui-text-sm);
		color: var(--vui-text-sub);
		font-weight: var(--vui-font-medium);
		margin-bottom: 4px;
	}

	table {
		width: 100%;
		border-collapse: collapse;
	}

	th {
		text-align: left;
		padding: var(--vui-space-sm);
		border-bottom: 1px solid var(--vui-border);
	}

	td {
		padding: var(--vui-space-sm);
		border-bottom: 1px solid var(--vui-border);
		vertical-align: middle;
	}
</style>
