<script lang="ts">
	import { onMount } from 'svelte';
	import { dev } from '$app/environment';
	import { api } from '$lib/api';
	import type { User } from '$lib/api';

	let users: User[] = $state([]);
	let showCreate = $state(false);
	let newUsername = $state('');
	let newPassword = $state('');
	let newRole: 'admin' | 'viewer' = $state('viewer');
	let errorMsg = $state('');

	onMount(async () => {
		try {
			users = await api.getUsers();
		} catch (e) {
			errorMsg = `Failed to load users: ${e instanceof Error ? e.message : String(e)}`;
		}
	});

	async function createUser() {
		if (!newUsername || !newPassword) return;
		errorMsg = '';
		try {
			await api.createUser(newUsername, newPassword, newRole);
			users = await api.getUsers();
			newUsername = '';
			newPassword = '';
			showCreate = false;
		} catch (e) {
			if (dev) {
				// In dev mode the API mock always succeeds synchronously; an error here
				// means the mock itself threw, which should not happen. Surface it.
				errorMsg = `Create failed: ${e instanceof Error ? e.message : String(e)}`;
			} else {
				errorMsg = `Failed to create user: ${e instanceof Error ? e.message : String(e)}`;
			}
		}
	}

	async function deleteUser(id: number) {
		errorMsg = '';
		try {
			await api.deleteUser(id);
			users = users.filter(u => u.id !== id);
		} catch (e) {
			errorMsg = `Failed to delete user: ${e instanceof Error ? e.message : String(e)}`;
		}
	}
</script>

<svelte:head>
	<title>Users — BACnet Bridge</title>
</svelte:head>

<div class="users-page">
	<div class="page-header">
		<h1>Users</h1>
		<button class="vui-btn vui-btn-primary vui-btn-sm" onclick={() => showCreate = !showCreate}>
			{showCreate ? 'Cancel' : '+ Add User'}
		</button>
	</div>

	{#if errorMsg}
		<div class="vui-alert vui-alert-danger" style="margin-bottom: var(--vui-space-md);" role="alert">
			{errorMsg}
		</div>
	{/if}

	{#if showCreate}
		<div class="vui-card vui-animate-fade-in" style="margin-bottom: var(--vui-space-lg);">
			<div class="vui-section-header">New User</div>
			<form class="create-form" onsubmit={(e) => { e.preventDefault(); createUser(); }}>
				<div class="vui-input-group">
					<label for="new-username">Username</label>
					<input class="vui-input" id="new-username" bind:value={newUsername} placeholder="username" required />
				</div>
				<div class="vui-input-group">
					<label for="new-password">Password</label>
					<input class="vui-input" id="new-password" type="password" bind:value={newPassword} placeholder="password" required />
				</div>
				<div class="vui-input-group">
					<label for="new-role">Role</label>
					<select id="new-role" class="vui-input" bind:value={newRole}>
						<option value="viewer">Viewer</option>
						<option value="admin">Admin</option>
					</select>
				</div>
				<button type="submit" class="vui-btn vui-btn-primary">Create</button>
			</form>
		</div>
	{/if}

	<div class="vui-card">
		<table>
			<thead>
				<tr>
					<th>Username</th>
					<th>Role</th>
					<th style="width: 80px"></th>
				</tr>
			</thead>
			<tbody>
				{#each users as user (user.id)}
					<tr>
						<td class="mono">{user.username}</td>
						<td>
							<span class="vui-badge" class:vui-badge-success={user.role === 'admin'} class:vui-badge-info={user.role === 'viewer'}>
								{user.role}
							</span>
						</td>
						<td>
							<button
								class="vui-btn vui-btn-sm vui-btn-danger"
								onclick={() => deleteUser(user.id)}
								disabled={user.username === 'admin'}
							>Delete</button>
						</td>
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>

<style>
	.users-page {
		padding: var(--vui-space-lg);
		height: 100%;
		overflow-y: auto;
		max-width: 640px;
	}
	.page-header {
		justify-content: space-between;
	}
	.create-form {
		display: flex;
		flex-direction: column;
		gap: var(--vui-space-md);
		padding: var(--vui-space-md) 0;
	}
	.create-form label {
		font-size: var(--vui-text-sm);
		color: var(--vui-text-sub);
		font-weight: var(--vui-font-medium);
	}
	table {
		width: 100%;
		border-collapse: collapse;
	}
	th {
		text-align: left;
		padding: var(--vui-space-sm);
		font-size: var(--vui-text-xs);
		color: var(--vui-text-muted);
		text-transform: uppercase;
		letter-spacing: 0.05em;
		border-bottom: 1px solid var(--vui-border);
	}
	td {
		padding: var(--vui-space-sm);
		border-bottom: 1px solid var(--vui-border);
		font-size: var(--vui-text-sm);
	}
</style>
