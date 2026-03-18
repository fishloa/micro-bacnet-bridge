<script lang="ts">
	import { api } from '$lib/api';
	import { goto } from '$app/navigation';

	let username = $state('');
	let password = $state('');
	let error = $state('');
	let loading = $state(false);

	async function handleLogin(e: Event) {
		e.preventDefault();
		if (!username || !password) {
			error = 'Username and password are required.';
			return;
		}
		error = '';
		loading = true;
		try {
			const result = await api.login(username, password);
			if (result.ok && result.token) {
				localStorage.setItem('auth_token', result.token);
				localStorage.setItem('auth_role', result.role ?? 'viewer');
				await goto('/');
			} else {
				error = 'Invalid username or password.';
			}
		} catch (err) {
			error = err instanceof Error ? err.message : 'Login failed.';
		} finally {
			loading = false;
		}
	}
</script>

<svelte:head>
	<title>Login — BACnet Bridge</title>
</svelte:head>

<div class="login-page">
	<div class="vui-card login-card vui-animate-fade-in">
		<div class="login-logo">
			<span class="logo-mark">B</span>
			<h1>BACnet Bridge</h1>
			<p class="text-sub">Icomb Place</p>
		</div>

		{#if error}
			<div class="vui-alert vui-alert-danger" style="margin-bottom: var(--vui-space-md);">
				{error}
			</div>
		{/if}

		<form onsubmit={handleLogin} class="login-form">
			<div class="vui-input-group">
				<label for="username">Username</label>
				<input
					id="username"
					class="vui-input"
					type="text"
					bind:value={username}
					autocomplete="username"
					placeholder="admin"
					required
					disabled={loading}
				/>
			</div>
			<div class="vui-input-group">
				<label for="password">Password</label>
				<input
					id="password"
					class="vui-input"
					type="password"
					bind:value={password}
					autocomplete="current-password"
					placeholder="••••••••"
					required
					disabled={loading}
				/>
			</div>
			<button
				type="submit"
				class="vui-btn vui-btn-primary login-btn"
				disabled={loading}
			>
				{loading ? 'Signing in…' : 'Sign In'}
			</button>
		</form>
	</div>
</div>

<style>
	.login-page {
		display: flex;
		align-items: center;
		justify-content: center;
		min-height: 100%;
		padding: var(--vui-space-xl);
	}

	.login-card {
		width: 100%;
		max-width: 380px;
		padding: var(--vui-space-xl);
	}

	.login-logo {
		text-align: center;
		margin-bottom: var(--vui-space-xl);
	}

	.logo-mark {
		display: inline-flex;
		align-items: center;
		justify-content: center;
		width: 48px;
		height: 48px;
		border-radius: var(--vui-radius-lg);
		background: var(--vui-accent);
		color: white;
		font-size: 24px;
		font-weight: var(--vui-font-bold);
		margin-bottom: var(--vui-space-sm);
	}

	.login-logo h1 {
		font-size: var(--vui-text-xl);
		font-weight: var(--vui-font-bold);
		margin-bottom: 4px;
	}

	.login-form {
		display: flex;
		flex-direction: column;
		gap: var(--vui-space-md);
	}

	.login-form label {
		display: block;
		font-size: var(--vui-text-sm);
		font-weight: var(--vui-font-medium);
		color: var(--vui-text-sub);
		margin-bottom: 4px;
	}

	.login-btn {
		width: 100%;
		justify-content: center;
		margin-top: var(--vui-space-sm);
	}
</style>
